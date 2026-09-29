#!/usr/bin/env bash
# Behavior tests for the validated, task-bound status writer.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

REPORT="$ROOT/bin/mx-report"
TMP_ROOT=$(mx_test_tmproot mx-report)
EXPECTED_STATES='working
paused
blocked
needs-decision
done
failed
resolved'

run_bound() {
  local home=$1 id=$2
  shift 2
  MX_HOME="$home" MX_TASK_ID="$id" "$REPORT" "$@"
}

test_script_contract() {
  bash -n "$REPORT" || fail "mx-report does not parse"
  [ -x "$REPORT" ] || fail "mx-report is not executable"
  local states
  states=$("$REPORT" --list-states) || fail "--list-states failed"
  [ "$states" = "$EXPECTED_STATES" ] \
    || fail "--list-states does not expose the exact seven-state vocabulary"$'\n'"$states"
  pass "mx-report: executable shell contract and single-owner state vocabulary"
}

test_valid_states_and_keyed_grammar() {
  local home id state status_file expected count
  home="$TMP_ROOT/valid-home"
  id=report-valid-a1
  mkdir -p "$home/state"
  status_file="$home/state/$id.status"

  count=0
  while IFS= read -r state; do
    run_bound "$home" "$id" --id "$id" --state "$state" --message "message $state" \
      || fail "valid state '$state' was rejected"
    count=$((count + 1))
    [ "$(wc -l < "$status_file" | tr -d ' ')" = "$count" ] \
      || fail "state '$state' did not append exactly one line"
    expected="$state: message $state"
    [ "$(tail -n 1 "$status_file")" = "$expected" ] \
      || fail "state '$state' wrote the wrong line"
  done <<EOF
$EXPECTED_STATES
EOF

  run_bound "$home" "$id" --id "$id" --state needs-decision \
    --key api-shape --message "choose one" \
    || fail "keyed needs-decision was rejected"
  [ "$(tail -n 1 "$status_file")" = "needs-decision [key=api-shape]: choose one" ] \
    || fail "keyed status grammar changed"

  local folded
  folded=$(MX_CLASSIFY_PAUSED_VERB=paused bash -c \
    '. "$1"; status_open_decisions "$2"' _ "$ROOT/bin/mx-classify-lib.sh" "$status_file")
  printf '%s\n' "$folded" | grep -F $'api-shape\tneeds-decision\tchoose one' >/dev/null \
    || fail "status_open_decisions did not parse mx-report's keyed output"
  pass "mx-report: all valid states and keyed decision grammar append byte-compatibly"
}

test_invalid_inputs_never_write() {
  local home id status_file before output rc invalid
  home="$TMP_ROOT/invalid-home"
  id=report-invalid-b2
  mkdir -p "$home/state"
  status_file="$home/state/$id.status"
  printf 'working: existing\n' > "$status_file"
  before=$(shasum -a 256 "$status_file")

  for invalid in blocekd paused-ish maintainer-held; do
    output=$(run_bound "$home" "$id" --id "$id" --state "$invalid" --message nope 2>&1)
    rc=$?
    [ "$rc" -ne 0 ] || fail "invalid state '$invalid' exited zero"
    [ "$(shasum -a 256 "$status_file")" = "$before" ] \
      || fail "invalid state '$invalid' changed the status file"
    while IFS= read -r state; do
      printf '%s\n' "$output" | grep -F "$state" >/dev/null \
        || fail "invalid-state error omitted valid state '$state'"
    done <<EOF
$EXPECTED_STATES
EOF
  done

  local fresh_home="$TMP_ROOT/invalid-fresh" fresh_id=report-invalid-c3
  mkdir -p "$fresh_home/state"
  run_bound "$fresh_home" "$fresh_id" --id "$fresh_id" --state bogus --message nope \
    >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "invalid state in a fresh home exited zero"
  assert_absent "$fresh_home/state/$fresh_id.status" \
    "invalid state created a previously absent status file"
  pass "mx-report: invalid states fail loudly without creating or changing a status file"
}

test_message_passthrough_and_newline_rejection() {
  local home id status_file message before rc
  home="$TMP_ROOT/message-home"
  id=report-message-d4
  mkdir -p "$home/state"
  status_file="$home/state/$id.status"
  message='colon: brackets [key=fake] quotes "double" and '\''single'\'''
  run_bound "$home" "$id" --id "$id" --state working --message "$message" \
    || fail "single-line punctuation message was rejected"
  [ "$(cat "$status_file")" = "working: $message" ] \
    || fail "message payload was rewritten"
  before=$(shasum -a 256 "$status_file")
  run_bound "$home" "$id" --id "$id" --state working --message $'first\nsecond' \
    >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "newline message exited zero"
  [ "$(shasum -a 256 "$status_file")" = "$before" ] \
    || fail "newline message changed the status file"
  pass "mx-report: one-line messages pass through verbatim and multiline messages are rejected"
}

