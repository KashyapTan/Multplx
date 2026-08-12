#!/usr/bin/env bash
# Behavior tests for the Claude Stop-owned watcher auto-arm
# (bin/mx-claude-stop-autoarm.sh, docs/watcher-continuity.md).
#
# The hook fires as a Claude asyncRewake Stop hook. These tests run it hermetically
# as a child of a fake harness (a bash symlink named "claude") whose pid is
# written into the fixture home's state/.lock for ordinary owned-lock cases.
# Stale-owner cases instead leave a dead recorded pid for the hook to reclaim
# through the real mx-lock.sh path. The arm wrapper is a per-test fixture, so no
# real watcher, model, or system state is touched.
# shellcheck disable=SC2016 # single quotes are deliberate: $MX_HOME expands inside the fake harness child, and grep needles are literal strings
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

if [ "${MX_SUPERVISION_IMPLEMENTATION:-rust}" = rust ]; then
  export MX_RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}
fi

TMP_ROOT=$(mx_test_tmproot mx-claude-stop-autoarm)
mx_git_identity fmtest fmtest@example.invalid

FAKEBIN=$(mx_fakebin "$TMP_ROOT/fakebin")
ln -s /bin/bash "$FAKEBIN/claude"
FAKE_CLAUDE="$FAKEBIN/claude"

# Copy the hook and its sourced dependencies into a fixture checkout.
install_autoarm_scripts() {
  local dir=$1
  mkdir -p "$dir/bin"
  cp "$ROOT/bin/mx-claude-stop-autoarm.sh" "$dir/bin/mx-claude-stop-autoarm.sh"
  cp "$ROOT/bin/mx-rust-runtime.sh" "$dir/bin/mx-rust-runtime.sh"
  cp "$ROOT/bin/mx-primary-scope-lib.sh" "$dir/bin/mx-primary-scope-lib.sh"
  cp "$ROOT/bin/mx-supervision-lib.sh" "$dir/bin/mx-supervision-lib.sh"
  cp "$ROOT/bin/mx-wake-lib.sh" "$dir/bin/mx-wake-lib.sh"
  cp "$ROOT/bin/mx-session-lock-lib.sh" "$dir/bin/mx-session-lock-lib.sh"
  cp "$ROOT/bin/mx-lock.sh" "$dir/bin/mx-lock.sh"
  chmod +x "$dir/bin/mx-claude-stop-autoarm.sh" "$dir/bin/mx-lock.sh"
}

make_primary_dir() {
  local dir=$1
  mkdir -p "$dir/state"
  git init -q "$dir"
  git -C "$dir" commit -q --allow-empty -m init
  : > "$dir/AGENTS.md"
  install_autoarm_scripts "$dir"
  printf '%s\n' "$dir"
}

make_daemon_dir() {
  local dir=$1
  make_primary_dir "$dir" >/dev/null
  printf 'sm-autoarm-1\n' > "$dir/.mx-daemon-home"
  printf '%s\n' "$dir"
}

# A genuine linked git worktree: the shape every actor/scout task worktree
# has (git-dir != git-common-dir), which must keep the hook inert.
make_actor_worktree_dir() {
  local base=$1 dir=$2
  mx_git_worktree "$base" "$dir" mx/autoarm-test-branch
  mkdir -p "$dir/state"
  : > "$dir/AGENTS.md"
  install_autoarm_scripts "$dir"
  printf '%s\n' "$dir"
}

# Run the hook as a child of the fake harness holding the fixture home's
# session lock. $1 = fixture dir. Any extra env assignments must be exported
# before invocation. Captures stdout+stderr; exit code on stdout of the caller.
run_autoarm() {
  local dir=$1 rc=0
  printf '%s\n' '{"session_id":"sess-autoarm","stop_hook_active":false}' \
    | MX_HOME="$dir" "$FAKE_CLAUDE" -c '
        printf "%s\n" "$$" > "$MX_HOME/state/.lock"
        "$MX_HOME/bin/mx-claude-stop-autoarm.sh"
      ' 2>&1 || rc=$?
  printf 'RC=%s\n' "$rc" >&2
  return "$rc"
}

# Arm fixture variants, installed per test as <dir>/bin/mx-watch-arm.sh.
write_arm_fixture() {
  local dir=$1 kind=$2
  case "$kind" in
    actionable)
      cat > "$dir/bin/mx-watch-arm.sh" <<'SH'
