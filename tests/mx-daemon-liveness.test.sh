#!/usr/bin/env bash
# tests/mx-daemon-liveness.test.sh - the session-start daemon liveness
# guarantee owned by bin/mx-backend.sh's detailed mx_backend_agent_state and
# bin/mx-bootstrap.sh's daemon_liveness_sweep that acts on it.
#
# The gap under test (AGENTS.md "Session start"; evidence 2026-07-07): a
# daemon agent that has exited leaves its backend endpoint alive as a bare
# shell. mx_backend_target_exists only checks pane PRESENCE, so it reports
# that shell "alive"; recovery only respawns endpoints reported dead, and the
# watcher deliberately exempts daemons from stale-pane detection (an idle
# daemon pane is healthy by design). A dead-shell daemon was therefore
# invisible to every existing check and sat dead indefinitely.
#
# The guarantees under test:
#   - mx_backend_agent_state is the detailed owner that distinguishes alive,
#     dead, missing, ambiguous, unreadable, and unverified.
#   - The tmux classifier returns missing only after a readable session
#     inventory omits the exact window, regardless of display-message fallback.
#   - The Herdr classifier preserves the proven husk mapping while separating a
#     missing pane from an existing agent-less pane.
#   - mx_backend_agent_alive preserves the older three-state compatibility view.
#   - bin/mx-bootstrap.sh's daemon_liveness_sweep recovers only dead or
#     missing endpoints, keeps successful recovery and already-live results
#     silent by default, and reports ambiguous and unreadable targets distinctly.
#   - The sweep converges: once a daemon reads alive, a later run never
#     re-touches it (idempotent by construction, not by remembering what it
#     already did).
#   - The sweep is skipped entirely under MX_BOOTSTRAP_DETECT_ONLY=1 (the
#     read-only session path), matching the other mutating sweeps.
#   - The sweep is naturally scoped to the primary: with no kind=daemon
#     meta present (a daemon's own state/ never holds one, since
#     daemons never spawn daemons), it is a silent no-op.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

BASE_PATH=${MX_TEST_BASE_PATH:-/usr/bin:/bin:/usr/sbin:/sbin}
mx_git_identity fmtest fmtest@example.com

TMP_ROOT=$(mx_test_tmproot mx-daemon-liveness)

# --- unit level: mx_backend_tmux_agent_state --------------------------------

# make_probe_tmux <dir> <pane_current_command>: a fake tmux whose
# #{pane_current_command} display-message query answers with the fixed value;
# every other subcommand is a silent no-op success.
make_probe_tmux() {
  local dir=$1 comm=$2 fakebin
  fakebin=$(mx_fakebin "$dir")
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
set -u
case "\${1:-}" in
  display-message)
    for a in "\$@"; do case "\$a" in *pane_current_command*) printf '%s\n' '$comm'; exit 0 ;; esac; done
    exit 0 ;;
  list-windows) printf '%s\n' win; exit 0 ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$fakebin"
}

# make_failed_probe_tmux <dir> <inventory>: missing and present fail the pane
# read, while unreadable returns a misleading fallback node process but fails
# the inventory that must be authoritative.
make_failed_probe_tmux() {
  local dir=$1 inventory=$2 fakebin
  fakebin=$(mx_fakebin "$dir")
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
set -u
case "\${1:-}" in
  display-message)
    [ '$inventory' = unreadable ] && { printf '%s\n' node; exit 0; }
    exit 1
    ;;
  list-windows)
    case '$inventory' in
      missing) printf '%s\n' main ; exit 0 ;;
      missing-session) printf '%s\n' "can't find session: sess" >&2; exit 1 ;;
      missing-server) printf '%s\n' "no server running on /tmp/tmux-test/default" >&2; exit 1 ;;
      missing-socket) printf '%s\n' "error connecting to /tmp/tmux-test/default (No such file or directory)" >&2; exit 1 ;;
      present) printf '%s\n' mx-sm1 ; exit 0 ;;
      *) printf '%s\n' "permission denied" >&2; exit 1 ;;
    esac
    ;;
