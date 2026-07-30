#!/usr/bin/env bash
# Behavior tests for the deep-review lifecycle capability boundary.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

GATE_LIB="$ROOT/bin/mx-gate-refuse-lib.sh"
TMP_ROOT=$(mx_test_tmproot mx-gate-refuse)

test_marker_refusal() {
  local out rc
  out=$(env -u MX_GATE_REFUSE_BYPASS DEEP_REVIEW_GATE=1 bash -c \
    '. "$1"; mx_refuse_if_gate_agent' _ "$GATE_LIB" 2>&1)
  rc=$?
  [ "$rc" -eq 3 ] || fail "DEEP_REVIEW_GATE refusal returned $rc"
  assert_contains "$out" "deep-review agent must not drive Multplx lifecycle" \
    "marker refusal diagnostic changed"

  out=$(env -u MX_GATE_REFUSE_BYPASS DEEP_REVIEW_GATE= bash -c \
    '. "$1"; mx_refuse_if_gate_agent' _ "$GATE_LIB" 2>&1)
  rc=$?
  [ "$rc" -eq 3 ] || fail "empty DEEP_REVIEW_GATE marker did not refuse"
  pass "deep-review marker refuses lifecycle even when its value is empty"
}

test_normal_and_bypass() {
  env -u MX_GATE_REFUSE_BYPASS -u DEEP_REVIEW_GATE bash -eu -c \
    '. "$1"; mx_refuse_if_gate_agent' _ "$GATE_LIB" \
    || fail "normal session was refused"
  MX_GATE_REFUSE_BYPASS=1 DEEP_REVIEW_GATE=1 bash -eu -c \
    '. "$1"; mx_refuse_if_gate_agent' _ "$GATE_LIB" \
    || fail "test bypass did not remain available"
  pass "deep-review refusal is inert for normal sessions and explicit test fixtures"
}

test_lifecycle_entrypoints_refuse_before_mutation() {
  local script out rc
  for script in mx-spawn.sh mx-send.sh mx-teardown.sh; do
    out=$(env -u MX_GATE_REFUSE_BYPASS DEEP_REVIEW_GATE=1 \
      MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$TMP_ROOT/home" \
      "$ROOT/bin/$script" fake-task 2>&1)
    rc=$?
    [ "$rc" -eq 3 ] || fail "$script did not refuse gate context before parsing/mutation: $rc"
    assert_contains "$out" "deep-review agent must not drive Multplx lifecycle" \
      "$script did not use the shared refusal"
  done
  [ ! -e "$TMP_ROOT/home/state" ] || fail "lifecycle refusal created state"
  [ ! -e "$TMP_ROOT/home/data" ] || fail "lifecycle refusal created data"
  [ ! -e "$TMP_ROOT/home/projects" ] || fail "lifecycle refusal created projects"
  pass "spawn, send, and teardown refuse deep-review turns before mutation"
}

test_session_start_stays_silent() {
  local out rc
  out=$(env -u MX_GATE_REFUSE_BYPASS DEEP_REVIEW_GATE=1 \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$TMP_ROOT/home" \
    "$ROOT/bin/mx-sessionstart-nudge.sh" 2>&1)
  rc=$?
  [ "$rc" -eq 0 ] || fail "session-start gate suppression returned $rc"
  [ -z "$out" ] || fail "session-start gate suppression printed output: $out"
  pass "session-start nudge is silent inside deep-review"
}

test_marker_refusal
test_normal_and_bypass
test_lifecycle_entrypoints_refuse_before_mutation
test_session_start_stays_silent