test_missing_arguments_and_bad_keys() {
  local home id rc args
  home="$TMP_ROOT/usage-home"
  id=report-usage-e5
  mkdir -p "$home/state"
  for args in \
    "--state working --message note" \
    "--id $id --message note" \
    "--id $id --state working"; do
    # shellcheck disable=SC2086
    MX_HOME="$home" MX_TASK_ID="$id" "$REPORT" $args >/dev/null 2>&1
    rc=$?
    [ "$rc" -ne 0 ] || fail "missing-argument case exited zero: $args"
  done
  run_bound "$home" "$id" --id "$id" --state working --message note --key 'bad key' \
    >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "invalid key exited zero"
  assert_absent "$home/state/$id.status" "usage or key error wrote a status file"
  pass "mx-report: missing arguments and invalid keys are side-effect-free usage errors"
}

test_task_binding_enforcement() {
  local home id other output rc
  home="$TMP_ROOT/binding-home"
  id=report-bound-f6
  other=report-other-g7
  mkdir -p "$home/state"

  output=$(run_bound "$home" "$id" --id "$other" --state done --message nope 2>&1)
  rc=$?
  [ "$rc" -ne 0 ] || fail "cross-task write exited zero"
  printf '%s\n' "$output" | grep -F "calling session is '$id', requested '$other'" >/dev/null \
    || fail "cross-task error did not name both bindings"
  assert_absent "$home/state/$id.status" "cross-task write changed the caller's status"
  assert_absent "$home/state/$other.status" "cross-task write created the target's status"

  run_bound "$home" "$id" --id "$id" --state done --message okay \
    || fail "same-task write was rejected"
  [ "$(cat "$home/state/$id.status")" = "done: okay" ] \
    || fail "same-task write did not land"
  pass "mx-report: MX_TASK_ID permits only the calling task's status file"
}

test_cwd_metadata_fallback_and_missing_binding() {
  local home id worktree output rc
  home="$TMP_ROOT/fallback-home"
  id=report-fallback-h8
  worktree="$TMP_ROOT/fallback worktree"
  mkdir -p "$home/state" "$worktree/subdir"
  printf 'worktree=%s\n' "$worktree" > "$home/state/$id.meta"

  (
    cd "$worktree/subdir" || exit 1
    MX_HOME="$home" MX_TASK_ID= "$REPORT" \
      --id "$id" --state paused --message "external wait"
  ) || fail "cwd-to-meta fallback did not bind the task"
  [ "$(cat "$home/state/$id.status")" = "paused: external wait" ] \
    || fail "cwd-to-meta fallback wrote the wrong event"

  local unbound="$TMP_ROOT/unbound-home"
  mkdir -p "$unbound/state" "$TMP_ROOT/unbound-cwd"
  output=$(
    cd "$TMP_ROOT/unbound-cwd" &&
      MX_HOME="$unbound" MX_TASK_ID= "$REPORT" \
        --id report-unbound-i9 --state done --message nope 2>&1
  )
  rc=$?
  [ "$rc" -ne 0 ] || fail "missing task binding exited zero"
  printf '%s\n' "$output" | grep -F "no task binding found" >/dev/null \
    || fail "missing-binding error was not distinct"
  assert_absent "$unbound/state/report-unbound-i9.status" \
    "missing binding created a status file"
  pass "mx-report: cwd metadata fallback is exact and an unbound caller fails closed"
}

test_current_report_invalidates_dependency_completion() (
  local home="$TMP_ROOT/dependency-home" project="$TMP_ROOT/dependency-project"
  local fakebin="$TMP_ROOT/dependency-fakebin" model attempt generation revision allocation commit
  local project_id checkout_id start output before status_count
  local mx="${MX_RUST_BIN:-$ROOT/target/release/mx}"
  mkdir -p "$home/state" "$home/config" "$home/data" "$home/projects" "$project" "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/bin/sh
case "${1:-}" in
  has-session|new-session) exit 0 ;;
  new-window) printf '%s\n' '@report-test' ;;
  display-message) printf '%s\n' '%1' ;;
  list-windows) printf '%s\n' 'test:window' ;;
  *) exit 0 ;;