esac
exit 1
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$fakebin"
}

test_tmux_agent_state_classifies() {
  local fb out

  for harness in claude codex; do
    fb=$(make_probe_tmux "$TMP_ROOT/tmux-$harness" "$harness")
    out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:win' "$ROOT")
    [ "$out" = alive ] || fail "a live $harness foreground process should classify as alive, got '$out'"
  done

  for shell in zsh bash -zsh; do
    fb=$(make_probe_tmux "$TMP_ROOT/tmux-${shell#-}" "$shell")
    out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:win' "$ROOT")
    [ "$out" = dead ] || fail "a bare $shell foreground process should classify as dead, got '$out'"
  done

  fb=$(make_probe_tmux "$TMP_ROOT/tmux-node" node)
  out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:win' "$ROOT")
  [ "$out" = ambiguous ] || fail "an existing node process should classify as ambiguous, got '$out'"
  [ "$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_alive tmux sess:win' "$ROOT")" = unknown ] \
    || fail "the compatibility view must keep an existing node process unknown"

  fb=$(make_failed_probe_tmux "$TMP_ROOT/tmux-missing" missing)
  out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:mx-sm1' "$ROOT")
  [ "$out" = missing ] || fail "a readable inventory omitting the target should classify as missing, got '$out'"
  [ "$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_alive tmux sess:mx-sm1' "$ROOT")" = dead ] \
    || fail "the compatibility view should treat an authoritatively missing target as dead"

  for inventory in present unreadable; do
    fb=$(make_failed_probe_tmux "$TMP_ROOT/tmux-$inventory" "$inventory")
    out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:mx-sm1' "$ROOT")
    [ "$out" = unreadable ] || fail "a $inventory inventory case should stay unreadable, got '$out'"
  done

  for inventory in missing-session missing-server missing-socket; do
    fb=$(make_failed_probe_tmux "$TMP_ROOT/tmux-$inventory" "$inventory")
    out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:mx-sm1' "$ROOT")
    [ "$out" = missing ] || fail "a confirmed $inventory inventory failure should classify as missing, got '$out'"
  done

  pass "mx_backend_tmux_agent_state: separates live, dead, missing, ambiguous, and unreadable"
}

