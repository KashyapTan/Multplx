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

test_script_contract
test_valid_states_and_keyed_grammar
test_invalid_inputs_never_write
test_message_passthrough_and_newline_rejection
test_missing_arguments_and_bad_keys
test_task_binding_enforcement
test_cwd_metadata_fallback_and_missing_binding