esac
SH
  chmod +x "$fakebin/tmux"
  export PATH="$fakebin:$PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$ROOT"
  export MX_RUST_SOURCE_ROOT="$ROOT" MX_HEADROOM_SKIP_QUEUE=0
  export MX_HEADROOM_CPU_COUNT=64 MX_HEADROOM_LOAD1=0
  export MX_HEADROOM_MEM_AVAILABLE_BYTES=68719476736 MX_HEADROOM_IN_USE=0
  export MX_HEADROOM_API_CAPACITY=20 MX_MULTICALL_EXPLICIT=1
  printf 'fixture\n' > "$project/README.md"
  git -C "$project" init -q -b main || fail 'dependency Git init failed'
  git -C "$project" -c user.name=Fixture -c user.email=fixture@example.invalid \
    add README.md || fail 'dependency Git add failed'
  git -C "$project" -c user.name=Fixture -c user.email=fixture@example.invalid \
    commit -qm initial || fail 'dependency Git commit failed'
  "$mx" project register "$project" --alias report-dependency > "$home/project.json" \
    || fail 'dependency project registration failed'
  project_id=$(jq -r .project_id "$home/project.json")
  checkout_id=$(jq -r .checkout_id "$home/project.json")
  start=$(jq -r .starting_revision "$home/project.json")
  for task in prerequisite dependent unrelated; do
    "$mx" brief "$task" report-dependency --role implementer >/dev/null \
      || fail "brief creation failed for $task"
    sed "s/{TASK}/Implement $task./" "$home/data/$task/brief.md" \
      > "$home/data/$task/brief.next"
    mv "$home/data/$task/brief.next" "$home/data/$task/brief.md"
  done
  output=$("$mx" spawn prerequisite report-dependency --role implementer \
    --backend tmux --harness codex --request-id prerequisite-spawn) \
    || fail 'prerequisite spawn failed'
  assert_contains "$output" 'spawned prerequisite ' 'prerequisite endpoint absent'
  model=$("$mx" task-model inspect prerequisite) || fail 'prerequisite model absent'
  attempt=$(jq -r .attempt.id <<< "$model")
  generation=$(jq -r .attempt.generation <<< "$model")
  revision=$(jq -r .accepted_brief_revision <<< "$model")
  allocation=$(jq -r .allocation.path <<< "$model")
  commit=$(git -C "$allocation" rev-parse HEAD) || fail 'allocated HEAD absent'
  jq -n --arg attempt "$attempt" --arg commit "$commit" \
    --arg observed "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    --argjson generation "$generation" --argjson revision "$revision" \
    '{evidence_id:"report-dependency-evidence",attempt_id:$attempt,attempt_generation:$generation,brief_revision:$revision,commit:$commit,checks:[{name:"fixture",outcome:"passed",summary:"passed",artifact:null}],review:null,limitations:[],pr_url:null,outcome:"evidence-updated",observed_at:$observed,mark_current:true,expected_current_commit:null}' \
    > "$home/delivery.json"
  "$mx" task-model evidence prerequisite --request-file "$home/delivery.json" >/dev/null \
    || fail 'current typed delivery evidence rejected'
  bound_report() {
    MX_TASK_ID=prerequisite MX_ATTEMPT_ID="$attempt" \
      MX_ATTEMPT_GENERATION="$generation" MX_BRIEF_REVISION="$revision" \
      MX_RUST_BIN="$mx" MX_LAUNCH_BIN_PATH="$mx" \
      "$REPORT" --id prerequisite "$@"
  }
  bound_report --state done --message 'evidenced completion' --message-id dependency-done \
    || fail 'evidenced done report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = completed ] \
    || fail 'typed done did not complete prerequisite'
  if bound_report --state failed --message 'stale attempt' --message-id dependency-stale-attempt \
    --generation "$((generation + 1))" >/dev/null 2>&1; then
    fail 'stale attempt failure was accepted'
  fi
  if bound_report --state failed --message 'stale brief' --message-id dependency-stale-brief \
    --brief-revision "$((revision + 1))" >/dev/null 2>&1; then
    fail 'stale brief failure was accepted'
  fi
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = completed ] \
    || fail 'rejected stale report invalidated completion'
  bound_report --state failed --message 'current failure' --message-id dependency-failed \
    || fail 'current failure report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = waiting-external ] \
    || fail 'current failure left prerequisite completed'
  before=$(shasum -a 256 "$home/state/prerequisite.meta")
  status_count=$(wc -l < "$home/state/prerequisite.status" | tr -d ' ')
  bound_report --state done --message 'evidenced completion' --message-id dependency-done \
    || fail 'old done replay failed'
  [ "$(shasum -a 256 "$home/state/prerequisite.meta")" = "$before" ] \
    || fail 'old done replay re-completed prerequisite'
  [ "$(wc -l < "$home/state/prerequisite.status" | tr -d ' ')" = "$status_count" ] \
    || fail 'old done replay appended duplicate status'
  "$mx" request submit --batch dependency-test --request dependent-request --task dependent \
    --client test --project "$project_id" --checkout "$checkout_id" --start "$start" \
    --brief 1 --scope dependent --depends prerequisite >/dev/null \
    || fail 'dependent request submission failed'
  output=$("$mx" spawn dependent report-dependency --role implementer \
    --backend tmux --harness codex --request-id dependent-request) \
    || fail 'dependent deferral failed'
  assert_contains "$output" 'queued: dependent' 'failed prerequisite admitted dependent'
  [ ! -f "$home/state/dependent.meta" ] || fail 'deferred dependent gained task model'
  output=$("$mx" headroom --queue-drain) || fail 'blocked dependency drain failed'
  [ -z "$output" ] || fail 'blocked dependency drained queued dependent'
  [ -f "$home/state/.dispatch-queue/dependent-request.request" ] \
    || fail 'blocked dependency lost queued dependent'
  output=$("$mx" spawn unrelated report-dependency --role implementer \
    --backend tmux --harness codex --request-id unrelated-spawn) \
    || fail 'unrelated spawn failed'
  assert_contains "$output" 'spawned unrelated ' 'failed prerequisite stalled unrelated work'
  bound_report --state working --message 'failure recovery underway' \
    --message-id dependency-working-after-failed || fail 'working after failure failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = running ] \
    || fail 'working report did not clear failed report wait'
  output=$("$mx" headroom --queue-drain) || fail 'working prerequisite drain failed'
  [ -z "$output" ] || fail 'working prerequisite released dependent before done'
  bound_report --state done --message 'fresh evidenced completion' \
    --message-id dependency-done-fresh || fail 'fresh done report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = completed ] \
    || fail 'fresh valid done did not recover completion'
  output=$("$mx" headroom --queue-drain) || fail 'recovered dependency drain failed'
  assert_contains "$output" 'dependent' 'fresh done did not release queued dependent'
  [ ! -f "$home/state/.dispatch-queue/dependent-request.request" ] \
    || fail 'released dependent remained queued'
  bound_report --state blocked --message 'external blocker' --message-id dependency-blocked \
    || fail 'blocked report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = waiting-external ] \
    || fail 'blocked report retained completion'
  bound_report --state working --message 'blocker being addressed' \
    --message-id dependency-working-after-blocked || fail 'working after blocker failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = running ] \
    || fail 'working report did not clear blocked report wait'
  bound_report --state done --message 'blocker cleared' --message-id dependency-done-after-blocked \
    || fail 'completion after blocker failed'
  bound_report --state paused --message 'paused work' --message-id dependency-paused \
    || fail 'paused report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = waiting-external ] \
    || fail 'paused report retained completion'
  bound_report --state resolved --message 'pause ended' \
    --message-id dependency-resolved-after-pause || fail 'resolved after pause failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = running ] \
    || fail 'resolved report did not clear paused report wait'
  bound_report --state done --message 'pause ended' --message-id dependency-done-after-pause \
    || fail 'completion after pause failed'
  bound_report --state needs-decision --key route --message 'choose route' \
    --message-id dependency-decision || fail 'keyed decision report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r '.schedule.state + ":" + .schedule.waiting_condition')" = waiting-human:route ] \
    || fail 'keyed decision report retained completion'
  bound_report --state done --message 'decision handled' --message-id dependency-done-after-decision \
    || fail 'completion after decision failed'
  bound_report --state needs-decision --key route --message 'choose route' \
    --message-id dependency-decision-repeated || fail 'repeat keyed decision report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = waiting-human ] \
    || fail 'repeat current decision retained completion'
  bound_report --state working --message 'work resumed pending decision' \
    --message-id dependency-working-with-decision || fail 'working with decision failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = waiting-human ] \
    || fail 'working report bypassed human decision wait'
  bound_report --state done --message 'decision handled again' \
    --message-id dependency-done-after-repeated-decision \
    || fail 'completion after repeated decision failed'
  printf 'changed HEAD\n' > "$allocation/changed.txt"
  git -C "$allocation" add changed.txt || fail 'changed HEAD Git add failed'
  git -C "$allocation" -c user.name=Fixture -c user.email=fixture@example.invalid \
    commit -qm changed || fail 'changed HEAD Git commit failed'
  bound_report --state done --message 'completion without current evidence' \
    --message-id dependency-unproven-done >/dev/null \
    || fail 'unproven done report failed'
  [ "$("$mx" task-model inspect prerequisite | jq -r .schedule.state)" = running ] \
    || fail 'unproven done retained old completion'
  pass 'current failure invalidates typed completion, reserve and drain defer, replay stays inert, and fresh done releases work'
)

test_script_contract
test_valid_states_and_keyed_grammar
test_invalid_inputs_never_write
test_message_passthrough_and_newline_rejection
test_missing_arguments_and_bad_keys
test_task_binding_enforcement
test_cwd_metadata_fallback_and_missing_binding
test_current_report_invalidates_dependency_completion
