#!/usr/bin/env bash
# Exact competing-session termination: TERM the bound verified owner, prove
# exit, then acquire the ordinary lock. The fixture replaces only process
# identity discovery so the test does not need to run beneath a real harness.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mx_test_tmproot mx-lock-override)
BIN="$TMP_ROOT/bin"
STATE="$TMP_ROOT/state"
mkdir -p "$BIN" "$STATE"
cp "$ROOT/bin/mx-lock.sh" "$ROOT/bin/mx-maintainer-override-lib.sh" \
  "$ROOT/bin/mx-override-bindings.sh" "$ROOT/bin/mx-wake-lib.sh" "$BIN/"
cat > "$BIN/mx-session-lock-lib.sh" <<'SH'
mx_harness_ancestry_pid() { printf '%s\n' "${MX_TEST_ME_PID:?}"; }
mx_harness_pid_alive() {
  kill -0 "$1" 2>/dev/null || return 1
  [ "$1" != "${MX_TEST_ME_PID:?}" ]
}
SH
chmod +x "$BIN/"*.sh

grant_binding() (
  local bindings=$1 request boundary task project operation target digest consequence
  export MX_STATE_OVERRIDE=$STATE
  # shellcheck source=bin/mx-maintainer-override-lib.sh
  . "$BIN/mx-maintainer-override-lib.sh"
  mx_override_require_primary_lock() { return 0; }
  boundary=$(printf '%s' "$bindings" | jq -r '.boundary')
  task=$(printf '%s' "$bindings" | jq -r '.task')
  project=$(printf '%s' "$bindings" | jq -r '.project')
  operation=$(printf '%s' "$bindings" | jq -r '.operation')
  target=$(printf '%s' "$bindings" | jq -r '.target')
  digest=$(printf '%s' "$bindings" | jq -r '.expected_state_digest')
  consequence=$(printf '%s' "$bindings" | jq -r '.consequence')
  request=$(mx_override_request "$boundary" "$task" "$project" "$operation" "$target" "$digest" "$consequence") || exit 1
  mx_override_grant "$request" "Grant $boundary for $operation on $target only." >/dev/null || exit 1
  printf '%s\n' "$request"
)

test_terminate_owner_and_reacquire() {
  local owner bindings request out status me
  sleep 60 & owner=$!
  printf '%s\n' "$owner" > "$STATE/.lock"
  me=$$
  if out=$(MX_STATE_OVERRIDE="$STATE" MX_TEST_ME_PID="$me" "$BIN/mx-lock.sh" 2>&1); then status=0; else status=$?; fi
  expect_code 1 "$status" "ordinary competing lock acquisition"
  assert_contains "$out" 'request an exact session.terminate-owner grant' "ordinary refusal omitted exact alternate"
  kill -0 "$owner" 2>/dev/null || fail "ordinary refusal terminated the owner"

  bindings=$(MX_STATE_OVERRIDE="$STATE" MX_TEST_ME_PID="$me" "$BIN/mx-override-bindings.sh" terminate-owner "$owner") \
    || fail "terminate-owner binding failed"
  request=$(grant_binding "$bindings") || fail "terminate-owner grant failed"
  out=$(MX_STATE_OVERRIDE="$STATE" MX_TEST_ME_PID="$me" "$BIN/mx-lock.sh" --terminate-owner "$request") \
    || fail "exact terminate-owner alternate failed"
  wait "$owner" 2>/dev/null || true
  assert_contains "$out" "lock acquired: harness pid $me" "ordinary lock was not reacquired"
  if kill -0 "$owner" 2>/dev/null; then fail "bound competing owner survived TERM"; fi
  [ "$(cat "$STATE/.lock")" = "$me" ] || fail "new ordinary owner was not recorded"
  [ "$(jq -r '.outcome' "$STATE/maintainer-overrides/consumed/$request.json")" = succeeded ] \
    || fail "terminate-owner outcome was not recorded truthfully"
  if MX_STATE_OVERRIDE="$STATE" MX_TEST_ME_PID="$me" "$BIN/mx-lock.sh" --terminate-owner "$request" >/dev/null 2>&1; then
    fail "terminate-owner grant replay succeeded"
  fi
  pass "exact terminate-owner grant sends TERM, proves exit, and reacquires normally"
}

test_terminate_owner_and_reacquire
