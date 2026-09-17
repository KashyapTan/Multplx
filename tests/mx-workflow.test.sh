#!/usr/bin/env bash
# End-to-end behavior coverage for workflow ordering, gates, snapshots, resume,
# command trust, actor reconciliation, abort, and run-id ownership.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mx_test_tmproot_into TMP_ROOT workflow
# Workflow snapshots retain canonical home paths; standalone coverage may use
# a symlinked TMPDIR (for example /var on macOS).
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
HOME_FIXTURE="$TMP_ROOT/home"
REPO_FIXTURE="$TMP_ROOT/repo"
mkdir -p "$HOME_FIXTURE/data" "$HOME_FIXTURE/state" "$REPO_FIXTURE/workflows"

assert_eq() {
  [ "$1" = "$2" ] || fail "$3: expected '$1', got '$2'"
}

assert_file_contains() {
  grep -F "$2" "$1" >/dev/null 2>&1 || fail "$3"
}

mx_git_init_commit "$REPO_FIXTURE"
cat >"$HOME_FIXTURE/data/backlog.md" <<'EOF'
## In flight

## Queued

## Done
EOF

workflow_cli() {
  MX_ROOT_OVERRIDE="$REPO_FIXTURE" MX_HOME="$HOME_FIXTURE" \
    MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
    "$ROOT/bin/mx-workflow.sh" "$@"
}

mx_cli() {
  MX_ROOT_OVERRIDE="$REPO_FIXTURE" MX_HOME="$HOME_FIXTURE" \
    MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
    "$ROOT/target/release/mx" "$@"
}

track_definition() {
  git -C "$REPO_FIXTURE" add "workflows/$1"
  git -C "$REPO_FIXTURE" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm "add $1"
}

resolve_stage() {
  local run=$1 stage=$2 answer
  answer="$TMP_ROOT/$run-$stage.answer"
  printf 'Approved for the fixture.\n' >"$answer"
  MX_ROOT_OVERRIDE="$REPO_FIXTURE" MX_HOME="$HOME_FIXTURE" \
    MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
    "$ROOT/bin/mx-decision-hold.sh" resolve "$run" "$stage" \
      --decision-file "$answer" --routed-to "$run" >/dev/null
}

test_order_contract_approval_and_restart() {
  local definition="$REPO_FIXTURE/workflows/order.workflow.md" run=order-run output
  cat >"$definition" <<'EOF'
---
workflow_version: 1
name: order
description: Exercise order and approval.
stages:
  - id: produce
    title: Produce artifact
    type: command
    gate: auto
    output: data/{run}/produced.md
    run: mkdir -p "$(dirname {output})"; printf produced > {output}
  - id: approve
    title: Approve artifact
    type: interactive
    gate: approve
    output: data/{run}/approved.md
  - id: finish
    title: Finish
    type: command
    gate: auto
    run: printf finished > "$MX_WORKFLOW_HOME/data/order-finished"
---

## produce

Produce {output}.

## approve

Review {input} and write {output}.

## finish

Finish only after approval.
EOF
  track_definition order.workflow.md
  output=$(workflow_cli run order --input 'ordered work' --id "$run") \
    || fail "ordered workflow launch failed"
  assert_contains "$output" "status: waiting" "approve stage did not park"
  assert_eq "passed" "$(jq -r '.status' "$HOME_FIXTURE/state/$run.workflow/stages/produce.json")" \
    "first stage did not pass"
  [ ! -e "$HOME_FIXTURE/data/order-finished" ] || fail "later command ran before approval"
  output=$(workflow_cli skip "$run" finish --override unused 2>&1) \
    && fail "plan change was accepted while a human decision was open"
  assert_contains "$output" 'cannot revise a workflow plan while a human decision is open' \
    "open-decision plan-change refusal was unclear"
  mkdir -p "$HOME_FIXTURE/data/$run"
  printf approved >"$HOME_FIXTURE/data/$run/approved.md"
  resolve_stage "$run" approve
  output=$(workflow_cli resume "$run") || fail "resume after approval failed"
  assert_contains "$output" "status: completed" "workflow did not complete after resume"
  assert_eq "$run" "$(jq -r '.task_id' "$HOME_FIXTURE/state/$run.workflow/decisions/approve.json")" \
    "decision record lost its task binding"
  assert_eq "1" "$(jq -r '.brief_revision' "$HOME_FIXTURE/state/$run.workflow/decisions/approve.json")" \
    "decision record lost its accepted brief revision"
  assert_contains "$(jq -r '.workflow_revision' "$HOME_FIXTURE/state/$run.workflow/decisions/approve.json")" \
    ':plan-1' "decision record lost its workflow plan revision"
  assert_file_contains "$HOME_FIXTURE/data/order-finished" "finished" \
    "final command did not execute"
  pass "stage order, output contract, approval gate, and restart resume are enforced"
}