#!/usr/bin/env bash
echo "$$" >> "$MX_HOME/state/arm-ran"
printf 'watcher: started pid=%s (beacon fresh)\n' "$$"
printf 'stale: fixture-win actionable\n'
exit 0
SH
      ;;
    failed)
      cat > "$dir/bin/mx-watch-arm.sh" <<'SH'
#!/usr/bin/env bash
echo "$$" >> "$MX_HOME/state/arm-ran"
printf 'watcher: FAILED - no live watcher with a fresh beacon\n'
exit 1
SH
      ;;
    clean)
      cat > "$dir/bin/mx-watch-arm.sh" <<'SH'
#!/usr/bin/env bash
echo "$$" >> "$MX_HOME/state/arm-ran"
printf 'watcher: attached pid=%s (beacon 2s)\n' "$$"
exit 0
SH
      ;;
    slow-actionable)
      cat > "$dir/bin/mx-watch-arm.sh" <<'SH'
#!/usr/bin/env bash
echo "$$" >> "$MX_HOME/state/arm-ran"
sleep 2
printf 'watcher: started pid=%s (beacon fresh)\n' "$$"
printf 'signal: task.status done: slow fixture\n'
exit 0
SH
      ;;
    meta-vanishes)
      cat > "$dir/bin/mx-watch-arm.sh" <<'SH'
#!/usr/bin/env bash
echo "$$" >> "$MX_HOME/state/arm-ran"
rm -f "$MX_HOME/state/task.meta"
printf 'watcher: started pid=%s (beacon fresh)\n' "$$"
printf 'signal: task.status done: fixture\n'
exit 0
SH
      ;;
    afk-appears)
      cat > "$dir/bin/mx-watch-arm.sh" <<'SH'
#!/usr/bin/env bash
echo "$$" >> "$MX_HOME/state/arm-ran"
: > "$MX_HOME/state/.afk"
printf 'watcher: started pid=%s (beacon fresh)\n' "$$"
printf 'stale: fixture-win actionable\n'
exit 0
SH
      ;;
    *)
      echo "unknown arm fixture: $kind" >&2
      return 2
      ;;
  esac
  chmod +x "$dir/bin/mx-watch-arm.sh"
}

epoch_outcome() {
  sed -n 's/^.*outcome=\([a-z][a-z]*\) .*$/\1/p' "$1/state/.claude-autoarm-epoch" 2>/dev/null || true
}

# --- registration contract ----------------------------------------------------

test_settings_registers_autoarm_with_multi_hour_timeout() {
  local settings
  settings="$ROOT/.claude/settings.json"
  jq -e '
    [.hooks.Stop[].hooks[] | select(.command | contains("mx-claude-stop-autoarm.sh"))]
      | length == 1
  ' "$settings" >/dev/null || fail "settings must register exactly one Stop auto-arm hook"
  jq -e '
    [.hooks.Stop[].hooks[] | select(.command | contains("mx-claude-stop-autoarm.sh"))][0]
      | .asyncRewake == true and .type == "command" and (.timeout | type == "number" and . >= 28800)
  ' "$settings" >/dev/null || fail "auto-arm must be asyncRewake with an explicit timeout of at least 28800s (the 600s default is forbidden)"
  jq -e '
    [.hooks.Stop[].hooks[] | select(.command | contains("mx-claude-stop-autoarm.sh"))][0].command
      | contains("&") | not
  ' "$settings" >/dev/null || fail "auto-arm registration must not use shell fire-and-forget"
  grep -q '"$SCRIPT_DIR/mx-watch-arm.sh" >"$OUT" 2>&1' "$ROOT/bin/mx-claude-stop-autoarm.sh" \
    || fail "auto-arm must foreground the arm wrapper inside the hook-owned process tree"
  grep -q 'asyncRewake' "$ROOT/bin/mx-claude-stop-autoarm.sh" \
    || fail "auto-arm header must document its asyncRewake registration contract"
  pass "settings.json registers the asyncRewake auto-arm with timeout >= 28800 and a foreground arm"
}

# --- scope and gates ----------------------------------------------------------