test_tmux_agent_state_rejects_malformed_targets_before_probe() {
  local fakebin marker target out
  fakebin=$(mx_fakebin "$TMP_ROOT/tmux-malformed")
  marker="$TMP_ROOT/tmux-malformed-called"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
printf 'called\n' > "$MX_TEST_TMUX_MARKER"
printf 'bash\n'
SH
  chmod +x "$fakebin/tmux"

  for target in sess sess: :win sess:win:extra; do
    out=$(PATH="$fakebin:$BASE_PATH" MX_TEST_TMUX_MARKER="$marker" \
      bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux "$1"' "$ROOT" "$target")
    [ "$out" = unreadable ] || fail "malformed tmux target '$target' should classify as unreadable, got '$out'"
    [ ! -e "$marker" ] || fail "malformed tmux target '$target' invoked tmux"
  done

  pass "mx_backend_tmux_agent_state: rejects malformed targets before probing tmux"
}

# --- unit level: mx_backend_herdr_agent_state -------------------------------

test_herdr_agent_state_preserves_husk_classifier() {
  local pane_state expected out

  for row in 'dead missing' 'no-agent dead' 'live alive' 'unknown unreadable'; do
    pane_state=${row%% *}
    expected=${row#* }
    out=$(MX_TEST_PANE_STATE="$pane_state" bash -c '. "$0/bin/backends/herdr.sh"; mx_backend_herdr_pane_agent_state() { printf "%s" "$MX_TEST_PANE_STATE"; }; mx_backend_herdr_agent_state "sess:p1"' "$ROOT")
    [ "$out" = "$expected" ] || fail "Herdr pane state $pane_state should map to $expected, got '$out'"
  done

  out=$(bash -c '. "$0/bin/backends/herdr.sh"; mx_backend_herdr_agent_state "no-colon-target"' "$ROOT")
  [ "$out" = unreadable ] || fail "an unparseable Herdr target should classify as unreadable, got '$out'"

  out=$(bash -c '. "$0/bin/backends/herdr.sh"; mx_backend_herdr_pane_agent_state() { printf "no-agent"; }; mx_backend_herdr_agent_alive "sess:p1"' "$ROOT")
  [ "$out" = dead ] || fail "the Herdr compatibility view should keep a no-agent husk dead, got '$out'"

  pass "mx_backend_herdr_agent_state: preserves missing/no-agent/live/unknown husk behavior"
}

# --- unit level: the generic dispatchers ------------------------------------

test_agent_state_dispatcher_and_compatibility() {
  local fb out

  fb=$(make_probe_tmux "$TMP_ROOT/dispatch-tmux" claude)
  out=$(PATH="$fb:$BASE_PATH" bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state tmux sess:win' "$ROOT")
  [ "$out" = alive ] || fail "detailed dispatcher should route tmux, got '$out'"

  out=$(bash -c '. "$0/bin/mx-backend.sh"; mx_backend_source herdr; mx_backend_herdr_pane_agent_state() { printf "live"; }; mx_backend_agent_state herdr sess:p1' "$ROOT")
  [ "$out" = alive ] || fail "detailed dispatcher should route Herdr, got '$out'"

  out=$(bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_state cmux ws:sf' "$ROOT")
  [ "$out" = unverified ] || fail "cmux should remain unverified, got '$out'"
  out=$(bash -c '. "$0/bin/mx-backend.sh"; mx_backend_agent_alive cmux ws:sf' "$ROOT")
  [ "$out" = unknown ] || fail "the compatibility dispatcher should map unverified to unknown, got '$out'"

  pass "mx_backend_agent_state: routes tmux/Herdr and keeps cmux unverified"
}

# --- sweep level: bin/mx-bootstrap.sh's daemon_liveness_sweep -----------

# make_toolchain <dir>: the fixed set of stubs bin/mx-bootstrap.sh's read-only
# diagnostics need to stay quiet (mirrors tests/mx-daemon-sync.test.sh's
# make_fake_toolchain), MINUS tmux - callers add their own controllable tmux.
make_toolchain() {
  local dir=$1 fakebin
  fakebin=$(mx_fakebin "$dir")
  mx_fake_exit0 "$fakebin" node
  cat > "$fakebin/gh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fakebin/gh"
  cat > "$fakebin/treehouse" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = get ] && [ "${2:-}" = --help ]; then
  printf '%s\n' 'Usage: treehouse get [--lease]'
fi
exit 0
SH
  chmod +x "$fakebin/treehouse"
  cat > "$fakebin/no-mistakes" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = --version ]; then
  printf '%s\n' 'no-mistakes version v1.31.2 (fake)'
  exit 0
fi
exit 0
SH
  chmod +x "$fakebin/no-mistakes"
  printf '%s\n' "$fakebin"
}

# make_liveness_tmux <dir>: a controllable tmux stub. MX_TEST_PANE_CMD may be
# a foreground command, `missing` (readable inventory omits the window), or
# `unreadable` (both pane and inventory reads fail).
make_liveness_tmux() {
  local dir=$1 fakebin
  fakebin=$(mx_fakebin "$dir")
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
mode=${MX_TEST_PANE_CMD:-zsh}
case "${1:-}" in
  display-message)
    for a in "$@"; do
      case "$a" in
        *pane_current_command*)
          case "$mode" in
            missing) printf '%s\n' node; exit 0 ;;
            unreadable) exit 1 ;;
            *) printf '%s\n' "$mode"; exit 0 ;;
          esac
          ;;
      esac
    done
    exit 0
    ;;
  list-windows)
    case "$mode" in
      missing) printf '%s\n' main; exit 0 ;;
      unreadable) exit 1 ;;
      *) [ -e "${MX_TMUX_CALL_LOG:?}.killed" ] || printf '%s\n' mx-sm1; exit 0 ;;
    esac
    ;;
  new-window|kill-window)
    printf '%s\n' "$*" >> "${MX_TMUX_CALL_LOG:?}"
    [ "${1:-}" = kill-window ] && : > "${MX_TMUX_CALL_LOG}.killed"
    [ "${MX_TEST_FAIL_NEW_WINDOW:-0}" = 1 ] && [ "${1:-}" = new-window ] && exit 1
    [ "${1:-}" = new-window ] && rm -f "${MX_TMUX_CALL_LOG}.killed"
    exit 0
    ;;
  has-session) exit 0 ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$fakebin"
}