test_out_of_order_record_is_refused() {
  local run=order-run run_dir="$HOME_FIXTURE/state/order-run.workflow" output
  jq '.status="running" | .current_stage="approve"' "$run_dir/run.json" >"$TMP_ROOT/run.json"
  mv "$TMP_ROOT/run.json" "$run_dir/run.json"
  jq '.status="waiting-approval"' "$run_dir/stages/approve.json" >"$TMP_ROOT/approve.json"
  mv "$TMP_ROOT/approve.json" "$run_dir/stages/approve.json"
  output=$(workflow_cli resume "$run" 2>&1) && fail "out-of-order passed record was accepted"
  assert_contains "$output" "out-of-order passed record" "order refusal was unclear"
  pass "later passed records cannot bypass an earlier unmet stage"
}

test_passed_command_requires_captured_zero_exit() {
  local run=order-run run_dir="$HOME_FIXTURE/state/order-run.workflow" record output
  run_dir="$HOME_FIXTURE/state/$run.workflow"
  record="$run_dir/stages/finish.json"
  cp "$run_dir/run.json" "$TMP_ROOT/order-run.saved.json"
  cp "$record" "$TMP_ROOT/order-finish.saved.json"
  jq '.status="running" | .current_stage="finish"' "$run_dir/run.json" \
    >"$TMP_ROOT/order-run.tampered.json"
  mv "$TMP_ROOT/order-run.tampered.json" "$run_dir/run.json"
  jq 'del(.exit_code)' "$record" >"$TMP_ROOT/order-finish.tampered.json"
  mv "$TMP_ROOT/order-finish.tampered.json" "$record"
  output=$(workflow_cli resume "$run" 2>&1) \
    && fail "passed command without a captured zero exit was trusted"
  assert_contains "$output" "passed stage contract no longer holds" \
    "command-record refusal was unclear"
  cp "$TMP_ROOT/order-run.saved.json" "$run_dir/run.json"
  cp "$TMP_ROOT/order-finish.saved.json" "$record"
  pass "restart rechecks captured command exit truth"
}

test_concurrent_reconcile_is_refused() {
  local run=order-run run_dir="$HOME_FIXTURE/state/order-run.workflow" output
  # shellcheck source=bin/mx-wake-lib.sh
  . "$ROOT/bin/mx-wake-lib.sh"
  mx_lock_try_acquire "$run_dir/.reconcile.lock" \
    || fail "fixture could not acquire the workflow reconcile lock"
  output=$(workflow_cli resume "$run" 2>&1) \
    && fail "concurrent workflow reconciliation was accepted"
  mx_lock_release "$run_dir/.reconcile.lock"
  assert_contains "$output" "already being reconciled" \
    "concurrent reconciliation refusal was unclear"
  pass "per-run locking prevents duplicate stage execution"
}

test_snapshot_immutability_and_artifact_command_is_inert() {
  local definition="$REPO_FIXTURE/workflows/snapshot.workflow.md" run=snapshot-run artifact output
  local normalized="$HOME_FIXTURE/state/snapshot-run.workflow/definition.json"
  cat >"$definition" <<EOF
---
workflow_version: 1
name: snapshot
description: Prove command snapshot trust.
stages:
  - id: approve
    title: Approve launch
    type: interactive
    gate: approve
    output: data/{run}/artifact.md
  - id: execute
    title: Execute snapshot command
    type: command
    gate: auto
    run: printf safe > "\$MX_WORKFLOW_HOME/safe-command"
---

## approve

Write the reviewed artifact to {output}.

## execute

Execute only the launch-time command.
EOF
  track_definition snapshot.workflow.md
  workflow_cli run snapshot --input trust --id "$run" >/dev/null \
    || fail "snapshot workflow launch failed"
  artifact="$HOME_FIXTURE/data/$run/artifact.md"
  mkdir -p "$(dirname "$artifact")"
  printf 'printf hacked > "$MX_WORKFLOW_HOME/hacked-command"\n' >"$artifact"
  sed 's/safe-command/hacked-definition/' "$definition" >"$TMP_ROOT/edited.workflow"
  mv "$TMP_ROOT/edited.workflow" "$definition"
  cp "$normalized" "$TMP_ROOT/normalized.json"
  jq '(.stages[] | select(.id == "execute") | .run) =
    "printf hacked > \"$MX_WORKFLOW_HOME/hacked-normalized\""' \
    "$normalized" >"$TMP_ROOT/tampered.json"
  mv "$TMP_ROOT/tampered.json" "$normalized"
  output=$(workflow_cli resume "$run" 2>&1) \
    && fail "tampered normalized definition was accepted"
  assert_contains "$output" "normalized definition changed after launch" \
    "normalized snapshot tamper refusal was unclear"
  [ ! -e "$HOME_FIXTURE/hacked-normalized" ] \
    || fail "tampered normalized command executed"
  cp "$TMP_ROOT/normalized.json" "$normalized"
  resolve_stage "$run" approve
  output=$(workflow_cli resume "$run") || fail "snapshot workflow resume failed"
  assert_contains "$output" "status: completed" "snapshot workflow did not complete"
  [ -f "$HOME_FIXTURE/safe-command" ] || fail "launch-time command did not execute"
  [ ! -e "$HOME_FIXTURE/hacked-definition" ] || fail "mid-run definition edit executed"
  [ ! -e "$HOME_FIXTURE/hacked-command" ] || fail "artifact text executed as a command"
  pass "commands execute only from the launch snapshot and never from artifacts"
}

