#!/usr/bin/env bash
# End-to-end behavior coverage for workflow ordering, gates, snapshots, resume,
# command trust, actor reconciliation, abort, and run-id ownership.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mx_test_tmproot_into TMP_ROOT workflow
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
  mkdir -p "$HOME_FIXTURE/data/$run"
  printf approved >"$HOME_FIXTURE/data/$run/approved.md"
  resolve_stage "$run" approve
  output=$(workflow_cli resume "$run") || fail "resume after approval failed"
  assert_contains "$output" "status: completed" "workflow did not complete after resume"
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
id=$1
shift
[ "${1:-}" = --intent-file ]
[ -s "${2:-}" ]
[ "$(git symbolic-ref --quiet --short HEAD)" = "mx/$id" ]
printf 'ready\n' >"$MX_WORKFLOW_HOME/state/$id.ready-to-push"
printf 'reference deep-review passed\n'
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

  output=$(WF_FAKE_ACTOR_STATE=done \
    MX_WORKFLOW_ACTOR_STATE_COMMAND="$fake_state" workflow_cli resume "$run") \
    || fail "reference review stage failed"
  assert_contains "$output" "current_stage: deliver" \
    "reference delivery did not park: $(cat "$HOME_FIXTURE/state/$run.workflow/commands/review.stderr" 2>/dev/null)"
  assert_file_contains \
    "$HOME_FIXTURE/state/$run.workflow/commands/review.stdout" \
    "reference deep-review passed" "reference review command did not run"
  printf 'delivered\n' >"$HOME_FIXTURE/state/$run.delivered"
  resolve_stage "$run" deliver
  output=$(workflow_cli resume "$run") || fail "reference delivery resume failed"
  assert_contains "$output" "status: completed" "reference workflow did not complete"
  assert_eq "passed" \
    "$(jq -r '.status' "$HOME_FIXTURE/state/$run.workflow/stages/review.json")" \
    "reference deep-review stage did not pass"
  pass "shipped new-feature workflow completes end to end through local adapters"
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
