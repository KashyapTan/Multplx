#!/usr/bin/env bash
# Behavior tests for the resumable deep-review orchestrator with a fake harness.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_git_identity deep-review-tests deep-review-tests@example.invalid

GATE="$ROOT/bin/mx-deep-review.sh"
TMP_ROOT=$(mx_test_tmproot mx-deep-review)

make_fake_agent() {
  local path=$1
  cat > "$path" <<'SH'
#!/usr/bin/env bash
set -u
session= schema= prompt= output= session_out=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --session) session=$2; shift 2 ;;
    --schema) schema=$2; shift 2 ;;
    --prompt) prompt=$2; shift 2 ;;
    --output) output=$2; shift 2 ;;
    --session-out) session_out=$2; shift 2 ;;
    *) exit 2 ;;
  esac
done
step=$(sed -n 's/^DEEP-REVIEW STEP: \([^ ]*\).*/\1/p' "$prompt")
mode=$(sed -n 's/^DEEP-REVIEW STEP: [^ ]* (\([^)]*\)).*/\1/p' "$prompt")
printf '%s %s %s\n' "$step" "$mode" "$session" >> "$MX_FAKE_AGENT_LOG"
if [ -n "${MX_FAKE_KILL_ONCE_FILE:-}" ] && [ "$step:$mode" = review:assess ] \
  && [ ! -e "$MX_FAKE_KILL_ONCE_FILE" ]; then
  touch "$MX_FAKE_KILL_ONCE_FILE"
  kill -TERM "$PPID"
  exit 143
fi
count=$(wc -l < "$MX_FAKE_AGENT_LOG" | tr -d '[:space:]')
session_id="session-$count"
[ "${MX_FAKE_SAME_SESSION:-0}" != 1 ] || session_id=session-1
case "$step:$mode:${MX_FAKE_MODE:-clean}" in
  review:assess:invalid)
    printf '%s\n' '{}' > "$output"
    ;;
  review:assess:ask)
    printf '%s\n' '{"findings":[{"id":"api-choice","file":"src/api.sh","line":7,"severity":"error","action":"ask-user","review_scope":"source","message":"Choose the accepted API behavior."}],"risk_level":"high","risk_rationale":"Product behavior is unresolved.","risk_scope":"source"}' > "$output"
    ;;
  review:assess:*)
    printf '%s\n' '{"findings":[],"risk_level":"low","risk_rationale":"Focused local change with no source findings.","risk_scope":"source"}' > "$output"
    ;;
  test:assess:*)
    printf '%s\n' '{"findings":[],"summary":"Focused evidence passed.","tested":["focused smoke"],"testing_summary":"The accepted behavior was exercised.","artifacts":["CLI transcript recorded by fake harness."]}' > "$output"
    ;;
  *)
    printf '%s\n' '{"summary":"Applied focused gate correction"}' > "$output"
    ;;
esac
printf '%s\n' "$session_id" > "$session_out"
SH
  chmod +x "$path"
}

write_config() {
  local repo=$1 test_command=${2:-} allow=${3:-false}
  cat > "$repo/.deep-review.yaml" <<EOF
allow_repo_commands: $allow
disable_project_settings: true
commands:
  test: "$test_command"
  lint: ""
  format: ""
document:
  instructions: |
    Keep documentation current.
test:
  evidence:
    store_in_repo: false
EOF
}

make_case() {
  local name=$1 test_command=${2:-} case_dir repo state id branch head
  case_dir="$TMP_ROOT/$name"
  repo="$case_dir/repo"
  state="$case_dir/state"
  id="gate-$name"
  branch="mx/$id"
  mkdir -p "$state" "$case_dir/data"
  mx_git_init_commit "$repo"
  git -C "$repo" branch -M main
  write_config "$repo" "$test_command"
  git -C "$repo" add .deep-review.yaml
  git -C "$repo" commit -qm "trusted gate config"
  git -C "$repo" checkout -qb "$branch"
  printf '%s\n' "$name change" > "$repo/change.txt"
  git -C "$repo" add change.txt
  git -C "$repo" commit -qm "$name change"
  head=$(git -C "$repo" rev-parse HEAD)
  mx_write_meta "$state/$id.meta" \
    "window=mx-$id" "worktree=$repo" "project=$repo" \
    "kind=delivery" "mode=deep-review" "harness=codex"
  chmod 600 "$state/$id.meta"
  make_fake_agent "$case_dir/fake-agent"
  : > "$case_dir/agent.log"
  printf '%s\t%s\t%s\t%s\n' "$case_dir" "$repo" "$state" "$id"
}