test_inert_in_child_worktree() {
  local base dir out status
  base="$TMP_ROOT/actors-base"
  dir="$TMP_ROOT/actors-wt"
  make_actor_worktree_dir "$base" "$dir" >/dev/null
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" actionable
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 0 "$status" "hook must stay inert in a child task worktree"
  [ ! -e "$dir/state/arm-ran" ] || fail "hook armed inside a child worktree"
  [ ! -e "$dir/state/.claude-autoarm-epoch" ] || fail "hook wrote an epoch inside a child worktree"
  pass "auto-arm: inert in a linked child worktree even when in-flight"
}

test_inert_without_session_lock() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/no-lock")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" actionable
  # No state/.lock: run the hook directly (no fake harness, no lock file).
  out=$(printf '%s\n' '{"session_id":"s"}' | MX_HOME="$dir" bash "$dir/bin/mx-claude-stop-autoarm.sh" 2>&1); status=$?
  expect_code 0 "$status" "hook must stay inert when no session holds the home lock"
  [ ! -e "$dir/state/arm-ran" ] || fail "hook armed without a session lock"
  pass "auto-arm: inert with no session lock"
}

test_reclaims_stale_session_lock_before_arming() {
  local dir out status expected_owner actual_owner
  dir=$(make_primary_dir "$TMP_ROOT/stale-lock")
  : > "$dir/state/task.meta"
  printf '9999999\n' > "$dir/state/.lock"
  write_arm_fixture "$dir" actionable
  out=$(printf '%s\n' '{"session_id":"stale"}' \
    | MX_HOME="$dir" "$FAKE_CLAUDE" -c '
        printf "%s\n" "$$" > "$MX_HOME/state/expected-owner"
        "$MX_HOME/bin/mx-claude-stop-autoarm.sh"
      ' 2>&1); status=$?
  expect_code 2 "$status" "a dead recorded session owner must be reclaimed before the actionable rewake"
  expected_owner=$(cat "$dir/state/expected-owner")
  actual_owner=$(cat "$dir/state/.lock")
  [ "$actual_owner" = "$expected_owner" ] || fail "stale session lock was not claimed by the current harness: expected $expected_owner, got $actual_owner"
  [ -e "$dir/state/arm-ran" ] || fail "hook did not arm after reclaiming the stale session lock"
  [ "$(epoch_outcome "$dir")" = rewake ] || fail "stale-lock recovery must record outcome=rewake"
  pass "auto-arm: a demonstrably dead recorded session owner is reclaimed through mx-lock.sh before arming"
}

test_inert_when_lock_held_by_other_harness() {
  local dir other out status owner_after
  dir=$(make_primary_dir "$TMP_ROOT/other-lock")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" actionable
  # The trailing no-op keeps the fake harness process alive instead of allowing
  # bash to exec the final sleep into a non-harness process.
  "$FAKE_CLAUDE" -c 'sleep 60; :' &
  other=$!
  printf '%s\n' "$other" > "$dir/state/.lock"
  out=$(printf '%s\n' '{"session_id":"s"}' | MX_HOME="$dir" "$FAKE_CLAUDE" -c '"$MX_HOME/bin/mx-claude-stop-autoarm.sh"' 2>&1); status=$?
  owner_after=$(cat "$dir/state/.lock")
  kill "$other" 2>/dev/null || true
  wait "$other" 2>/dev/null || true
  expect_code 0 "$status" "hook must stay inert when another live harness holds the session lock"
  [ "$owner_after" = "$other" ] || fail "hook replaced another live harness owner: expected $other, got $owner_after"
  [ ! -e "$dir/state/arm-ran" ] || fail "hook armed while another session owned the lock"
  [ ! -e "$dir/state/.claude-autoarm-epoch" ] || fail "hook wrote an epoch while another session owned the lock"
  pass "auto-arm: inert without arm, rewake, or lock replacement when another live harness owns the home"
}

test_inert_when_afk() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/afk")
  : > "$dir/state/task.meta"
  : > "$dir/state/.afk"
  write_arm_fixture "$dir" actionable
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 0 "$status" "hook must never arm or rewake while away mode owns triage"
  [ ! -e "$dir/state/arm-ran" ] || fail "hook armed while state/.afk existed"
  pass "auto-arm: inert while AFK owns supervision"
}