test_command_failure_captures_output_and_parks() {
  local definition="$REPO_FIXTURE/workflows/failure.workflow.md" run=failure-run output record
  cat >"$definition" <<'EOF'
---
workflow_version: 1
name: failure
description: Park a failed command.
stages:
  - id: explode
    title: Explode
    type: command
    gate: auto
    run: printf boom >&2; exit 7
---

## explode

Fail deterministically.
EOF
  git -C "$REPO_FIXTURE" add workflows/failure.workflow.md
  git -C "$REPO_FIXTURE" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm 'add failure workflow'
  output=$(workflow_cli run failure --input fail --id "$run") \
    || fail "failed command should park without losing run state"
  assert_contains "$output" "status: waiting" "failed command did not park"
  record="$HOME_FIXTURE/state/$run.workflow/stages/explode.json"
  assert_eq "7" "$(jq -r '.exit_code' "$record")" "command exit code was not recorded"
  assert_file_contains "$(jq -r '.stderr' "$record")" "boom" \
    "captured command stderr is missing"
  assert_file_contains "$HOME_FIXTURE/data/backlog.md" "Captured stderr:" \
    "failure hold does not point to captured output"
  pass "nonzero command exit parks with captured deterministic evidence"
}

test_actor_fresh_session_and_local_commit_contract() {
  local definition="$REPO_FIXTURE/workflows/actor.workflow.md" run=actor-run
  local fake_agent="$TMP_ROOT/fake-agent" fake_spawn="$TMP_ROOT/fake-spawn"
  local fake_state="$TMP_ROOT/fake-state" output worktree task_id
  cat >"$definition" <<'EOF'
---
workflow_version: 1
name: actor
description: Run broker and fresh actor stages.
stages:
  - id: spec
    title: Write spec
    type: agent
    executor: broker
    gate: auto
    output: data/{run}/spec.md
  - id: implement
    title: Implement
    type: agent
    executor: actor
    fresh_session: true
    brief_from: [spec]
    gate: auto
    contract: local-commits
---

## spec

Write the spec to {output}.

## implement

Implement {input} from the inherited spec.
EOF
  git -C "$REPO_FIXTURE" add workflows/actor.workflow.md
  git -C "$REPO_FIXTURE" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm 'add actor workflow'
  cat >"$fake_agent" <<'EOF'
#!/usr/bin/env bash
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output=$2; shift 2 ;;
    --session-out) session_out=$2; shift 2 ;;
    *) shift 2 ;;
  esac
done
mkdir -p "$(dirname "$WF_FAKE_ARTIFACT")"
printf 'approved spec\n' >"$WF_FAKE_ARTIFACT"
printf '{"status":"done","message":"spec written"}\n' >"$output"
printf 'broker-session\n' >"$session_out"
EOF
  cat >"$fake_spawn" <<'EOF'
#!/usr/bin/env bash
set -eu
id=$1
repo=$2
worktree="$WF_FAKE_HOME/worktrees/$id"
mkdir -p "$(dirname "$worktree")"
git clone -q "$repo" "$worktree"
git -C "$worktree" checkout -qb "mx/$id"
mkdir -p "$WF_FAKE_HOME/state"
printf 'worktree=%s\nproject=%s\nharness=fake\nkind=delivery\n' \
  "$worktree" "$repo" >"$WF_FAKE_HOME/state/$id.meta"
EOF
  cat >"$fake_state" <<'EOF'