run_gate() {
  local case_dir=$1 repo=$2 state=$3 id=$4
  shift 4
  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_TASK_ID="$id" MX_DEEP_REVIEW_AGENT="$case_dir/fake-agent" \
      MX_FAKE_AGENT_LOG="$case_dir/agent.log" "$@" "$GATE" "$id" \
      --intent "Implement the accepted $id behavior."
  )
}

test_full_order_and_delivery_boundary() {
  local case_dir repo state id history calls
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case order)
EOF
  run_gate "$case_dir" "$repo" "$state" "$id" env \
    >"$case_dir/out" 2>"$case_dir/err" || fail "clean deep-review run failed"
  history=$(jq -r '.history | join(" ")' "$state/$id.gate/run.json")
  [ "$history" = "intent rebase review test document lint" ] \
    || fail "gate step order changed: $history"
  calls=$(awk '{print $1}' "$case_dir/agent.log" | tr '\n' ' ')
  [ "$calls" = "review test document " ] \
    || fail "unexpected agent step calls: $calls"
  assert_grep 'no test command configured, asking agent to run tests…' "$case_dir/out" \
    "empty test command did not dispatch the agent-evidence fallback"
  assert_not_contains "$history" "push" "local gate contains a push step"
  assert_not_contains "$history" "pr" "local gate contains a PR step"
  assert_not_contains "$history" "ci" "local gate contains a CI step"
  [ "$(jq -r '.status' "$state/$id.gate/run.json")" = passed ] \
    || fail "gate did not reach passed"
  assert_present "$state/$id.ready-to-push" "gate did not write delivery handoff"
  assert_grep 'approval=pending' "$state/$id.ready-to-push" \
    "gate bypassed delivery approval"
  assert_grep "approved_sha=$(git -C "$repo" rev-parse HEAD)" \
    "$state/$id.ready-to-push" "handoff did not pin exact HEAD"
  pass "deep-review enforces local step order and ends at a pending exact-SHA handoff"
}

test_intent_and_unknown_step_fail_closed() {
  local case_dir repo state id rc
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case intent)
EOF
  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_TASK_ID="$id" MX_DEEP_REVIEW_AGENT="$case_dir/fake-agent" \
      MX_FAKE_AGENT_LOG="$case_dir/agent.log" "$GATE" "$id"
  ) >"$case_dir/out" 2>"$case_dir/err"
  rc=$?
  [ "$rc" -ne 0 ] || fail "gate without intent exited zero"
  assert_grep 'explicit intent required' "$case_dir/err" "missing-intent diagnostic changed"
  assert_absent "$state/$id.gate/run.json" "missing intent created a run record"
  [ ! -s "$case_dir/agent.log" ] || fail "missing intent invoked a summarizer or harness"

  mkdir -p "$state/$id.gate"
  printf '%s\n' '{"version":1,"status":"running","step":"push"}' > "$state/$id.gate/run.json"
  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_TASK_ID="$id" MX_DEEP_REVIEW_AGENT="$case_dir/fake-agent" \
      MX_FAKE_AGENT_LOG="$case_dir/agent.log" "$GATE" "$id" --intent accepted
  ) >"$case_dir/out2" 2>"$case_dir/err2"
  rc=$?
  [ "$rc" -ne 0 ] || fail "unknown hand-edited step exited zero"
  assert_grep 'invalid or unknown step' "$case_dir/err2" "unknown-step refusal changed"
  pass "deep-review requires explicit intent and rejects unknown persisted steps"
}