test_stale_lock_recovery_preserves_afk_and_need_gates() {
  local afk_dir idle_dir out status
  afk_dir=$(make_primary_dir "$TMP_ROOT/stale-afk")
  : > "$afk_dir/state/task.meta"
  : > "$afk_dir/state/.afk"
  printf '9999999\n' > "$afk_dir/state/.lock"
  write_arm_fixture "$afk_dir" actionable
  out=$(printf '%s\n' '{"session_id":"stale-afk"}' | MX_HOME="$afk_dir" "$FAKE_CLAUDE" -c '"$MX_HOME/bin/mx-claude-stop-autoarm.sh"' 2>&1); status=$?
  expect_code 0 "$status" "a stale owner must not widen the AFK gate"
  [ "$(cat "$afk_dir/state/.lock")" = 9999999 ] || fail "AFK stale lock was reclaimed despite away ownership"
  [ ! -e "$afk_dir/state/arm-ran" ] || fail "stale AFK home armed"

  idle_dir=$(make_primary_dir "$TMP_ROOT/stale-idle")
  printf '9999999\n' > "$idle_dir/state/.lock"
  write_arm_fixture "$idle_dir" actionable
  out=$(printf '%s\n' '{"session_id":"stale-idle"}' | MX_HOME="$idle_dir" "$FAKE_CLAUDE" -c '"$MX_HOME/bin/mx-claude-stop-autoarm.sh"' 2>&1); status=$?
  expect_code 0 "$status" "a stale owner must not widen the supervision-need gate"
  [ "$(cat "$idle_dir/state/.lock")" = 9999999 ] || fail "idle stale lock was reclaimed without supervision need"
  [ ! -e "$idle_dir/state/arm-ran" ] || fail "stale idle home armed"
  pass "auto-arm: stale-owner recovery leaves the AFK and supervision-need gates unchanged"
}

test_inert_when_system_idle() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/idle")
  write_arm_fixture "$dir" actionable
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 0 "$status" "hook must exit 0 in an idle home"
  [ ! -e "$dir/state/arm-ran" ] || fail "hook armed an idle home"
  pass "auto-arm: inert with nothing in flight"
}

# --- the armed cycle ----------------------------------------------------------

test_actionable_close_rewakes_with_reason() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/actionable")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" actionable
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 2 "$status" "an actionable arm close must exit 2 so Claude rewakes"
  assert_contains "$out" "broker watcher wake" "rewake must carry the wake banner"
  assert_contains "$out" "stale: fixture-win actionable" "rewake must carry the arm's reason line"
  assert_contains "$out" "bin/mx-wake-drain.sh" "rewake must direct the drain-first protocol"
  assert_contains "$out" "do NOT run bin/mx-watch-arm.sh" "rewake must forbid a duplicate model re-arm"
  [ "$(epoch_outcome "$dir")" = rewake ] || fail "epoch must record outcome=rewake, got: $(epoch_outcome "$dir")"
  [ ! -e "$dir/state/.claude-autoarm.lock" ] || fail "owner lock must be released after the cycle"
  [ -e "$dir/state/arm-ran" ] || fail "hook never foregrounded the arm wrapper"
  pass "auto-arm: actionable close translates to exactly one exit-2 rewake with reason"
}

test_failed_close_rewakes_with_failure_banner() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/failed")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" failed
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 2 "$status" "a typed watcher failure must rewake as an alarm"
  assert_contains "$out" "watcher cycle FAILED" "failure rewake must carry the failure banner"
  assert_contains "$out" "watcher: FAILED" "failure rewake must carry the arm's typed failure"
  assert_contains "$out" "repair supervision" "failure rewake must direct the manual repair"
  [ "$(epoch_outcome "$dir")" = rewake ] || fail "epoch must record outcome=rewake, got: $(epoch_outcome "$dir")"
  pass "auto-arm: watcher: FAILED translates to an exit-2 alarm rewake"
}

test_clean_close_exits_silently() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/clean")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" clean
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 0 "$status" "a clean arm close with no actionable reason must not rewake"
  [ -z "$out" ] || fail "clean close produced output: $out"
  [ "$(epoch_outcome "$dir")" = clean ] || fail "epoch must record outcome=clean, got: $(epoch_outcome "$dir")"
  pass "auto-arm: clean close exits silently with a clean epoch"
}