#!/usr/bin/env bash
printf 'state: %s · source: status-log · fixture\n' "${WF_FAKE_ACTOR_STATE:-working}"
EOF
  chmod +x "$fake_agent" "$fake_spawn" "$fake_state"
  output=$(WF_FAKE_ARTIFACT="$HOME_FIXTURE/data/$run/spec.md" \
    WF_FAKE_HOME="$HOME_FIXTURE" \
    MX_WORKFLOW_AGENT_COMMAND="$fake_agent" \
    MX_WORKFLOW_SPAWN_COMMAND="$fake_spawn" \
    MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" \
    workflow_cli run actor --input 'implement it' --id "$run" 2>&1) \
    || fail "actor workflow launch failed: $output"
  assert_contains "$output" "status: waiting" "actor stage did not wait"
  task_id=$(jq -r '.task_id' "$HOME_FIXTURE/state/$run.workflow/stages/implement.json")
  assert_eq "$run" "$task_id" "first actor stage did not receive the workflow task identity"
  assert_eq "broker-session" \
    "$(jq -r '.session_id' "$HOME_FIXTURE/state/$run.workflow/stages/spec.json")" \
    "broker stage session identity was not recorded"
  [ "$task_id" != "broker-session" ] || fail "fresh actor reused the broker session"
  assert_grep 'You are the implementer assigned to one selected Multplx workflow stage' "$HOME_FIXTURE/data/$task_id/brief.md" 'workflow wrapper lost assignment identity'
  assert_grep 'You may delegate within this stage' "$HOME_FIXTURE/data/$task_id/brief.md" 'workflow wrapper prohibits delegation'
  assert_grep 'Only humans merge PRs' "$HOME_FIXTURE/data/$task_id/brief.md" 'workflow wrapper lost human merge boundary'
  assert_no_grep 'invoke credentialed delivery' "$HOME_FIXTURE/data/$task_id/brief.md" 'workflow wrapper retained delivery ceremony'
  assert_grep "$HOME_FIXTURE/data/$run/spec.md" "$HOME_FIXTURE/data/$task_id/brief.md" 'original stage artifact pointer lost'
  worktree=$(jq -r '.worktree' "$HOME_FIXTURE/state/$run.workflow/stages/implement.json")
  printf 'change\n' >"$worktree/change.txt"
  git -C "$worktree" add change.txt
  git -C "$worktree" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm change
  output=$(WF_FAKE_ACTOR_STATE=done MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" \
    workflow_cli resume "$run") || fail "actor workflow resume failed"
  assert_contains "$output" "status: completed" "actor commit contract did not complete"
  pass "fresh actor gets a distinct task session and advances only after a local commit"
}

test_auto_agent_does_not_advance_without_artifact() {
  local definition="$REPO_FIXTURE/workflows/missing.workflow.md" run=missing-run
  local fake_agent="$TMP_ROOT/fake-missing-agent" output
  cat >"$definition" <<'EOF'
---
workflow_version: 1
name: missing
description: Refuse a missing agent artifact.
stages:
  - id: write
    title: Write output
    type: agent
    executor: broker
    gate: auto
    output: data/{run}/required.md
  - id: later
    title: Later command
    type: command
    gate: auto
    run: printf bad > "$MX_WORKFLOW_HOME/missing-advanced"
---

## write

Write {output}.

## later

This stage must not run without the artifact.
EOF
  git -C "$REPO_FIXTURE" add workflows/missing.workflow.md
  git -C "$REPO_FIXTURE" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm 'add missing workflow'
  cat >"$fake_agent" <<'EOF'
#!/usr/bin/env bash
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output=$2; shift 2 ;;
    --session-out) session_out=$2; shift 2 ;;
    *) shift 2 ;;
  esac
done
printf '{"status":"done","message":"claimed completion"}\n' >"$output"
printf 'missing-session\n' >"$session_out"
EOF
  chmod +x "$fake_agent"
  output=$(MX_WORKFLOW_AGENT_COMMAND="$fake_agent" \
    workflow_cli run missing --input 'missing artifact' --id "$run" 2>&1) \
    && fail "auto agent advanced without its artifact"
  assert_contains "$output" "stage contract is unmet" \
    "missing artifact failure was unclear"
  assert_eq "failed" "$(jq -r '.status' "$HOME_FIXTURE/state/$run.workflow/run.json")" \
    "missing artifact did not fail the run"
  [ ! -e "$HOME_FIXTURE/missing-advanced" ] \
    || fail "later command ran without the required artifact"
  pass "automatic agent gates do not advance on model self-report alone"
}

test_reference_workflow_end_to_end() {
  local run=reference-run output worktree
  local fake_agent="$TMP_ROOT/reference-agent" fake_spawn="$TMP_ROOT/reference-spawn"
  local fake_state="$TMP_ROOT/reference-state" fake_review="$HOME_FIXTURE/bin/mx-deep-review.sh"
  cp "$ROOT/workflows/new-feature.workflow.md" \
    "$REPO_FIXTURE/workflows/new-feature.workflow.md"
  track_definition new-feature.workflow.md
  mkdir -p "$HOME_FIXTURE/bin"
  cat >"$fake_agent" <<'EOF'
#!/usr/bin/env bash
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output=$2; shift 2 ;;
    --session-out) session_out=$2; shift 2 ;;
    *) shift 2 ;;
  esac
done
mkdir -p "$(dirname "$WF_REFERENCE_SPEC")"
printf 'reference specification\n' >"$WF_REFERENCE_SPEC"
printf '{"status":"done","message":"reference spec written"}\n' >"$output"
printf 'reference-broker-session\n' >"$session_out"
EOF
  cat >"$fake_spawn" <<'EOF'
#!/usr/bin/env bash
set -eu
id=$1
repo=$2
worktree="$WF_FAKE_HOME/worktrees/$id"
mkdir -p "$(dirname "$worktree")"
git clone -q "$repo" "$worktree"
git -C "$worktree" checkout -qb "mx/$id"
mkdir -p "$WF_FAKE_HOME/state"
printf 'worktree=%s\nproject=%s\nharness=fake\nkind=delivery\n' \
  "$worktree" "$repo" >"$WF_FAKE_HOME/state/$id.meta"
EOF
  cat >"$fake_state" <<'EOF'