# new_world <name>: a scratch broker HOME (state/, watcher beacon, pinned
# harness) with no kind=daemon meta yet. MX_ROOT is left to resolve
# naturally to the real checkout under test ($ROOT), exactly as production
# always has it - this sweep's own mx-spawn.sh invocation resolves the
# daemon harness through $MX_ROOT/bin/mx-harness.sh, which only exists in
# the real tree. The harness is pinned because ambient own-harness detection is
# environment-dependent: interactive harness sessions expose markers or parent
# process names, while a plain pipeline shell can fall through to "unknown",
# which has no mx-spawn.sh launch template.
new_world() {
  local name=$1 w
  w="$TMP_ROOT/$name"
  mkdir -p "$w/home/state" "$w/home/config"
  touch "$w/home/state/.last-watcher-beat"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf '%s\n' "$w"
}

# add_sm_home <w> <id> <window>: a plain (non-git) daemon home - the
# probe/respawn machinery under test never requires the home to be a real
# worktree; a non-git home just makes the unrelated fast-forward sweep log a
# harmless "not a git repo" skip.
add_sm_home() {
  local w=$1 id=$2 window=$3 harness=${4:-claude}
  local home="$w/$id"
  mkdir -p "$home/bin" "$home/data" "$home/state" "$home/config" "$home/projects"
  printf '%s\n' "$id" > "$home/.mx-daemon-home"
  printf '# Multplx\n' > "$home/AGENTS.md"
  printf 'charter\n' > "$home/data/charter.md"
  {
    printf 'window=%s\n' "$window"
    printf 'kind=daemon\n'
    printf 'harness=%s\n' "$harness"
    printf 'home=%s\n' "$home"
  } > "$w/home/state/$id.meta"
}

run_bootstrap() {  # <fakebin> <home> <pane-cmd> <call-log> [extra env...] -> stdout
  local fb=$1 home=$2 cmd=$3 log=$4; shift 4
  PATH="$fb:$BASE_PATH" TMUX='' MX_BACKEND=tmux MX_HOME="$home" \
    MX_TEST_PANE_CMD="$cmd" MX_TMUX_CALL_LOG="$log" \
    env "$@" "$ROOT/bin/mx-bootstrap.sh" 2>&1
}

test_sweep_respawns_confirmed_dead_daemon() {
  local w fb tmuxfb log out
  w=$(new_world sweep-dead)
  add_sm_home "$w" sm1 broker:mx-sm1
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" zsh "$log")

  assert_not_contains "$out" "DAEMON_LIVENESS: daemon sm1: respawned" \
    "a successfully respawned daemon should be handled silently"
  assert_contains "$(cat "$log")" "kill-window -t broker:mx-sm1" \
    "the stale endpoint must be killed before respawn (tmux refuses a same-named window over a live one)"
  assert_contains "$(cat "$log")" "new-window" \
    "a confirmed-dead daemon should actually be relaunched"
  pass "sweep: a confirmed-dead daemon endpoint is killed and respawned"
}

