#!/usr/bin/env bash
# Rendering and filter tests for the sanctioned task-journal reader.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TIMELINE="$ROOT/bin/mx-timeline.sh"
FIXTURE="$ROOT/tests/fixtures/timeline.journal.jsonl"
GOLDEN="$ROOT/tests/fixtures/timeline.golden"
ID=timeline-fixture
mx_test_tmproot_into TMP_ROOT mx-timeline
HOME_FIXTURE="$TMP_ROOT/home"
STATE="$HOME_FIXTURE/state"
DATA="$HOME_FIXTURE/data"
mkdir -p "$STATE" "$DATA"
cp "$FIXTURE" "$STATE/$ID.journal"

timeline() {
  MX_HOME="$HOME_FIXTURE" MX_STATE_OVERRIDE="$STATE" MX_DATA_OVERRIDE="$DATA" \
    "$TIMELINE" "$ID" "$@"
}

test_script_and_golden_render() {
  local output="$TMP_ROOT/rendered.txt"
  bash -n "$TIMELINE" || fail "timeline reader does not parse"
  [ -x "$TIMELINE" ] || fail "timeline reader is not executable"
  TZ=UTC timeline >"$output" || fail "timeline render failed"
  cmp -s "$GOLDEN" "$output" || {
    diff -u "$GOLDEN" "$output" >&2 || true
    fail "timeline render differs from the golden file"
  }
  pass "timeline renders the committed fixture deterministically"
}

test_event_and_since_filters() {
  local output
  output=$(timeline --event 'gate.*') || fail "event filter failed"
  [ "$(printf '%s\n' "$output" | grep -c 'gate.step.finished')" = 1 ] \
    || fail "event glob did not select exactly the gate event"
  ! printf '%s\n' "$output" | grep -F 'status.reported' >/dev/null \
    || fail "event glob leaked a nonmatching event"

  output=$(timeline --since 2026-07-30T12:06:00Z) || fail "ISO since filter failed"
  [ "$(printf '%s\n' "$output" | grep -c '^12:')" = 3 ] \
    || fail "ISO since filter returned the wrong number of rows"
  output=$(MX_TIMELINE_NOW_MS=1785413700000 timeline --since 5m) \
    || fail "duration since filter failed"
  [ "$(printf '%s\n' "$output" | grep -c '^12:')" = 2 ] \
    || fail "duration since filter returned the wrong number of rows"
  pass "timeline filters by event glob, ISO time, and duration"
}

test_json_passthrough_and_malformed_tolerance() {
  local output warning malformed_state="$TMP_ROOT/malformed-state"
  output=$(timeline --json --event 'status.*') || fail "JSON passthrough failed"
  [ "$(printf '%s\n' "$output" | jq -s 'length')" = 2 ] \
    || fail "JSON passthrough returned the wrong records"
  [ "$(printf '%s\n' "$output" | head -1)" = "$(sed -n '2p' "$FIXTURE")" ] \
    || fail "JSON passthrough rewrote a matching record"

  mkdir -p "$malformed_state"
  cp "$FIXTURE" "$malformed_state/$ID.journal"
  printf '%s\n' '{broken' >>"$malformed_state/$ID.journal"
  output=$(MX_HOME="$HOME_FIXTURE" MX_STATE_OVERRIDE="$malformed_state" \
    MX_DATA_OVERRIDE="$DATA" "$TIMELINE" "$ID" 2>"$TMP_ROOT/malformed.err") \
    || fail "malformed journal line made the reader fail"
  [ "$(printf '%s\n' "$output" | grep -c '^12:')" = 5 ] \
    || fail "malformed tolerance dropped valid events"
  warning=$(cat "$TMP_ROOT/malformed.err")
  [ "$warning" = "mx-timeline: skipped 1 malformed journal line(s)" ] \
    || fail "malformed-line warning is not one counted line"
  pass "JSON passthrough is exact and malformed lines are skipped with one warning"
}

test_html_artifact_and_missing_vplan() {
  local artifact output rc
  artifact=$(timeline --html) || fail "HTML timeline render failed"
  [ -f "$artifact" ] || fail "HTML timeline artifact was not created"
  grep -F '<!DOCTYPE html>' "$artifact" >/dev/null \
    || fail "HTML artifact is incomplete"
  grep -F 'delivery.pr_opened' "$artifact" >/dev/null \
    || fail "HTML artifact omits timeline rows"
  ! grep -E '<(script|link)| (src|href)=' "$artifact" >/dev/null \
    || fail "HTML artifact depends on an external asset"

  output=$(MX_VPLAN_BIN="$TMP_ROOT/missing-vplan" timeline --html 2>&1)
  rc=$?
  [ "$rc" -ne 0 ] || fail "missing vplan module was accepted"
  printf '%s\n' "$output" | grep -F 'vplan module is unavailable' >/dev/null \
    || fail "missing vplan error is unclear"
  pass "HTML output is self-contained and vplan absence fails cleanly"
}

test_script_and_golden_render
test_event_and_since_filters
test_json_passthrough_and_malformed_tolerance
test_html_artifact_and_missing_vplan