test_actor_binding_and_failure_state() {
  local case_dir repo state id rc
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case binding)
EOF
  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_DEEP_REVIEW_AGENT="$case_dir/fake-agent" \
      MX_FAKE_AGENT_LOG="$case_dir/agent.log" \
      "$GATE" "$id" --intent accepted
  ) >"$case_dir/unbound-out" 2>"$case_dir/unbound-err"
  rc=$?
  [ "$rc" -ne 0 ] || fail "gate accepted a caller without the initiating task binding"
  assert_absent "$state/$id.gate" "unbound caller created gate state"

  run_gate "$case_dir" "$repo" "$state" "$id" env MX_FAKE_MODE=invalid \
    >"$case_dir/invalid-out" 2>"$case_dir/invalid-err"
  rc=$?
  [ "$rc" -ne 0 ] || fail "invalid structured output exited zero"
  [ "$(jq -r '.status' "$state/$id.gate/run.json")" = failed ] \
    || fail "terminal gate error did not persist failed state"
  [ "$(grep -c '^review assess ' "$case_dir/agent.log")" -eq 2 ] \
    || fail "malformed structured output did not receive the bounded retry count"
  pass "deep-review requires the initiating actor binding and persists terminal failures"
}

test_deterministic_test_failure_drives_fix() {
  local case_dir repo state id marker command
  marker="$TMP_ROOT/command-failure-once"
  command="if [ ! -e '$marker' ]; then printf 'deterministic failure evidence\\n'; touch '$marker'; exit 7; fi; printf 'deterministic pass\\n'"
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case command "$command")
EOF
  run_gate "$case_dir" "$repo" "$state" "$id" env \
    >"$case_dir/out" 2>"$case_dir/err" \
    || fail "gate did not recover from a deterministic test failure"
  assert_grep 'deterministic failure evidence' \
    "$state/$id.gate/cmd-output/test-round-01.log" \
    "configured test stdout was not captured"
  [ "$(jq -r '.findings[0].severity' \
    "$state/$id.gate/findings/round-01-test-command.json")" = error ] \
    || fail "nonzero test command did not create a blocking finding"
  assert_grep 'test fix ' "$case_dir/agent.log" \
    "nonzero command did not dispatch the test fixer"
  [ "$(jq -r '.exit_code' "$state/$id.gate/cmd-output/test.json")" -eq 0 ] \
    || fail "model output overrode the final real test exit code"
  pass "deep-review uses captured subprocess evidence and real exits for configured tests"
}

test_restart_and_head_change() {
  local case_dir repo state id rc history
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case restart)
EOF
  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_TASK_ID="$id" MX_DEEP_REVIEW_AGENT="$case_dir/fake-agent" \
      MX_FAKE_AGENT_LOG="$case_dir/agent.log" \
      MX_FAKE_KILL_ONCE_FILE="$case_dir/killed" \
      "$GATE" "$id" --intent accepted
  ) >"$case_dir/out1" 2>"$case_dir/err1"
  rc=$?
  [ "$rc" -ne 0 ] || fail "forced mid-review termination exited zero"
  [ "$(jq -r '.step' "$state/$id.gate/run.json")" = review ] \
    || fail "mid-review termination did not persist current step"
  git -C "$repo" commit -q --allow-empty -m "out of band head movement"
  run_gate "$case_dir" "$repo" "$state" "$id" env \
    >"$case_dir/out2" 2>"$case_dir/err2" || fail "restart after HEAD change failed"
  assert_grep 'HEAD changed; restarting current step' "$case_dir/out2" \
    "resume trusted stale findings after HEAD movement"
  history=$(jq -r '.history | join(" ")' "$state/$id.gate/run.json")
  [ "$history" = "intent rebase review test document lint" ] \
    || fail "resume repeated completed steps: $history"
  pass "deep-review reconstructs after termination and invalidates stale-head findings"
}