test_sweep_leaves_alive_daemon_untouched() {
  local w fb tmuxfb log out
  w=$(new_world sweep-alive)
  add_sm_home "$w" sm1 broker:mx-sm1
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" claude "$log")

  assert_not_contains "$out" "DAEMON_LIVENESS: daemon sm1: already-live" \
    "an already-live daemon should be handled silently"
  [ ! -s "$log" ] || fail "an already-live daemon must never be killed or respawned: $(cat "$log")"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" claude "$log" MX_BOOTSTRAP_VERBOSE_FACTS=1)
  assert_contains "$out" "BOOTSTRAP_INFO: daemon sm1 already live (backend=tmux)" \
    "verbose diagnostics should identify the already-live outcome"
  [ ! -s "$log" ] || fail "verbose reporting must not touch an already-live daemon: $(cat "$log")"
  pass "sweep: an already-live daemon is untouched and distinguishable in verbose diagnostics"
}

test_sweep_respawns_authoritatively_missing_pi_daemon() {
  local w fb tmuxfb log out
  w=$(new_world sweep-missing-pi)
  add_sm_home "$w" sm1 broker:mx-sm1 pi
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" missing "$log")

  assert_not_contains "$out" "DAEMON_LIVENESS:" "a successful missing-window recovery should stay silent by default"
  assert_contains "$(cat "$log")" "new-window" "an authoritatively missing Pi daemon should be relaunched"
  assert_not_contains "$(cat "$log")" "kill-window" "an absent window should not need a destructive pre-kill"
  pass "sweep: an authoritatively missing Pi daemon window is relaunched"
}

test_sweep_never_acts_on_ambiguous_existing_process() {
  local w fb tmuxfb log out
  w=$(new_world sweep-ambiguous)
  add_sm_home "$w" sm1 broker:mx-sm1 pi
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" node "$log")

  assert_contains "$out" "DAEMON_LIVENESS: daemon sm1: skipped: existing endpoint has ambiguous agent process" \
    "an existing Pi-shaped node process should be reported as ambiguous"
  [ ! -s "$log" ] || fail "an ambiguous existing process must never trigger kill or relaunch: $(cat "$log")"
  pass "sweep: an existing ambiguous Pi process prevents duplicate recovery"
}

test_sweep_never_acts_on_transient_unreadability() {
  local w fb tmuxfb log out
  w=$(new_world sweep-unreadable)
  add_sm_home "$w" sm1 broker:mx-sm1 pi
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" unreadable "$log")

  assert_contains "$out" "DAEMON_LIVENESS: daemon sm1: skipped: endpoint probe unreadable" \
    "a transiently unreadable target should be distinguished from an absent one"
  [ ! -s "$log" ] || fail "an unreadable target must never trigger kill or relaunch: $(cat "$log")"
  pass "sweep: transient target unreadability never licenses recovery"
}

test_sweep_reports_missing_endpoint_relaunch_failure() {
  local w fb tmuxfb log out
  w=$(new_world sweep-missing-failure)
  add_sm_home "$w" sm1 broker:mx-sm1 pi
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" missing "$log" MX_TEST_FAIL_NEW_WINDOW=1)

  assert_contains "$out" "DAEMON_LIVENESS: daemon sm1: respawn failed after recorded endpoint confidently missing" \
    "a failed missing-endpoint relaunch should retain its authorizing cause"
  pass "sweep: failed relaunch diagnostics distinguish a confidently missing endpoint"
}

test_sweep_never_acts_on_unverified_harness_dead_reading() {
  local w fb tmuxfb log out
  w=$(new_world sweep-unverified-harness)
  add_sm_home "$w" sm1 broker:mx-sm1 custom-agent
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" zsh "$log")

  assert_contains "$out" "DAEMON_LIVENESS: daemon sm1: skipped: recorded harness 'custom-agent' is unverified for recovery" \
    "an unverified harness should not let a dead endpoint become actionable"
  [ ! -s "$log" ] || fail "an unverified harness must never trigger kill or relaunch: $(cat "$log")"
  pass "sweep: an unverified harness blocks recovery with a concrete diagnostic"
}

