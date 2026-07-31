#!/usr/bin/env bash
# Behavior and static-contract tests for best-effort task journals.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

JOURNAL_LIB="$ROOT/bin/mx-journal-lib.sh"
REPORT="$ROOT/bin/mx-report"
mx_test_tmproot_into TMP_ROOT mx-journal

# shellcheck source=bin/mx-journal-lib.sh
. "$JOURNAL_LIB"

test_script_contract() {
  bash -n "$JOURNAL_LIB" || fail "journal library does not parse"
  [ -x "$JOURNAL_LIB" ] || fail "journal library is not executable"
  pass "journal library has an executable shell contract"
}

test_shape_and_append_order() {
  local state="$TMP_ROOT/order-state" task=journal-order event detail line_count
  mkdir -p "$state"
  detail='{"raw":"working: first","state":"working","validated":true}'
  MX_STATE_OVERRIDE="$state" MX_JOURNAL_SOURCE=mx-test \
    MX_JOURNAL_NOW=2026-07-30T10:00:00Z \
    mx_journal_emit "$task" status.reported "$detail" \
    || fail "first strict emit failed"
  MX_STATE_OVERRIDE="$state" MX_JOURNAL_SOURCE=mx-test \
    MX_JOURNAL_NOW=2026-07-30T10:00:01Z \
    mx_journal_emit "$task" status.reported \
      '{"raw":"paused: wait","state":"paused","validated":true}' \
    || fail "second strict emit failed"
  MX_STATE_OVERRIDE="$state" MX_JOURNAL_SOURCE=mx-test \
    MX_JOURNAL_NOW=2026-07-30T10:00:02Z \
    mx_journal_emit "$task" delivery.pr_opened \
      '{"pr_url":"https://github.com/example/repo/pull/1"}' \
    || fail "third strict emit failed"
  line_count=$(wc -l <"$state/$task.journal" | tr -d ' ')
  [ "$line_count" = 3 ] || fail "three emits did not append three lines"
  jq -e -s '
    length == 3 and
    all(.[]; (keys | sort) == ["detail","event","source","task","ts"]) and
    all(.[]; .task == "journal-order" and .source == "mx-test") and
    (map(.event) == ["status.reported","status.reported","delivery.pr_opened"]) and
    (map(.ts) == [
      "2026-07-30T10:00:00Z",
      "2026-07-30T10:00:01Z",
      "2026-07-30T10:00:02Z"
    ])
  ' "$state/$task.journal" >/dev/null || fail "journal envelope or append order is wrong"
  pass "strict emits append valid envelopes in call order"
}

test_malformed_calls_are_rejected() {
  local state="$TMP_ROOT/reject-state" rc
  mkdir -p "$state"
  MX_JOURNAL_WARNED=0
  MX_STATE_OVERRIDE="$state" mx_journal_emit safe-task unknown.event '{}' >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "unknown event was accepted"
  MX_STATE_OVERRIDE="$state" mx_journal_emit '../unsafe' task.spawned '{}' >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "unsafe task id was accepted"
  MX_STATE_OVERRIDE="$state" mx_journal_emit safe-task task.spawned '{broken' >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "malformed detail JSON was accepted"
  MX_STATE_OVERRIDE="$state" MX_JOURNAL_NOW='not-a-time"}' \
    mx_journal_emit safe-task task.spawned '{}' >/dev/null 2>&1
  rc=$?
  [ "$rc" -ne 0 ] || fail "malformed timestamp override was accepted"
  [ -z "$(find "$state" -type f -print -quit)" ] || fail "rejected calls wrote a journal"
  pass "strict entry point rejects unsafe ids, unknown events, malformed detail, and invalid time"
}

test_write_failure_is_best_effort_and_warns_once() {
  local home="$TMP_ROOT/failure-home" state task=journal-failure output
  state="$home/state"
  mkdir -p "$state/$task.journal"
  MX_JOURNAL_WARNED=0
  output=$(
    {
      MX_STATE_OVERRIDE="$state"
      export MX_STATE_OVERRIDE
      mx_journal_try "$task" task.spawned \
        '{"kind":"delivery","backend":"tmux","worktree":"/tmp/w","branch":"mx/x"}'
      mx_journal_try "$task" delivery.pr_opened '{"pr_url":"https://example.invalid"}'
    } 2>&1
  ) || fail "best-effort wrapper returned nonzero"
  [ "$(printf '%s\n' "$output" | grep -c '^mx-journal: ')" = 1 ] \
    || fail "write failures did not warn exactly once per process"

  output=$(MX_HOME="$home" MX_TASK_ID="$task" "$REPORT" \
    --id "$task" --state done --message "status survives journal failure" 2>&1) \
    || fail "mx-report failed because its journal was unwritable"
  [ "$(cat "$state/$task.status")" = "done: status survives journal failure" ] \
    || fail "mx-report did not durably append status beside a journal failure"
  printf '%s\n' "$output" | grep -F "mx-journal:" >/dev/null \
    || fail "mx-report did not expose the best-effort journal warning"
  pass "journal failures never fail the wrapped status operation and warn once"
}

test_vocabulary_and_writer_registration() {
  local lib_events doc_events event file
  lib_events=$(printf '%s\n' "$MX_JOURNAL_EVENTS" | LC_ALL=C sort)
  doc_events=$(awk '
      /^## Closed vocabulary/ { in_vocab=1; next }
      /^## / { in_vocab=0 }
      in_vocab { print }
    ' "$ROOT/docs/journal-events.md" \
    | sed -n 's/^| `\([^`]*\)` |.*/\1/p' | LC_ALL=C sort)
  [ "$lib_events" = "$doc_events" ] \
    || fail "library allowlist and documentation vocabulary differ"
  for event in $MX_JOURNAL_EVENTS; do
    grep -F "$event" "$ROOT/docs/journal-events.md" >/dev/null \
      || fail "documentation omits $event"
  done
  for file in \
    bin/mx-spawn.sh bin/mx-report bin/mx-actor-state.sh \
    bin/mx-push-transition-lib.sh bin/mx-deep-review.sh \
    bin/mx-workflow.sh bin/mx-workflow-lib.sh \
    bin/mx-decision-hold.sh bin/mx-deliver.sh; do
    grep -E 'mx_journal_try|wf_journal_stage_' "$ROOT/$file" >/dev/null \
      || fail "$file has no journal emission seam"
  done
  pass "event vocabulary is synchronized and every planned writer has an emit seam"
}

test_no_control_flow_reads() {
  local files unexpected
  files=$(rg -l '\.journal' "$ROOT/bin" | LC_ALL=C sort)
  unexpected=$(printf '%s\n' "$files" | grep -vE \
    '/bin/(mx-journal-lib|mx-timeline|mx-teardown)\.sh$' || true)
  [ -z "$unexpected" ] \
    || fail "production scripts outside the emitter, reader, or teardown mention journals:$unexpected"
  grep -F 'rm -f' "$ROOT/bin/mx-teardown.sh" >/dev/null \
    || fail "teardown journal mention is not on its removal path"
  ! rg -n '(cat|grep|sed|awk|jq|tail|head|<)[^#]*\.journal' \
    "$ROOT/bin/mx-teardown.sh" >/dev/null \
    || fail "teardown reads journal contents"
  pass "static tripwire allows only append, timeline read, and teardown removal"
}

test_script_contract
test_shape_and_append_order
test_malformed_calls_are_rejected
test_write_failure_is_best_effort_and_warns_once
test_vocabulary_and_writer_registration
test_no_control_flow_reads