#!/usr/bin/env bash
printf 'state: %s · source: status-log · fixture\n' "${WF_FAKE_ACTOR_STATE:-working}"
EOF
cat >"$fake_review" <<'EOF'
#!/usr/bin/env bash
set -eu
printf 'unexpected implicit deep-review invocation\n' >&2
exit 97
EOF
  chmod +x "$fake_agent" "$fake_spawn" "$fake_state" "$fake_review"

  output=$(workflow_cli run new-feature --input 'Add the reference feature' --id "$run") \
    || fail "reference workflow launch failed"
  assert_contains "$output" "current_stage: ideate" "reference ideation did not park"
  mkdir -p "$HOME_FIXTURE/data/$run"
  printf 'approved approach\n' >"$HOME_FIXTURE/data/$run/approach.md"
  resolve_stage "$run" ideate

  output=$(WF_REFERENCE_SPEC="$HOME_FIXTURE/data/$run/spec.md" \
    MX_WORKFLOW_AGENT_COMMAND="$fake_agent" workflow_cli resume "$run") \
    || fail "reference specification stage failed"
  assert_contains "$output" "current_stage: spec" "reference spec approval did not park"
  resolve_stage "$run" spec

  output=$(WF_FAKE_HOME="$HOME_FIXTURE" \
    MX_WORKFLOW_SPAWN_COMMAND="$fake_spawn" \
    MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" \
    workflow_cli resume "$run") || fail "reference actor launch failed"
  assert_contains "$output" "current_stage: implement" "reference actor did not wait"
  worktree=$(jq -r '.worktree' \
    "$HOME_FIXTURE/state/$run.workflow/stages/implement.json")
  printf 'implemented\n' >"$worktree/reference-change.txt"
  git -C "$worktree" add reference-change.txt
  git -C "$worktree" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm 'reference implementation'
  printf 'commit: %s\nchecks: fixture passed\nlimitations: none\ndelivery: local branch\n' \
    "$(git -C "$worktree" rev-parse HEAD)" >"$HOME_FIXTURE/data/$run/implementation.md"

  output=$(WF_FAKE_ACTOR_STATE=done \
    MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" workflow_cli resume "$run") \
    || fail "reference implementation completion failed"
  [ ! -e "$HOME_FIXTURE/state/$run.workflow/stages/review.json" ] \
    || fail "general-purpose workflow retained an implicit review stage"
  assert_contains "$output" "status: completed" "reference workflow did not complete"
  [ ! -e "$HOME_FIXTURE/state/$run.workflow/stages/deliver.json" ] \
    || fail "general-purpose workflow retained a separate delivery approval stage"
  pass "shipped new-feature workflow completes with direct agent delivery and no implicit review tooling"
}

test_abort_and_run_id_reuse_refusal() {
  local run=failure-run output
  workflow_cli abort "$run" >/dev/null || fail "abort failed"
  output=$(workflow_cli resume "$run" 2>&1) && fail "aborted run resumed"
  assert_contains "$output" "permanently aborted" "aborted resume refusal was unclear"
  output=$(workflow_cli run failure --input reuse --id "$run" 2>&1) \
    && fail "existing run id was reused"
  assert_contains "$output" "run id already exists" "run-id reuse refusal was unclear"
  pass "abort is permanent and run identities cannot be reused"
}

grant_workflow_binding() {
  local bindings=$1 request operation target
  operation=$(printf '%s' "$bindings" | jq -r '.operation')
  target=$(printf '%s' "$bindings" | jq -r '.target')
  request=$(MX_STATE_OVERRIDE="$HOME_FIXTURE/state" "$ROOT/bin/mx-maintainer-override.sh" request \
    --boundary "$(printf '%s' "$bindings" | jq -r '.boundary')" \
    --task "$(printf '%s' "$bindings" | jq -r '.task')" \
    --project "$(printf '%s' "$bindings" | jq -r '.project')" \
    --operation "$operation" --target "$target" \
    --expected-state "$(printf '%s' "$bindings" | jq -r '.expected_state_digest')" \
    --consequence "$(printf '%s' "$bindings" | jq -r '.consequence')") || return 1
  MX_STATE_OVERRIDE="$HOME_FIXTURE/state"
  export MX_STATE_OVERRIDE
  # shellcheck source=bin/mx-maintainer-override-lib.sh
  . "$ROOT/bin/mx-maintainer-override-lib.sh"
  mx_override_require_primary_lock() { return 0; }
  mx_override_grant "$request" "Grant $(printf '%s' "$bindings" | jq -r '.boundary') for $operation on $target only." \
    || return 1
  printf '%s\n' "$request"
}

test_exact_workflow_skip_and_reorder() {
  local definition="$REPO_FIXTURE/workflows/exceptions.workflow.md" run bindings request output
  cat >"$definition" <<'EOF'
---
workflow_version: 1
name: exceptions
description: Exercise exact workflow exception transitions.
stages:
  - id: hold
    title: Hold first
    type: interactive
    gate: approve
    output: data/{run}/hold.md
  - id: second
    title: Second
    type: command
    gate: auto
    run: printf second > "$MX_WORKFLOW_HOME/data/{run}-second"
  - id: third
    title: Third
    type: command
    gate: auto
    run: printf third > "$MX_WORKFLOW_HOME/data/{run}-third"
---

## hold

Wait for approval.

## second

Run second.

## third

Run third.
EOF
  track_definition exceptions.workflow.md

  run=workflow-skip-run
  workflow_cli run exceptions --input exact --id "$run" --depends failure-run >/dev/null || fail "workflow skip fixture did not launch"
  bindings=$(MX_HOME="$HOME_FIXTURE" MX_STATE_OVERRIDE="$HOME_FIXTURE/state" \
    "$ROOT/bin/mx-override-bindings.sh" workflow-skip "$run" second) || fail "workflow skip bindings failed"
  request=$(grant_workflow_binding "$bindings") || fail "workflow skip grant failed"
  output=$(workflow_cli skip "$run" second --override "$request") || fail "workflow exact skip failed"
  assert_contains "$output" 'second: skipped' "workflow skip is not labeled skipped"
  [ "$(jq -r '.exception' "$HOME_FIXTURE/state/$run.workflow/stages/second.json")" = maintainer-directed ] \
    || fail "workflow skip did not record maintainer direction"
  if workflow_cli skip "$run" third --override "$request" >/dev/null 2>&1; then
    fail "workflow skip grant authorized a second stage"
  fi

  run=workflow-reorder-run
  workflow_cli run exceptions --input exact --id "$run" --depends failure-run >/dev/null || fail "workflow reorder fixture did not launch"
  bindings=$(MX_HOME="$HOME_FIXTURE" MX_STATE_OVERRIDE="$HOME_FIXTURE/state" \
    "$ROOT/bin/mx-override-bindings.sh" workflow-reorder "$run" third second) || fail "workflow reorder bindings failed"
  request=$(grant_workflow_binding "$bindings") || fail "workflow reorder grant failed"
  workflow_cli reorder "$run" third --before second --override "$request" >/dev/null \
    || fail "workflow exact reorder failed"
  [ "$(jq -r 'join(",")' "$HOME_FIXTURE/state/$run.workflow/stage-order.json")" = 'hold,third,second' ] \
    || fail "workflow reorder changed the wrong stage order"
  [ "$(jq -r '.outcome' "$HOME_FIXTURE/state/maintainer-overrides/consumed/$request.json")" = succeeded ] \
    || fail "workflow reorder outcome was not recorded"
  pass "workflow skip and reorder consume distinct exact grants and preserve other stages"
}

test_headless_command_stage_refuses_remote_merge() {
  local definition="$REPO_FIXTURE/workflows/merge-boundary.workflow.md" run=merge-boundary-run fakebin
  definition="$REPO_FIXTURE/workflows/merge-boundary.workflow.md"
  fakebin="$TMP_ROOT/merge-boundary-bin"
  mkdir -p "$fakebin"
  cat >"$fakebin/gh" <<SH
#!/usr/bin/env bash
touch "$TMP_ROOT/headless-merge-ran"
exit 0
SH
  chmod +x "$fakebin/gh"
  cat >"$definition" <<'EOF'
---
workflow_version: 1
name: merge-boundary
description: Exercise the human-only merge boundary in headless command stages.
stages:
  - id: merge
    title: Merge remotely
    type: command
    gate: auto
    run: gh pr merge 7 --squash
---

## merge

This stage must be refused before command execution.
EOF
  track_definition merge-boundary.workflow.md
  PATH="$fakebin:$PATH" workflow_cli run merge-boundary --input boundary --id "$run" >/dev/null \
    || fail "merge-boundary workflow did not record its guarded failure"
  [ ! -e "$TMP_ROOT/headless-merge-ran" ] || fail "headless workflow executed gh pr merge"
  assert_file_contains "$HOME_FIXTURE/state/$run.workflow/commands/merge.stderr" \
    '[remote-pr-merge]' "headless merge refusal omitted its stable reason"
  [ "$(jq -r '.exit_code' "$HOME_FIXTURE/state/$run.workflow/stages/merge.json")" = 3 ] \
    || fail "headless merge refusal did not record its synthetic exit code"
  pass "headless workflow command stages refuse remote PR merges before execution"
}

test_project_bound_dependencies_and_revision_decisions() {
  local blocking="$REPO_FIXTURE/workflows/blocking.workflow.md"
  local simple="$REPO_FIXTURE/workflows/simple.workflow.md"
  local repo_a="$TMP_ROOT/repo-a" repo_b="$TMP_ROOT/repo-b" repo_c="$TMP_ROOT/repo-c"
  local output answer="$TMP_ROOT/stale.answer" binding_a binding_b binding_c
  for repo in "$repo_a" "$repo_b" "$repo_c"; do
    mkdir -p "$repo"
    mx_git_init_commit "$repo"
  done
  cat >"$blocking" <<'EOF'
---
workflow_version: 2
name: blocking
description: Wait for one explicit decision.
stages:
  - id: choose
    title: Choose the target
    type: interactive
    gate: approve
    output: data/{run}/choice.md
  - id: finish
    title: Finish after the decision
    type: command
    gate: auto
    run: printf finished > "$MX_WORKFLOW_HOME/data/{run}.done"
---

## choose

Choose the target for {input} and write {output}.

## finish

Finish after the exact decision target is resolved.
EOF
  cat >"$simple" <<'EOF'
---
workflow_version: 2
name: simple
description: Finish one independent project-bound run.
stages:
  - id: finish
    title: Finish
    type: command
    gate: auto
    run: printf finished > "$MX_WORKFLOW_HOME/data/{run}.done"
---

## finish

Finish this project-bound run.
EOF
  git -C "$REPO_FIXTURE" add workflows/blocking.workflow.md workflows/simple.workflow.md
  git -C "$REPO_FIXTURE" -c user.name='Multplx Tests' \
    -c user.email='tests@example.invalid' commit -qm 'add dependency workflows'

  binding_a=$(mx_cli project register "$repo_a" --alias workflow-a) \
    || fail "repo A registration failed"
  binding_b=$(mx_cli project register "$repo_b" --alias workflow-b) \
    || fail "repo B registration failed"
  binding_c=$(mx_cli project register "$repo_c" --alias workflow-c) \
    || fail "repo C registration failed"
  printf 'Original research for repo A.\n' >"$TMP_ROOT/repo-a-research.md"
  mx_cli request submit --batch one-chat-batch --request request-a --task repo-a-run \
    --client terminal-client --project "$(jq -r '.project_id' <<<"$binding_a")" \
    --checkout "$(jq -r '.checkout_id' <<<"$binding_a")" \
    --start "$(jq -r '.starting_revision' <<<"$binding_a")" --brief 3 \
    --scope choice --artifact "$TMP_ROOT/repo-a-research.md" >/dev/null \
    || fail "repo A routed request failed"
  mx_cli request submit --batch one-chat-batch --request request-b --task repo-b-run \
    --client terminal-client --project "$(jq -r '.project_id' <<<"$binding_b")" \
    --checkout "$(jq -r '.checkout_id' <<<"$binding_b")" \
    --start "$(jq -r '.starting_revision' <<<"$binding_b")" --brief 2 \
    --scope dependent --depends repo-a-run >/dev/null \
    || fail "repo B routed request failed"
  mx_cli request submit --batch one-chat-batch --request request-c --task repo-c-run \
    --client terminal-client --project "$(jq -r '.project_id' <<<"$binding_c")" \
    --checkout "$(jq -r '.checkout_id' <<<"$binding_c")" \
    --start "$(jq -r '.starting_revision' <<<"$binding_c")" --brief 1 \
    --scope independent >/dev/null || fail "repo C routed request failed"

  output=$(workflow_cli run blocking --request request-a --id wrong-task 2>&1) \
    && fail "routed workflow accepted a conflicting task identity"
  assert_contains "$output" 'conflicts with the routed request task identity' \
    "routed task conflict was unclear"
  workflow_cli run blocking --request request-a >/dev/null \
    || fail "blocking project workflow did not launch"
  output=$(workflow_cli run simple --request request-b) \
    || fail "dependent project workflow did not launch"
  assert_contains "$output" 'waiting on workflow dependencies: repo-a-run' \
    "dependent run did not remain waiting"
  [ ! -e "$HOME_FIXTURE/data/repo-b-run.done" ] || fail "dependent run executed early"
  output=$(workflow_cli run simple --request request-c) \
    || fail "independent project workflow did not complete"
  assert_contains "$output" 'status: completed' "independent project was stalled"
  assert_eq "3" "$(for run in repo-a-run repo-b-run repo-c-run; do jq -r '.project.project_id' "$HOME_FIXTURE/state/$run.workflow/run.json"; done | sort -u | wc -l | tr -d ' ')" \
    "workflow project bindings leaked across repositories"
  assert_eq "one-chat-batch" \
    "$(jq -r '.request_correlation.batch_id' "$HOME_FIXTURE/state/repo-a-run.workflow/run.json")" \
    "workflow lost the one-chat batch correlation"
  assert_eq "3" \
    "$(jq -r '.accepted_brief_revision' "$HOME_FIXTURE/state/repo-a-run.workflow/run.json")" \
    "workflow did not retain the routed accepted brief"
  assert_file_contains "$HOME_FIXTURE/state/repo-a-run.workflow/prompts/choose.md" \
    "$TMP_ROOT/repo-a-research.md" "workflow did not pass the original context pointer"

  mkdir -p "$HOME_FIXTURE/data/repo-a-run"
  printf 'A\n' >"$HOME_FIXTURE/data/repo-a-run/choice.md"
  printf 'A\n' >"$answer"
  output=$(MX_ROOT_OVERRIDE="$REPO_FIXTURE" MX_HOME="$HOME_FIXTURE" \
    MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
    "$ROOT/bin/mx-decision-hold.sh" resolve repo-a-run choose \
      --decision-file "$answer" --routed-to repo-a-run \
      --task repo-a-run --brief-revision 1 --workflow-revision wrong 2>&1) \
    && fail "stale workflow decision target was accepted"
  assert_contains "$output" 'stale decision target' "stale decision refusal was unclear"
  resolve_stage repo-a-run choose
  workflow_cli resume repo-a-run >/dev/null || fail "blocking workflow did not complete"
  output=$(workflow_cli resume repo-b-run) || fail "dependent workflow did not release"
  assert_contains "$output" 'status: completed' "completed dependency did not release its run"

  output=$(workflow_cli run simple --input cycle --id self-cycle --repo "$repo_a" \
    --depends self-cycle 2>&1) && fail "self dependency was accepted"
  assert_contains "$output" 'cannot include self' "dependency-cycle refusal was unclear"
  pass "project-bound workflows isolate repositories, decisions and dependency scheduling"
}

test_scoped_coordinator_stage_waits_for_declared_output() {
  local definition="$REPO_FIXTURE/workflows/coordinator.workflow.md" run=coordinator-run
  local fake_spawn="$TMP_ROOT/coordinator-spawn" fake_state="$TMP_ROOT/coordinator-state" output
  cat >"$definition" <<'EOF'
---
workflow_version: 2
name: coordinator
description: Preserve order around one bounded coordinator stage.
stages:
  - id: prepare
    title: Prepare
    type: command
    gate: auto
    output: data/{run}/prepare.md
    run: mkdir -p "$(dirname {output})"; printf prepared > {output}
  - id: coordinate
    title: Coordinate bounded implementation
    type: agent
    executor: sub-agent-session
    assignment: sub-orchestrator
    fresh_session: true
    brief_from: [prepare]
    gate: auto
    output: data/{run}/coordinated.md
  - id: finish
    title: Finish after coordination
    type: command
    gate: auto
    run: printf finished > "$MX_WORKFLOW_HOME/data/{run}.finished"
---

## prepare

Prepare the exact input.

## coordinate

Delegate two bounded child checks and synthesize their evidence in {output}.

## finish

Run only after the coordinator's declared output exists.
EOF
  track_definition coordinator.workflow.md
  cat >"$fake_spawn" <<'EOF'
#!/usr/bin/env bash
set -eu
printf '%s\n' "$*" >"$WF_COORDINATOR_ARGS"
id=$1
mkdir -p "$WF_FAKE_HOME/state"
printf 'project=%s\nharness=fake\nkind=daemon\n' "$2" >"$WF_FAKE_HOME/state/$id.meta"
EOF
  cat >"$fake_state" <<'EOF'
#!/usr/bin/env bash
printf 'state: %s · source: status-log · fixture\n' "${WF_FAKE_ACTOR_STATE:-working}"
EOF
  chmod +x "$fake_spawn" "$fake_state"
  output=$(WF_FAKE_HOME="$HOME_FIXTURE" WF_COORDINATOR_ARGS="$TMP_ROOT/coordinator.args" \
    MX_WORKFLOW_SPAWN_COMMAND="$fake_spawn" MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" \
    workflow_cli run coordinator --input 'coordinate it' --id "$run") \
    || fail "coordinator workflow did not launch"
  assert_contains "$output" 'current_stage: coordinate' "coordinator stage did not wait"
  assert_contains "$(cat "$TMP_ROOT/coordinator.args")" '--sub-orchestrator' \
    "coordinator stage did not use the scoped coordinator launch form"
  mkdir -p "$HOME_FIXTURE/data/$run/children"
  printf ready >"$HOME_FIXTURE/data/$run/children/one.md"
  printf ready >"$HOME_FIXTURE/data/$run/children/two.md"
  output=$(WF_FAKE_ACTOR_STATE=done MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" \
    workflow_cli resume "$run" 2>&1) \
    && fail "child readiness advanced a coordinator without its declared output"
  assert_contains "$output" 'reported done before its contract was met' \
    "coordinator false-done refusal was unclear"
  [ ! -e "$HOME_FIXTURE/data/$run.finished" ] || fail "later stage ran before coordination output"
  printf 'children: one, two\nchecks: passed\n' >"$HOME_FIXTURE/data/$run/coordinated.md"
  output=$(WF_FAKE_ACTOR_STATE=done MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" \
    workflow_cli resume "$run") || fail "coordinator workflow did not resume"
  assert_contains "$output" 'status: completed' "coordinator workflow did not reach stage three"
  pass "scoped coordinator stages retain one owner and wait for declared child synthesis"
}

test_order_contract_approval_and_restart
test_passed_command_requires_captured_zero_exit
test_concurrent_reconcile_is_refused
test_out_of_order_record_is_refused
test_snapshot_immutability_and_artifact_command_is_inert
test_command_failure_captures_output_and_parks
test_actor_fresh_session_and_local_commit_contract
test_auto_agent_does_not_advance_without_artifact
test_reference_workflow_end_to_end
test_abort_and_run_id_reuse_refusal
test_headless_command_stage_refuses_remote_merge
test_exact_workflow_skip_and_reorder
test_project_bound_dependencies_and_revision_decisions
test_scoped_coordinator_stage_waits_for_declared_output