test_sweep_converges_no_retouch_once_alive() {
  local w fb tmuxfb log out1 out2
  w=$(new_world sweep-idempotent)
  add_sm_home "$w" sm1 broker:mx-sm1
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  # Round 1: dead -> respawned silently (kill + new-window logged).
  out1=$(run_bootstrap "$tmuxfb:$fb" "$w/home" zsh "$log")
  assert_not_contains "$out1" "DAEMON_LIVENESS: daemon sm1: respawned" "round 1 should handle the successful respawn silently"
  [ -s "$log" ] || fail "round 1 should have logged the kill+respawn window operations"

  # Round 2: the (now-respawned) daemon is genuinely alive - a second
  # sweep must converge to a pure no-op, not respawn again.
  : > "$log"
  out2=$(run_bootstrap "$tmuxfb:$fb" "$w/home" claude "$log")
  assert_not_contains "$out2" "DAEMON_LIVENESS: daemon sm1: already-live" "round 2 should handle the already-live daemon silently"
  [ ! -s "$log" ] || fail "round 2 must not re-kill or re-respawn an already-live daemon: $(cat "$log")"
  pass "sweep: idempotent by construction - a live daemon is never re-touched on a later run"
}

test_sweep_skipped_under_detect_only() {
  local w fb tmuxfb log out
  w=$(new_world sweep-detect-only)
  add_sm_home "$w" sm1 broker:mx-sm1
  mkdir -p "$w/home/config"
  printf 'codex\n' > "$w/home/config/actor-harness"
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" zsh "$log" MX_BOOTSTRAP_DETECT_ONLY=1)

  assert_not_contains "$out" "ACTOR_HARNESS_OVERRIDE:" \
    "detect-only should keep routine harness facts silent"
  assert_not_contains "$out" "DAEMON_LIVENESS:" \
    "the read-only detect-only path must never run the mutating liveness sweep"
  [ ! -s "$log" ] || fail "detect-only must never touch any endpoint: $(cat "$log")"
  pass "sweep: skipped entirely under MX_BOOTSTRAP_DETECT_ONLY=1, exactly like the other mutating sweeps"
}

test_sweep_noop_with_no_daemon_meta() {
  local w fb tmuxfb log out
  w=$(new_world sweep-no-daemons)
  # No add_sm_home call: this state/ dir looks exactly like what a
  # daemon's OWN home always has (daemons never spawn daemons),
  # proving the sweep's primary-only scoping falls out naturally.
  fb=$(make_toolchain "$w"); tmuxfb=$(make_liveness_tmux "$w")
  log="$w/calls.log"; : > "$log"

  out=$(run_bootstrap "$tmuxfb:$fb" "$w/home" zsh "$log")

  assert_not_contains "$out" "DAEMON_LIVENESS:" \
    "with no kind=daemon meta present, the sweep must print nothing"
  [ ! -s "$log" ] || fail "with no daemon meta, no endpoint should ever be touched: $(cat "$log")"
  pass "sweep: a silent no-op with no kind=daemon meta present (a daemon home's own natural scoping)"
}

test_tmux_agent_state_classifies
test_tmux_agent_state_rejects_malformed_targets_before_probe
test_herdr_agent_state_preserves_husk_classifier
test_agent_state_dispatcher_and_compatibility
test_sweep_respawns_confirmed_dead_daemon
test_sweep_leaves_alive_daemon_untouched
test_sweep_respawns_authoritatively_missing_pi_daemon
test_sweep_never_acts_on_ambiguous_existing_process
test_sweep_never_acts_on_transient_unreadability
test_sweep_reports_missing_endpoint_relaunch_failure
test_sweep_never_acts_on_unverified_harness_dead_reading
test_sweep_converges_no_retouch_once_alive
test_sweep_skipped_under_detect_only
test_sweep_noop_with_no_daemon_meta

echo "# all mx-daemon-liveness tests passed"