test_ask_user_response_and_session_isolation() {
  local case_dir repo state id rc key assess fix
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case decision)
EOF
  run_gate "$case_dir" "$repo" "$state" "$id" env MX_FAKE_MODE=ask \
    >"$case_dir/out1" 2>"$case_dir/err1"
  rc=$?
  [ "$rc" -eq 10 ] || fail "ask-user run did not park with exit 10: $rc"
  [ "$(jq -r '.status' "$state/$id.gate/run.json")" = parked ] \
    || fail "ask-user finding did not park run"
  key=$(jq -r '.pending_decision_key' "$state/$id.gate/run.json")
  assert_grep "needs-decision [key=$key]:" "$state/$id.status" \
    "ask-user finding did not use validated reporter"

  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_TASK_ID="$id" "$GATE" respond "$id" --decision "$key" \
      --answer "Preserve the accepted API."
  ) >"$case_dir/respond-out" 2>"$case_dir/respond-err" \
    || fail "actor-owned respond failed"
  run_gate "$case_dir" "$repo" "$state" "$id" env MX_FAKE_MODE=clean \
    >"$case_dir/out2" 2>"$case_dir/err2" || fail "run did not resume after decision"
  assess=$(jq -r '."review-assess-r1"' "$state/$id.gate/sessions.json")
  fix=$(jq -r '."review-fix-r1"' "$state/$id.gate/sessions.json")
  [ "$assess" != "$fix" ] || fail "reviewer and fixer reused a session"

  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case same-session)
EOF
  run_gate "$case_dir" "$repo" "$state" "$id" env MX_FAKE_MODE=ask \
    >"$case_dir/same1" 2>"$case_dir/same1err"
  key=$(jq -r '.pending_decision_key' "$state/$id.gate/run.json")
  (
    cd "$repo" || exit 1
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$(dirname "$state")" MX_STATE_OVERRIDE="$state" \
      MX_TASK_ID="$id" "$GATE" respond "$id" --decision "$key" --answer accepted
  ) >/dev/null 2>&1 || fail "same-session setup respond failed"
  run_gate "$case_dir" "$repo" "$state" "$id" env MX_FAKE_MODE=clean MX_FAKE_SAME_SESSION=1 \
    >"$case_dir/same2" 2>"$case_dir/same2err"
  rc=$?
  [ "$rc" -ne 0 ] || fail "forced reviewer/fixer session reuse exited zero"
  assert_grep 'refusing reviewer/fixer session reuse' "$case_dir/same2err" \
    "same-session refusal was unclear"
  pass "deep-review parks ask-user findings and enforces actor-owned isolated-session response"
}

test_default_branch_command_cannot_be_replaced() {
  local case_dir repo state id trusted_log canary rc
  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case trust)
EOF
  trusted_log="$case_dir/trusted.log"
  canary="$case_dir/canary.log"
  git -C "$repo" checkout -q main
  write_config "$repo" "printf trusted >> '$trusted_log'" false
  git -C "$repo" add .deep-review.yaml
  git -C "$repo" commit -qm "trusted command"
  git -C "$repo" checkout -q "mx/$id"
  git -C "$repo" rebase -q main
  write_config "$repo" "printf canary >> '$canary'" true
  git -C "$repo" add .deep-review.yaml
  git -C "$repo" commit -qm "untrusted command"
  run_gate "$case_dir" "$repo" "$state" "$id" env \
    >"$case_dir/out" 2>"$case_dir/err" || fail "trusted-command run failed"
  assert_present "$trusted_log" "trusted default-branch test command did not execute"
  assert_absent "$canary" "branch test command executed without trusted permission"

  IFS=$'\t' read -r case_dir repo state id <<EOF
$(make_case allow)
EOF
  trusted_log="$case_dir/trusted.log"
  canary="$case_dir/canary.log"
  git -C "$repo" checkout -q main
  write_config "$repo" "printf trusted >> '$trusted_log'" true
  git -C "$repo" add .deep-review.yaml
  git -C "$repo" commit -qm "trusted branch permission"
  git -C "$repo" checkout -q "mx/$id"
  git -C "$repo" rebase -q main
  write_config "$repo" "printf canary >> '$canary'" false
  git -C "$repo" add .deep-review.yaml
  git -C "$repo" commit -qm "permitted branch command"
  run_gate "$case_dir" "$repo" "$state" "$id" env \
    >"$case_dir/out" 2>"$case_dir/err"
  rc=$?
  [ "$rc" -eq 0 ] || fail "trusted allow_repo_commands positive case failed"
  assert_present "$canary" "trusted permission did not allow branch command"
  pass "deep-review command execution is controlled only by default-branch permission"
}

test_full_order_and_delivery_boundary
test_intent_and_unknown_step_fail_closed
test_actor_binding_and_failure_state
test_deterministic_test_failure_drives_fix
test_restart_and_head_change
test_ask_user_response_and_session_isolation
test_default_branch_command_cannot_be_replaced