test_single_flight_admits_exactly_one_owner() {
  local dir rc1 rc2 count
  dir=$(make_primary_dir "$TMP_ROOT/single-flight")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" slow-actionable
  MX_HOME="$dir" "$FAKE_CLAUDE" -c '
    printf "%s\n" "$$" > "$MX_HOME/state/.lock"
    printf "%s\n" "{\"session_id\":\"s\"}" | "$MX_HOME/bin/mx-claude-stop-autoarm.sh" >/dev/null 2>"$MX_HOME/state/err1" &
    p1=$!
    printf "%s\n" "{\"session_id\":\"s\"}" | "$MX_HOME/bin/mx-claude-stop-autoarm.sh" >/dev/null 2>"$MX_HOME/state/err2" &
    p2=$!
    wait "$p1"; echo $? > "$MX_HOME/state/rc1"
    wait "$p2"; echo $? > "$MX_HOME/state/rc2"
  '
  rc1=$(cat "$dir/state/rc1")
  rc2=$(cat "$dir/state/rc2")
  count=$(wc -l < "$dir/state/arm-ran" | tr -d ' ')
  [ "$count" -eq 1 ] || fail "concurrent firings must foreground exactly one arm, saw $count"
  { [ "$rc1" = 2 ] && [ "$rc2" = 0 ]; } || { [ "$rc1" = 0 ] && [ "$rc2" = 2 ]; } \
    || fail "exactly one firing must translate the close (rc 2) and the other must no-op (rc 0), got rc1=$rc1 rc2=$rc2"
  pass "auto-arm: concurrent firings admit one owner and one rewake translation"
}

test_need_vanished_mid_cycle_closes_quietly() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/vanished")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" meta-vanishes
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 0 "$status" "an actionable close after the system went idle must not rewake"
  [ -z "$out" ] || fail "vanished-need close produced output: $out"
  [ "$(epoch_outcome "$dir")" = clean ] || fail "epoch must record outcome=clean, got: $(epoch_outcome "$dir")"
  pass "auto-arm: need vanishing mid-cycle closes without a rewake"
}

test_afk_mid_cycle_suppresses_rewake() {
  local dir out status
  dir=$(make_primary_dir "$TMP_ROOT/afk-mid")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" afk-appears
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 0 "$status" "AFK appearing mid-cycle must suppress the primary rewake"
  [ -z "$out" ] || fail "AFK-suppressed close produced output: $out"
  [ "$(epoch_outcome "$dir")" = afk ] || fail "epoch must record outcome=afk, got: $(epoch_outcome "$dir")"
  pass "auto-arm: mid-cycle AFK hands triage to the daemon with no rewake"
}

test_active_in_marked_daemon_home() {
  local dir out status
  dir=$(make_daemon_dir "$TMP_ROOT/daemon")
  : > "$dir/state/task.meta"
  write_arm_fixture "$dir" actionable
  out=$(run_autoarm "$dir" 2>/dev/null); status=$?
  expect_code 2 "$status" "a marked daemon home must get the same active auto-arm as the main primary"
  [ -e "$dir/state/arm-ran" ] || fail "hook did not arm in a marked daemon home"
  [ "$(epoch_outcome "$dir")" = rewake ] || fail "daemon epoch must record outcome=rewake"
  pass "auto-arm: active in a marked daemon home"
}

test_mx_lock_status_still_works_with_shared_lib() {
  local out
  out=$(MX_HOME="$TMP_ROOT/lock-status-home" bash "$ROOT/bin/mx-lock.sh" status 2>&1)
  assert_contains "$out" "lock: free" "mx-lock.sh status must keep working after the session-lock lib extraction"
  pass "mx-lock: shared session-lock lib preserves the status path"
}

test_settings_registers_autoarm_with_multi_hour_timeout
test_inert_in_child_worktree
test_inert_without_session_lock
test_reclaims_stale_session_lock_before_arming
test_inert_when_lock_held_by_other_harness
test_inert_when_afk
test_stale_lock_recovery_preserves_afk_and_need_gates
test_inert_when_system_idle
test_actionable_close_rewakes_with_reason
test_failed_close_rewakes_with_failure_banner
test_clean_close_exits_silently
test_single_flight_admits_exactly_one_owner
test_need_vanished_mid_cycle_closes_quietly
test_afk_mid_cycle_suppresses_rewake
test_active_in_marked_daemon_home
test_mx_lock_status_still_works_with_shared_lib
