#!/usr/bin/env bash
# Behavior and tracked-registration tests for the native session-start nudge.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

unset NO_MISTAKES_GATE

TMP_ROOT=$(mx_test_tmproot mx-sessionstart-nudge)
NUDGE="$ROOT/bin/mx-sessionstart-nudge.sh"
# shellcheck source=/dev/null
. "$ROOT/bin/mx-operational-input.sh"
NUDGE_TEXT="Run \`bin/mx-session-start.sh\` now, exactly once, before executing any other instructions."
mx_operational_input_encode session-start "$NUDGE_TEXT" NUDGE_LINE \
  || fail "could not construct expected session-start nudge"
mx_git_identity fmtest fmtest@example.invalid

make_primary() {
  local dir=$1
  mkdir -p "$dir/bin" "$dir/state"
  git init -q "$dir"
  git -C "$dir" commit -q --allow-empty -m init
  : > "$dir/AGENTS.md"
}

run_nudge() {
  local root=$1
  MX_GATE_REFUSE_BYPASS=0 MX_ROOT_OVERRIDE="$root" MX_HOME="$root" "$NUDGE"
}

expect_silent_zero() {
  local label=$1
  shift
  local out status=0
  out=$("$@" 2>&1) || status=$?
  expect_code 0 "$status" "$label must exit 0"
  [ -z "$out" ] || fail "$label must be silent, got: $out"
}

test_genuine_primary_nudges() {
  local root="$TMP_ROOT/primary" out prefix_hex status=0
  make_primary "$root"
  out=$(run_nudge "$root") || status=$?
  expect_code 0 "$status" "genuine primary nudge"
  [ "$out" = "$NUDGE_LINE" ] || fail "genuine primary printed unexpected output: $out"
  prefix_hex=$(printf '%s' "$out" | head -c 3 | od -An -tx1 | tr -d ' \n')
  [ "$prefix_hex" = e281a3 ] || fail "genuine primary nudge lost its U+2063 operational marker: $prefix_hex"
  pass "mx-sessionstart-nudge: a genuine primary gets one explicitly marked instruction line"
}

test_gate_env_is_silent() {
  local root="$TMP_ROOT/gate-env"
  make_primary "$root"
  expect_silent_zero "gate env nudge" env NO_MISTAKES_GATE=1 MX_GATE_REFUSE_BYPASS=0 \
    MX_ROOT_OVERRIDE="$root" MX_HOME="$root" "$NUDGE"
  pass "mx-sessionstart-nudge: NO_MISTAKES_GATE is silent"
}

test_gate_common_dir_is_silent() {
  local source="$TMP_ROOT/gate-source" bare="$TMP_ROOT/.no-mistakes/repos/gate.git"
  local root="$TMP_ROOT/gate-worktree"
  mx_git_init_commit "$source"
  mkdir -p "$(dirname "$bare")"
  git clone --quiet --bare "$source" "$bare"
  git --git-dir="$bare" worktree add --quiet -b gate-test "$root" HEAD
  mkdir -p "$root/bin" "$root/state"
  : > "$root/AGENTS.md"
  printf 'gate-test\n' > "$root/.mx-daemon-home"
  expect_silent_zero "gate common-dir nudge" env MX_GATE_REFUSE_BYPASS=0 \
    MX_ROOT_OVERRIDE="$root" MX_HOME="$root" "$NUDGE"
  pass "mx-sessionstart-nudge: .no-mistakes gate common-dir is silent"
}

test_unmarked_linked_worktree_is_silent() {
  local base="$TMP_ROOT/worktree-base" root="$TMP_ROOT/worktree-child"
  mx_git_worktree "$base" "$root" mx/sessionstart-linked
  mkdir -p "$root/bin" "$root/state"
  : > "$root/AGENTS.md"
  expect_silent_zero "linked worktree nudge" run_nudge "$root"
  pass "mx-sessionstart-nudge: an unmarked linked task worktree is silent"
}

test_linked_daemon_primary_nudges() {
  local base="$TMP_ROOT/daemon-base" root="$TMP_ROOT/daemon-home" out status=0
  mx_git_worktree "$base" "$root" mx/sessionstart-daemon
  mkdir -p "$root/bin" "$root/state"
  : > "$root/AGENTS.md"
  printf 'sessionstart-sm\n' > "$root/.mx-daemon-home"
  out=$(run_nudge "$root") || status=$?
  expect_code 0 "$status" "linked daemon nudge"
  [ "$out" = "$NUDGE_LINE" ] || fail "linked daemon printed unexpected output: $out"
  pass "mx-sessionstart-nudge: a marked linked daemon home is a primary"
}

test_missing_state_is_silent() {
  local root="$TMP_ROOT/missing-state"
  make_primary "$root"
  rmdir "$root/state"
  expect_silent_zero "missing state nudge" run_nudge "$root"
  pass "mx-sessionstart-nudge: a checkout without state is silent"
}

test_owned_lock_is_silent() {
  local root="$TMP_ROOT/already-ran"
  make_primary "$root"
  printf '%s\n' "$$" > "$root/state/.lock"
  expect_silent_zero "owned lock nudge" run_nudge "$root"
  pass "mx-sessionstart-nudge: a lock holder in process ancestry is already run"
}

test_tracked_harness_registration() {
  local command pi_plugin
  jq -e '.hooks.SessionStart | length == 1' "$ROOT/.claude/settings.json" >/dev/null \
    || fail "Claude SessionStart hook is not registered exactly once"
  jq -e '.hooks.SessionStart[0].matcher == "startup|resume|clear"' "$ROOT/.claude/settings.json" >/dev/null \
    || fail "Claude SessionStart matcher must include startup/resume/clear and exclude compact"
  jq -e 'any(.hooks.SessionStart[]?.hooks[]?.command?; contains("mx-sessionstart-nudge.sh"))' \
    "$ROOT/.claude/settings.json" >/dev/null || fail "Claude SessionStart hook does not invoke the wrapper"

  command=$(jq -r '.hooks.SessionStart[0].hooks[0].command' "$ROOT/.codex/hooks.json")
  # shellcheck disable=SC2016
  assert_contains "$command" 'payload=$(cat' "Codex SessionStart hook does not read its payload"
  # shellcheck disable=SC2016
  assert_contains "$command" 'root=$(pwd -P)' "Codex SessionStart hook is not pwd-anchored"
  assert_contains "$command" 'mx-sessionstart-nudge.sh' "Codex SessionStart hook does not invoke the wrapper"

  pi_plugin=$(cat "$ROOT/.pi/extensions/mx-primary-turnend-guard.ts")
  assert_contains "$pi_plugin" '["startup", "new", "resume"]' "Pi SessionStart handler has the wrong reason allowlist"
  assert_contains "$pi_plugin" 'mx-sessionstart-nudge.sh' "Pi SessionStart handler does not invoke the wrapper"
  assert_contains "$pi_plugin" 'broker-sessionstart-nudge' "Pi SessionStart handler does not inject a custom context message"
  assert_contains "$pi_plugin" 'details: { kind: "session-start" }' "Pi SessionStart context does not retain its exact structured kind"
  assert_contains "$pi_plugin" 'pi.sendMessage' "Pi SessionStart handler does not use the context-safe message API"

  pass "all three verified harnesses register the shared session-start nudge"
}

test_genuine_primary_nudges
test_gate_env_is_silent
test_gate_common_dir_is_silent
test_unmarked_linked_worktree_is_silent
test_linked_daemon_primary_nudges
test_missing_state_is_silent
test_owned_lock_is_silent
test_tracked_harness_registration
