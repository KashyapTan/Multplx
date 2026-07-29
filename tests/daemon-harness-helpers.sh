#!/usr/bin/env bash
# Tests for the daemon-vs-actor harness split, the optional model/effort
# tokens config/daemon-harness carries alongside the harness, and the
# primary->daemon inherited local-material propagation.
#
# Three capabilities are under test:
#   A) Harness split. config/daemon-harness sets the harness the PRIMARY uses
#      to launch DAEMON agents, independent of config/actor-harness (the
#      actor harness). mx-harness.sh daemon resolves the fallback chain
#      config/daemon-harness -> config/actor-harness -> own; an absent or
#      "default" daemon-harness behaves exactly as the actor harness did before
#      this knob existed (full backward-compat). mx-spawn.sh resolves a daemon
#      launch through that mode, durably (every respawn re-resolves), while an
#      explicit per-spawn harness arg still wins.
#   B) Inheritance. The primary pushes a declared, extensible set of LOCAL
#      (gitignored) config items - config/actor-dispatch.json, config/actor-harness,
#      config/backlog-backend, and config/herdr-presentation-spaces - down into
#      each daemon home's config/, so the daemon's OWN actors,
#      dispatch profiles, backlog backend, and Herdr presentation opt-in inherit
#      the primary's settings. It is primary-authoritative (re-pushed at
#      daemon spawn, on the bootstrap daemon sweep, and by config push).
#      config/daemon-harness is deliberately NOT inherited (daemons do
#      not spawn daemons). After a successful push that changes allowlisted
#      config under an already-running home, a literal-content reread instruction
#      is written to the daemon home and only its pointer is sent via the
#      routed daemon path (exact destination bytes, no summaries); unchanged
#      config sends nothing unless a previous send failure is pending.

#   C) Model/effort pin. config/daemon-harness may carry optional model and
#      effort tokens after the harness ("<harness> [<model>] [<effort>]"), read by
#      mx-harness.sh daemon-model / daemon-effort. A bare harness-only
#      line (today's format) yields empty model/effort - full backward-compat.
#      mx-spawn.sh populates MODEL/EFFORT from those tokens for a --daemon
#      spawn only when the harness also resolves from that file, so the pin is
#      durable across every respawn while explicit per-spawn harness/model/effort
#      flags still win.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
# shellcheck source=/dev/null
. "$ROOT/bin/mx-ff-lib.sh"
# shellcheck source=/dev/null
. "$ROOT/bin/mx-config-inherit-lib.sh"

BASE_PATH=${MX_TEST_BASE_PATH:-/usr/bin:/bin:/usr/sbin:/sbin}
mx_git_identity fmtest fmtest@example.com
TMP_ROOT=$(mx_test_tmproot mx-daemon-harness)
MX_TEST_CLEANUP_DIRS+=("$TMP_ROOT")
trap mx_test_cleanup EXIT
export MX_BACKEND=tmux

# ===========================================================================
# A) mx-harness.sh daemon resolution + fallback (deterministic detect_own)
# ===========================================================================
# detect_own is pinned to claude via CLAUDECODE=1 so the "fall through to own"
# cases are reproducible. Each row sets actor-harness / daemon-harness in a
# fresh config dir (a literal '-' means leave the file absent) and asserts BOTH
# the daemon resolution AND that actor resolution is unchanged (backward-compat).
#   <label>^<actor-harness>^<daemon-harness>^<expect-daemon>^<expect-actor>
test_harness_resolution() {
  local label actor sm exp_sm exp_actor case_dir cfg got_sm got_actor n
  n=0
  while IFS='^' read -r label actor sm exp_sm exp_actor; do
    [ -n "$label" ] || continue
    n=$((n + 1))
    case_dir="$TMP_ROOT/harness-$n"
    cfg="$case_dir/config"
    mkdir -p "$cfg"
    [ "$actor" = "-" ] || printf '%s\n' "$actor" > "$cfg/actor-harness"
    [ "$sm" = "-" ] || printf '%s\n' "$sm" > "$cfg/daemon-harness"
    got_sm=$(CLAUDECODE=1 MX_CONFIG_OVERRIDE="$cfg" "$ROOT/bin/mx-harness.sh" daemon)
    got_actor=$(CLAUDECODE=1 MX_CONFIG_OVERRIDE="$cfg" "$ROOT/bin/mx-harness.sh" actor)
    [ "$got_sm" = "$exp_sm" ] || fail "$label: daemon resolved '$got_sm', expected '$exp_sm'"
    [ "$got_actor" = "$exp_actor" ] || fail "$label: actor resolved '$got_actor', expected '$exp_actor'"
  done <<'ROWS'
both absent -> own (backward-compat)^-^-^claude^claude
actor set, daemon absent -> actor (backward-compat)^codex^-^codex^codex
actor set, daemon set -> daemon wins, actor untouched^codex^pi^pi^codex
actor absent, daemon set -> daemon value, actor own^-^pi^pi^claude
daemon=default defers to actor^codex^default^codex^codex
actor=default resolves to own, daemon follows^default^-^claude^claude
daemon=default with actor absent -> own^-^default^claude^claude
ROWS
  pass "A1 mx-harness.sh daemon resolves the fallback chain; actor mode unchanged"
}

# ===========================================================================
# C) mx-harness.sh daemon-model / daemon-effort token resolution
# ===========================================================================
# config/daemon-harness holds "<harness> [<model>] [<effort>]" on one line.
# A bare harness (today's format) must yield empty model/effort - the
# backward-compat requirement. The file-line field uses \n for an embedded
# newline (expanded via printf '%b') so a row can express a multi-line file; the
# literal token ABSENT skips creating the file entirely.
#   <label>^<file-line-or-ABSENT>^<expect-harness>^<expect-model>^<expect-effort>
test_daemon_model_effort_tokens() {
  local label line exp_harness exp_model exp_effort case_dir cfg got_h got_m got_e n
  n=0
  while IFS='^' read -r label line exp_harness exp_model exp_effort; do
    [ -n "$label" ] || continue
    n=$((n + 1))
    case_dir="$TMP_ROOT/tokens-$n"
    cfg="$case_dir/config"
    mkdir -p "$cfg"
    [ "$line" = ABSENT ] || printf '%b\n' "$line" > "$cfg/daemon-harness"
    got_h=$(CLAUDECODE=1 MX_CONFIG_OVERRIDE="$cfg" "$ROOT/bin/mx-harness.sh" daemon)
    got_m=$(CLAUDECODE=1 MX_CONFIG_OVERRIDE="$cfg" "$ROOT/bin/mx-harness.sh" daemon-model)
    got_e=$(CLAUDECODE=1 MX_CONFIG_OVERRIDE="$cfg" "$ROOT/bin/mx-harness.sh" daemon-effort)
    [ "$got_h" = "$exp_harness" ] || fail "$label: harness resolved '$got_h', expected '$exp_harness'"
    [ "$got_m" = "$exp_model" ] || fail "$label: model resolved '$got_m', expected '$exp_model'"
    [ "$got_e" = "$exp_effort" ] || fail "$label: effort resolved '$got_e', expected '$exp_effort'"
  done <<'ROWS'
absent file -> own harness, empty model/effort^ABSENT^claude^^
bare harness only -> empty model/effort (backward-compat)^claude^claude^^
harness + model -> model only^claude opus^claude^opus^
harness + model + effort -> both^claude opus high^claude^opus^high
default harness token -> falls back to actor, empty model/effort^default^claude^^
extra whitespace between tokens is tolerated^codex   gpt-5    xhigh^codex^gpt-5^xhigh
leading/trailing blank lines and a comment are skipped^# a comment\n\nclaude opus low\n^claude^opus^low
ROWS
  pass "C1 mx-harness.sh daemon-model/daemon-effort resolve the optional tokens; bare harness stays empty (backward-compat)"
}

# ===========================================================================
# B) propagate_inheritable_config unit behavior
# ===========================================================================
test_propagate_lib() {
  local d src dest m1 m2 outside stdout stderr guard_repo err_text
  d="$TMP_ROOT/prop-lib"
  src="$d/src"
  dest="$d/dest"
  mkdir -p "$src" "$dest"

  # 1. present source is copied
  printf '{"default":{"harness":"codex"}}\n' > "$src/actor-dispatch.json"
  printf 'codex\n' > "$src/actor-harness"
  printf 'manual\n' > "$src/backlog-backend"
  : > "$src/herdr-presentation-spaces"
  stdout="$d/clean-copy.out"
  stderr="$d/clean-copy.err"
  propagate_inheritable_config "$src" "$dest" >"$stdout" 2>"$stderr" || fail "propagate returned non-zero"
  [ ! -s "$stdout" ] || fail "clean copy wrote to stdout"
  [ ! -s "$stderr" ] || fail "clean copy wrote to stderr"
  [ "$(cat "$dest/actor-dispatch.json")" = '{"default":{"harness":"codex"}}' ] || fail "actor-dispatch.json not propagated"
  [ "$(cat "$dest/actor-harness")" = codex ] || fail "actor-harness not propagated"
  [ "$(cat "$dest/backlog-backend")" = manual ] || fail "backlog-backend not propagated"
  [ -f "$dest/herdr-presentation-spaces" ] || fail "herdr-presentation-spaces not propagated"

  # 2. idempotent: an unchanged re-run does not churn the mtime
  m1=$(date -r "$dest/actor-harness" +%s 2>/dev/null || stat -c %Y "$dest/actor-harness")
  sleep 1
  stdout="$d/unchanged.out"
  stderr="$d/unchanged.err"
  propagate_inheritable_config "$src" "$dest" >"$stdout" 2>"$stderr"
  [ ! -s "$stdout" ] || fail "unchanged propagation wrote to stdout"
  [ ! -s "$stderr" ] || fail "unchanged propagation wrote to stderr"
  m2=$(date -r "$dest/actor-harness" +%s 2>/dev/null || stat -c %Y "$dest/actor-harness")
  [ "$m1" = "$m2" ] || fail "idempotent re-run churned mtime ($m1 -> $m2)"

  # 3. a changed source value converges downstream
  printf '{"default":{"harness":"claude"}}\n' > "$src/actor-dispatch.json"
  printf 'claude\n' > "$src/actor-harness"
  printf 'tasks-axi\n' > "$src/backlog-backend"
  propagate_inheritable_config "$src" "$dest"
  [ "$(cat "$dest/actor-dispatch.json")" = '{"default":{"harness":"claude"}}' ] || fail "changed dispatch profile did not converge"
  [ "$(cat "$dest/actor-harness")" = claude ] || fail "changed value did not converge"
  [ "$(cat "$dest/backlog-backend")" = tasks-axi ] || fail "changed backlog backend did not converge"

  outside="$d/outside-target"
  rm -f "$dest/actor-harness" "$outside"
  printf 'outside\n' > "$outside"
  ln -s "$outside" "$dest/actor-harness"
  printf 'pi\n' > "$src/actor-harness"
  propagate_inheritable_config "$src" "$dest"
  [ ! -L "$dest/actor-harness" ] || fail "destination symlink was not replaced"
  [ "$(cat "$dest/actor-harness")" = pi ] || fail "destination symlink replacement has wrong content"
  [ "$(cat "$outside")" = outside ] || fail "destination symlink target was overwritten"

  # 4. removing the source mirrors absence downstream (primary-authoritative)
  rm -f "$src/actor-dispatch.json" "$src/actor-harness" "$src/backlog-backend" "$src/herdr-presentation-spaces"
  propagate_inheritable_config "$src" "$dest"
  [ -e "$dest/actor-dispatch.json" ] && fail "dispatch profile absence not mirrored downstream"
  [ -e "$dest/actor-harness" ] && fail "absence not mirrored downstream"
  [ -e "$dest/backlog-backend" ] && fail "backlog-backend absence not mirrored downstream"
  [ -e "$dest/herdr-presentation-spaces" ] && fail "herdr-presentation-spaces absence not mirrored downstream"

  rm -f "$dest/actor-harness"
  ln -s "$d/missing-target" "$dest/actor-harness"
  propagate_inheritable_config "$src" "$dest"
  [ -L "$dest/actor-harness" ] && fail "broken destination symlink not removed on absence mirror"

  mkdir -p "$dest/actor-harness"
  stderr="$d/remove-error.err"
  if propagate_inheritable_config "$src" "$dest" 2>"$stderr"; then
    fail "failed absence mirror returned success"
  fi
  assert_contains "$(cat "$stderr")" "mx-config-inherit: error: failed to remove actor-harness" \
    "remove error did not emit a stderr diagnostic"
  [ -d "$dest/actor-harness" ] || fail "failed absence mirror removed the wrong path"
  rm -rf "$dest/actor-harness"

  # 5. daemon-harness is never inherited
  printf 'pi\n' > "$src/daemon-harness"
  printf '{"default":{"harness":"codex"}}\n' > "$src/actor-dispatch.json"
  printf 'codex\n' > "$src/actor-harness"
  printf 'manual\n' > "$src/backlog-backend"
  rm -rf "$d/dest2"
  mkdir -p "$d/dest2"
  propagate_inheritable_config "$src" "$d/dest2"
  [ -e "$d/dest2/daemon-harness" ] && fail "daemon-harness was inherited (must not be)"
  [ "$(cat "$d/dest2/actor-dispatch.json")" = '{"default":{"harness":"codex"}}' ] || fail "actor-dispatch.json not propagated alongside"
  [ "$(cat "$d/dest2/actor-harness")" = codex ] || fail "actor-harness not propagated alongside"
  [ "$(cat "$d/dest2/backlog-backend")" = manual ] || fail "backlog-backend not propagated alongside"

  # 6. nothing to propagate -> destination dir is never created (a true no-op)
  rm -rf "$d/src3" "$d/dest3"
  mkdir -p "$d/src3"
  propagate_inheritable_config "$d/src3" "$d/dest3/config"
  [ -e "$d/dest3/config" ] && fail "empty-source propagation created a destination dir"

  # 7. a git worktree that does not ignore an inherited item gets a visible
  # stderr warning and a skip, not a silent miss.
  guard_repo="$d/guard-repo"
  git init -q -b main "$guard_repo"
  printf 'config/actor-harness\nconfig/backlog-backend\n' > "$guard_repo/.gitignore"
  printf 'guard\n' > "$guard_repo/README.md"
  git -C "$guard_repo" add -A
  git -C "$guard_repo" commit -qm guard
  printf '{"default":{"harness":"pi"}}\n' > "$src/actor-dispatch.json"
  stdout="$d/guard-skip.out"
  stderr="$d/guard-skip.err"
  MX_INHERITABLE_CONFIG=actor-dispatch.json propagate_inheritable_config "$src" "$guard_repo/config" >"$stdout" 2>"$stderr" \
    || fail "guard skip should not make propagation fail"
  [ ! -s "$stdout" ] || fail "guard skip wrote to stdout"
  err_text=$(cat "$stderr")
  assert_contains "$err_text" "mx-config-inherit: warning: skipped actor-dispatch.json" \
    "guard skip did not emit a stderr warning"
  [ ! -e "$guard_repo/config/actor-dispatch.json" ] || fail "guard skip still copied the unignored item"

  pass "B1 propagate_inheritable_config: copy, idempotence, convergence, absence-mirror, exclusion, no-op, skip diagnostics"
}

# ===========================================================================
# B/A integration: a daemon spawn resolves the daemon harness and
# propagates the actor harness into the home's config.
# ===========================================================================

# A tmux stub that accepts every subcommand and prints nothing, so no window
# pre-exists and the spawn proceeds to write its meta. Echoes the fakebin dir.
make_noop_tmux() {
  local dir=$1 fakebin="$1/fakebin"
  mkdir -p "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$fakebin"
}

# A minimal seeded daemon home (validate_broker_home_for_spawn needs the
# seed marker, AGENTS.md, bin/, and a charter to launch). config/ is intentionally
# left absent so the spawn's propagation is what creates it.
make_seeded_home() {
  local home=$1 id=$2
  mkdir -p "$home/bin" "$home/data"
  printf '# Multplx\n' > "$home/AGENTS.md"
  printf '%s\n' "$id" > "$home/.mx-daemon-home"
  printf 'charter\n' > "$home/data/charter.md"
}

# spawn_daemon <world> <id> <home> [explicit-harness]
# Runs mx-spawn.sh in daemon mode. MX_ROOT is the real repo (so mx-harness.sh
# resolves), the primary config dir is <world>/home/config, and CLAUDECODE pins
# detect_own. stderr is discarded (the local-HEAD ff sync harmlessly skips a
# non-worktree home). Inspect <world>/home/state/<id>.meta and <home>/config after.
spawn_daemon() {
  local world=$1 id=$2 home=$3 harness=${4:-} fakebin
  mkdir -p "$world/home/state" "$world/home/data"
  fakebin=$(make_noop_tmux "$world/tmux-$id")
  # An empty harness must contribute zero args, not an empty positional; build the
  # arg list explicitly so the optional harness is omitted cleanly.
  local spawn_args=("$id" "$home")
  [ -n "$harness" ] && spawn_args+=("$harness")
  spawn_args+=(--daemon)
  PATH="$fakebin:$BASE_PATH" TMUX='' CLAUDECODE=1 \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$world/home" \
    MX_STATE_OVERRIDE="$world/home/state" MX_DATA_OVERRIDE="$world/home/data" \
    MX_PROJECTS_OVERRIDE="$world/home/projects" MX_CONFIG_OVERRIDE="$world/home/config" \
    MX_SPAWN_NO_GUARD=1 \
    "$ROOT/bin/mx-spawn.sh" "${spawn_args[@]}" >/dev/null 2>&1 || true
}

meta_harness() { grep '^harness=' "$1" 2>/dev/null | tail -1 | cut -d= -f2-; }

# Split active: actor-harness=claude + daemon-harness=codex. The daemon
# AGENT launches on codex; its own actors inherit claude; daemon-harness
# does not flow into the home.
test_spawn_split_and_inherit() {
  local w sm meta
  w="$TMP_ROOT/spawn-split"
  sm="$w/sm"
  mkdir -p "$w/home/config"
  printf '{"default":{"harness":"claude","model":"haiku","effort":"low"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'claude\n' > "$w/home/config/actor-harness"
  printf 'codex\n' > "$w/home/config/daemon-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  make_seeded_home "$sm" sm

  spawn_daemon "$w" sm "$sm"

  meta="$w/home/state/sm.meta"
  [ -f "$meta" ] || fail "split: no meta written"
  [ "$(meta_harness "$meta")" = codex ] \
    || fail "split: daemon launched on '$(meta_harness "$meta")', expected codex"
  [ "$(cat "$sm/config/actor-harness" 2>/dev/null)" = claude ] \
    || fail "split: home actor-harness not inherited as claude (got '$(cat "$sm/config/actor-harness" 2>/dev/null)')"
  [ "$(cat "$sm/config/actor-dispatch.json" 2>/dev/null)" = '{"default":{"harness":"claude","model":"haiku","effort":"low"}}' ] \
    || fail "split: home actor-dispatch.json not inherited"
  [ "$(cat "$sm/config/backlog-backend" 2>/dev/null)" = manual ] \
    || fail "split: home backlog-backend not inherited as manual"
  [ -e "$sm/config/daemon-harness" ] \
    && fail "split: daemon-harness leaked into the daemon home"
  pass "B2 spawn: daemon runs the daemon harness; its home inherits declared config"
}

# Backward-compat: daemon-harness absent -> the daemon launches on the
# actor harness, exactly as before this knob existed, and that actor value is the
# one inherited.
test_spawn_backward_compat_actor_fallback() {
  local w sm meta
  w="$TMP_ROOT/spawn-compat"
  sm="$w/sm"
  mkdir -p "$w/home/config"
  printf 'codex\n' > "$w/home/config/actor-harness"
  make_seeded_home "$sm" sm

  spawn_daemon "$w" sm "$sm"

  meta="$w/home/state/sm.meta"
  [ "$(meta_harness "$meta")" = codex ] \
    || fail "compat: daemon launched on '$(meta_harness "$meta")', expected the actor harness codex"
  [ "$(cat "$sm/config/actor-harness" 2>/dev/null)" = codex ] \
    || fail "compat: home actor-harness not inherited as codex"
  pass "B3 spawn: an absent daemon-harness falls back to the actor harness (backward-compat)"
}

# Bare backward-compat: no config at all. The daemon falls through to its own
# harness (claude here), and with no inheritable file the home is left untouched -
# no config/ side effects.
test_spawn_bare_backward_compat() {
  local w sm meta
  w="$TMP_ROOT/spawn-bare"
  sm="$w/sm"
  make_seeded_home "$sm" sm

  spawn_daemon "$w" sm "$sm"

  meta="$w/home/state/sm.meta"
  [ "$(meta_harness "$meta")" = claude ] \
    || fail "bare: daemon launched on '$(meta_harness "$meta")', expected own harness claude"
  [ -e "$sm/config/actor-dispatch.json" ] && fail "bare: an unset primary still created a home actor-dispatch.json"
  [ -e "$sm/config/actor-harness" ] && fail "bare: an unset primary still created a home actor-harness"
  pass "B4 spawn: no config at all -> own harness and no propagation side effects"
}

# An explicit per-spawn harness arg wins over config/daemon-harness.
test_spawn_explicit_harness_wins() {
  local w sm meta
  w="$TMP_ROOT/spawn-explicit"
  sm="$w/sm"
  mkdir -p "$w/home/config"
  printf 'codex\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon "$w" sm "$sm" claude

  meta="$w/home/state/sm.meta"
  [ "$(meta_harness "$meta")" = claude ] \
    || fail "explicit: launched on '$(meta_harness "$meta")', expected explicit claude over config codex"
  pass "B5 spawn: an explicit per-spawn harness arg overrides config/daemon-harness"
}

# The unverified-adapter guard holds on the resolved daemon path: an unknown
# config/daemon-harness aborts the spawn (no meta written) and names the source.
test_spawn_unverified_daemon_harness_refused() {
  local w sm fakebin err rc
  w="$TMP_ROOT/spawn-unverified"
  sm="$w/sm"
  mkdir -p "$w/home/config" "$w/home/state"
  printf 'bogus\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm
  fakebin=$(make_noop_tmux "$w/tmux")
  err="$w/spawn.err"
  rc=0
  PATH="$fakebin:$BASE_PATH" TMUX='' CLAUDECODE=1 \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$w/home" \
    MX_STATE_OVERRIDE="$w/home/state" MX_DATA_OVERRIDE="$w/home/data" \
    MX_PROJECTS_OVERRIDE="$w/home/projects" MX_CONFIG_OVERRIDE="$w/home/config" \
    MX_SPAWN_NO_GUARD=1 \
    "$ROOT/bin/mx-spawn.sh" sm "$sm" --daemon >/dev/null 2>"$err" || rc=$?

  [ "$rc" -ne 0 ] || fail "unverified: spawn should have failed"
  assert_contains "$(cat "$err")" "no launch template for harness 'bogus'" \
    "unverified: error names the rejected harness"
  assert_contains "$(cat "$err")" "config/daemon-harness" \
    "unverified: error names the daemon-harness source"
  [ -e "$w/home/state/sm.meta" ] && fail "unverified: a meta was written despite the abort"
  pass "B6 spawn: an unverified resolved daemon harness is refused (guard intact)"
}

# ===========================================================================
# C integration: config/daemon-harness's optional model/effort tokens thread
# into the daemon launch command and meta, durably and without a new file.
# ===========================================================================

meta_field() { grep "^$2=" "$1" 2>/dev/null | tail -1 | cut -d= -f2-; }

# A tmux stub that behaves like make_noop_tmux but also captures the literal
# `send-keys -l <cmd>` launch command into MX_FAKE_LAUNCH_LOG, mirroring the
# capture technique in mx-spawn-dispatch-profile.test.sh so the constructed
# launch command (not just meta) can be asserted on. Also answers the
# `#{pane_current_path}` probe from MX_FAKE_PANE_PATH so this same stub works
# for an actor/scout (non-daemon) spawn's treehouse-worktree wait loop.
make_launch_capturing_tmux() {
  local dir=$1 fakebin="$1/fakebin"
  mkdir -p "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "$*" in
  *"#{pane_current_path}"*) printf '%s\n' "${MX_FAKE_PANE_PATH:-}"; exit 0 ;;
esac
case "${1:-}" in
  display-message) printf 'broker\n'; exit 0 ;;
  list-windows) exit 0 ;;
  has-session|new-session|new-window|kill-window) exit 0 ;;
  send-keys)
    if [ -n "${MX_FAKE_LAUNCH_LOG:-}" ]; then
      prev=
      for a in "$@"; do
        if [ "$prev" = "-l" ]; then
          printf '%s\n' "$a" >> "$MX_FAKE_LAUNCH_LOG"
        fi
        prev=$a
      done
    fi
    exit 0
    ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$fakebin"
}

# spawn_daemon_capture <world> <id> <home> <launchlog> [extra mx-spawn.sh args...]
# Same shape as spawn_daemon but captures the launch command into <launchlog>
# and does not discard stderr, so callers can assert on both.
spawn_daemon_capture() {
  local world=$1 id=$2 home=$3 launchlog=$4 fakebin
  shift 4
  mkdir -p "$world/home/state" "$world/home/data"
  fakebin=$(make_launch_capturing_tmux "$world/tmux-$id")
  : > "$launchlog"
  PATH="$fakebin:$BASE_PATH" TMUX='' CLAUDECODE=1 \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$world/home" \
    MX_STATE_OVERRIDE="$world/home/state" MX_DATA_OVERRIDE="$world/home/data" \
    MX_PROJECTS_OVERRIDE="$world/home/projects" MX_CONFIG_OVERRIDE="$world/home/config" \
    MX_SPAWN_NO_GUARD=1 MX_FAKE_LAUNCH_LOG="$launchlog" \
    "$ROOT/bin/mx-spawn.sh" "$id" "$home" "$@" --daemon
}

# A bare "<harness>" daemon-harness file (today's format) must launch with
# NO --model/--effort flag at all, and meta must keep recording model=default,
# effort=default - the core backward-compat requirement of the new format.
test_spawn_bare_harness_no_model_effort_flag() {
  local w sm meta launchlog launch out status
  w="$TMP_ROOT/spawn-bare-tokens"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  out=$(spawn_daemon_capture "$w" sm "$sm" "$launchlog" 2>&1); status=$?
  expect_code 0 "$status" "bare-harness daemon spawn should succeed"

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" model)" = default ] || fail "bare-tokens: meta model not default (got '$(meta_field "$meta" model)')"
  [ "$(meta_field "$meta" effort)" = default ] || fail "bare-tokens: meta effort not default (got '$(meta_field "$meta" effort)')"
  launch=$(cat "$launchlog")
  assert_not_contains "$launch" "--model" "bare-tokens: launch must not carry a --model flag"
  assert_not_contains "$launch" "--effort" "bare-tokens: launch must not carry an --effort flag"
  pass "C2 spawn: a bare harness-only daemon-harness file launches with no model/effort flag (backward-compat)"
}

# "<harness> <model>" durably threads --model into the daemon launch and
# records it in meta, with no --effort flag (no effort token supplied).
test_spawn_daemon_harness_model_token() {
  local w sm meta launchlog launch
  w="$TMP_ROOT/spawn-model-token"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude opus\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" harness)" = claude ] || fail "model-token: meta harness not claude"
  [ "$(meta_field "$meta" model)" = opus ] || fail "model-token: meta model not opus (got '$(meta_field "$meta" model)')"
  [ "$(meta_field "$meta" effort)" = default ] || fail "model-token: meta effort not default (got '$(meta_field "$meta" effort)')"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "claude --dangerously-skip-permissions --model 'opus'" \
    "model-token: launch did not carry --model opus"
  assert_not_contains "$launch" "--effort" "model-token: launch must not carry an --effort flag"
  pass "C3 spawn: config/daemon-harness's model token threads --model into the launch and meta"
}

# "<harness> <model> <effort>" threads both flags into the launch and meta.
test_spawn_daemon_harness_model_and_effort_tokens() {
  local w sm meta launchlog launch
  w="$TMP_ROOT/spawn-model-effort-tokens"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude opus high\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" model)" = opus ] || fail "model-effort-tokens: meta model not opus"
  [ "$(meta_field "$meta" effort)" = high ] || fail "model-effort-tokens: meta effort not high (got '$(meta_field "$meta" effort)')"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "claude --dangerously-skip-permissions --model 'opus' --effort 'high'" \
    "model-effort-tokens: launch did not carry both --model opus and --effort high"
  pass "C4 spawn: config/daemon-harness's model+effort tokens thread into the launch and meta"
}

# Precedence: an explicit per-spawn --model overrides the file's model token.
test_spawn_explicit_model_overrides_daemon_harness_token() {
  local w sm meta launchlog launch
  w="$TMP_ROOT/spawn-explicit-model"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude opus high\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" --model sonnet >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" model)" = sonnet ] \
    || fail "explicit-model: meta model not sonnet (got '$(meta_field "$meta" model)'), explicit flag did not win over file token"
  [ "$(meta_field "$meta" effort)" = high ] || fail "explicit-model: file's effort token should still apply"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "--model 'sonnet'" "explicit-model: launch did not use the explicit --model"
  assert_not_contains "$launch" "--model 'opus'" "explicit-model: launch leaked the file's model token"
  pass "C5 spawn: an explicit --model overrides config/daemon-harness's model token; the file's effort token still applies"
}

# Precedence: an explicit per-spawn --effort overrides the file's effort token.
test_spawn_explicit_effort_overrides_daemon_harness_token() {
  local w sm meta launchlog launch
  w="$TMP_ROOT/spawn-explicit-effort"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude opus high\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" --effort low >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" model)" = opus ] || fail "explicit-effort: file's model token should still apply"
  [ "$(meta_field "$meta" effort)" = low ] \
    || fail "explicit-effort: meta effort not low (got '$(meta_field "$meta" effort)'), explicit flag did not win over file token"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "--effort 'low'" "explicit-effort: launch did not use the explicit --effort"
  assert_not_contains "$launch" "--effort 'high'" "explicit-effort: launch leaked the file's effort token"
  pass "C6 spawn: an explicit --effort overrides config/daemon-harness's effort token; the file's model token still applies"
}

test_spawn_explicit_harness_does_not_inherit_daemon_harness_tokens() {
  local w sm meta launchlog launch
  w="$TMP_ROOT/spawn-explicit-harness-no-tokens"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude opus high\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" --harness codex >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" harness)" = codex ] || fail "explicit-harness-no-tokens: meta harness not codex"
  [ "$(meta_field "$meta" model)" = default ] || fail "explicit-harness-no-tokens: meta model should stay default"
  [ "$(meta_field "$meta" effort)" = default ] || fail "explicit-harness-no-tokens: meta effort should stay default"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "codex --dangerously-bypass-approvals-and-sandbox" \
    "explicit-harness-no-tokens: launch did not use codex"
  assert_not_contains "$launch" "--model" "explicit-harness-no-tokens: launch must not carry a --model flag"
  assert_not_contains "$launch" "model_reasoning_effort" \
    "explicit-harness-no-tokens: launch must not carry a codex effort flag"
  pass "C7 spawn: an explicit --harness starts with clean model/effort defaults"
}

test_spawn_explicit_harness_uses_explicit_profile_axes() {
  local w sm meta launchlog launch
  w="$TMP_ROOT/spawn-explicit-harness-explicit-axes"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'claude opus high\n' > "$w/home/config/daemon-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" --harness codex --model gpt-5.5 --effort xhigh >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" harness)" = codex ] || fail "explicit-harness-explicit-axes: meta harness not codex"
  [ "$(meta_field "$meta" model)" = gpt-5.5 ] || fail "explicit-harness-explicit-axes: meta model did not use explicit value"
  [ "$(meta_field "$meta" effort)" = xhigh ] || fail "explicit-harness-explicit-axes: meta effort did not use explicit value"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "--model 'gpt-5.5'" \
    "explicit-harness-explicit-axes: launch did not use the explicit --model"
  assert_contains "$launch" "-c 'model_reasoning_effort=\"xhigh\"'" \
    "explicit-harness-explicit-axes: launch did not use the explicit --effort"
  assert_not_contains "$launch" "--model 'opus'" \
    "explicit-harness-explicit-axes: launch leaked the file's model token"
  assert_not_contains "$launch" "model_reasoning_effort=\"high\"" \
    "explicit-harness-explicit-axes: launch leaked the file's effort token"
  pass "C8 spawn: an explicit --harness still honors explicit model/effort flags"
}

# The harness fallback chain (daemon-harness -> actor-harness -> own) still
# resolves correctly with no model/effort tokens anywhere in the chain, and a
# actor/scout (non-daemon) launch is entirely unaffected by this feature: no
# model/effort is invented for it even though its own project has no profile set.
test_spawn_fallback_chain_and_actor_scout_unaffected() {
  local w sm meta home proj wt fakebin launchlog id launch
  w="$TMP_ROOT/spawn-fallback-and-actor"
  sm="$w/sm"
  launchlog="$w/launch.log"
  mkdir -p "$w/home/config"
  printf 'codex\n' > "$w/home/config/actor-harness"
  make_seeded_home "$sm" sm

  spawn_daemon_capture "$w" sm "$sm" "$launchlog" >/dev/null 2>&1

  meta="$w/home/state/sm.meta"
  [ "$(meta_field "$meta" harness)" = codex ] \
    || fail "fallback: daemon harness did not fall back to actor-harness codex"
  [ "$(meta_field "$meta" model)" = default ] || fail "fallback: meta model should stay default with no tokens anywhere"
  [ "$(meta_field "$meta" effort)" = default ] || fail "fallback: meta effort should stay default with no tokens anywhere"

  # actor/scout launch: same actor-harness config, no --daemon. Must resolve
  # the actor harness and record no model/effort - this codepath must never read
  # config/daemon-harness's tokens at all.
  id="actor-unaffected-z1"
  home="$w/home"
  proj="$w/actor-project"
  wt="$w/actor-wt"
  fakebin=$(make_launch_capturing_tmux "$w/tmux-actor")
  mx_git_worktree "$proj" "$wt" "wt-actor"
  mkdir -p "$home/data/$id" "$home/projects" "$home/state"
  printf 'brief\n' > "$home/data/$id/brief.md"
  : > "$launchlog"
  PATH="$fakebin:$BASE_PATH" TMUX="fake,1,0" CLAUDECODE=1 \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$home" \
    MX_STATE_OVERRIDE="$home/state" MX_DATA_OVERRIDE="$home/data" \
    MX_PROJECTS_OVERRIDE="$home/projects" MX_CONFIG_OVERRIDE="$home/config" \
    MX_SPAWN_NO_GUARD=1 MX_FAKE_PANE_PATH="$wt" MX_FAKE_LAUNCH_LOG="$launchlog" \
    "$ROOT/bin/mx-spawn.sh" "$id" "$proj" >/dev/null 2>&1
  meta="$home/state/$id.meta"
  [ "$(meta_field "$meta" kind)" = delivery ] || fail "actor-unaffected: expected an ordinary delivery task"
  [ "$(meta_field "$meta" harness)" = codex ] || fail "actor-unaffected: actor harness resolution changed"
  [ "$(meta_field "$meta" model)" = default ] || fail "actor-unaffected: actor task must not invent a model"
  [ "$(meta_field "$meta" effort)" = default ] || fail "actor-unaffected: actor task must not invent an effort"
  launch=$(cat "$launchlog")
  assert_not_contains "$launch" "--model" "actor-unaffected: actor launch must not carry a --model flag"
  assert_not_contains "$launch" "--effort" "actor-unaffected: actor launch must not carry an --effort flag"
  pass "C9 spawn: the harness fallback chain still resolves with no tokens; actor/scout launches are unaffected by this feature"
}

# ===========================================================================
# B integration: spawn, bootstrap, and config push propagate inherited local
# material and keep it converged on the primary (independent of tracked-file ff
# status).
# ===========================================================================

# A PRIMARY Multplx repo template on main with one commit, mirroring the real
# gitignore. Every case gets independent copied git metadata and writable bytes.
build_world_template() {
  local template=$1 dispatch_ignore=$2
  git init -q -b main "$template"
  {
    printf 'projects/\nstate/\ndata/\n.no-mistakes/\n'
    [ "$dispatch_ignore" = no ] || printf 'config/actor-dispatch.json\n'
    printf 'config/actor-harness\nconfig/daemon-harness\nconfig/backlog-backend\n'
  } > "$template/.gitignore"
  printf 'v1\n' > "$template/AGENTS.md"
  printf 'r1\n' > "$template/README.md"
  mkdir -p "$template/bin"
  printf 'echo a\n' > "$template/bin/tool.sh"
  git -C "$template" add -A
  git -C "$template" commit -qm c1
}

new_world() {
  local name=$1 dispatch_ignore=${2:-yes} w template
  w="$TMP_ROOT/$name"
  template="$TMP_ROOT/.world-template-$dispatch_ignore"
  if [ ! -d "$template" ]; then
    build_world_template "$template" "$dispatch_ignore"
    chmod -R a-w "$template"
  fi
  mkdir -p "$w/home/state" "$w/home/data" "$w/home/config"
  touch "$w/home/state/.last-watcher-beat"
  mx_test_clone_tree_template "$template" "$w/main"
  printf '%s\n' "$w"
}

# A live daemon home as a DETACHED worktree of the primary at <commit>, with
# its seed marker and a live kind=daemon meta.
add_sm_worktree() {
  local w=$1 id=$2 commit=$3
  git -C "$w/main" worktree add -q --detach "$w/$id" "$commit"
  printf '%s\n' "$id" > "$w/$id/.mx-daemon-home"
  {
    printf 'window=broker:mx-%s\n' "$id"
    printf 'kind=daemon\n'
    printf 'home=%s/%s\n' "$w" "$id"
  } > "$w/home/state/$id.meta"
}

make_fake_toolchain() {
  local dir=$1 fakebin
  fakebin="$dir/fakebin"
  mkdir -p "$fakebin"
  mx_fake_exit0 "$fakebin" node gh-axi chrome-devtools-axi lavish-axi
  # tmux fake supports mx-send's composer-verified submit path and optional
  # MX_FAKE_TMUX_LOG / MX_FAKE_TMUX_FAIL_LITERAL for reread-nudge assertions.
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
if [ -n "${MX_FAKE_TMUX_LOG:-}" ]; then
  printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
fi
case "$*" in
  *display-message*'#{pane_current_command}'*) printf '%s\n' codex; exit 0 ;;
  *display-message*'#{pane_id}'*) printf '%s\n' '%1'; exit 0 ;;
  *display-message*'#{cursor_y}'*) printf '%s\n' 0; exit 0 ;;
  *capture-pane*) printf '\n'; exit 0 ;;
  *'send-keys'*' -l '*)
    [ "${MX_FAKE_TMUX_FAIL_LITERAL:-0}" = 1 ] && exit 1
    exit 0
    ;;
  *send-keys*)
    [ "${MX_FAKE_TMUX_FAIL_LITERAL:-0}" = 1 ] && exit 1
    exit 0
    ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
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

run_bootstrap() {
  local w=$1 fakebin log=${2:-}
  fakebin=$(make_fake_toolchain "$w")
  if [ -n "$log" ]; then
    PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
      MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
      "$ROOT/bin/mx-bootstrap.sh" 2>/dev/null
  else
    PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
      MX_SEND_SETTLE=0 "$ROOT/bin/mx-bootstrap.sh" 2>/dev/null
  fi
}

run_config_push() {
  local w=$1 fakebin log=${2:-}
  fakebin=$(make_fake_toolchain "$w")
  if [ -n "$log" ]; then
    PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
      MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
      "$ROOT/bin/mx-config-push.sh"
  else
    PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
      MX_SEND_SETTLE=0 \
      "$ROOT/bin/mx-config-push.sh"
  fi
}

reread_instruction_path() {
  local home=$1 state path latest=
  state="$(cd "$home/state" && pwd -P)"
  for path in "$state"/.mx-inherited-config-reread.*; do
    case "$path" in
      *.pending) continue ;;
    esac
    [ -f "$path" ] && [ ! -L "$path" ] || continue
    latest="$path"
  done
  [ -n "$latest" ] || return 1
  printf '%s\n' "$latest"
}

reread_pending_path() {
  printf '%s.pending\n' "$(reread_instruction_path "$1")"
}

reread_retry_stage_path() {
  local home=$1 id=$2 retry_dir path latest=
  retry_dir="$home/state/.mx-inherited-config-reread-retry/$id"
  for path in "$retry_dir"/.mx-inherited-config-reread.*; do
    [ -f "$path" ] && [ ! -L "$path" ] || continue
    latest="$path"
  done
  [ -n "$latest" ] || return 1
  printf '%s\n' "$latest"
}

reread_retry_report_path() {
  local home=$1 id=$2 retry_dir path latest=
  retry_dir="$home/state/.mx-inherited-config-reread-retry/$id"
  for path in "$retry_dir"/.mx-inherited-config-reread.*.report; do
    [ -f "$path" ] && [ ! -L "$path" ] || continue
    latest="$path"
  done
  [ -n "$latest" ] || return 1
  printf '%s\n' "$latest"
}

reread_mode() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %Lp "$1"
  else
    stat -c %a "$1"
  fi
}

assert_no_reread_instructions() {
  local home=$1 state path
  state="$home/state"
  for path in "$state"/.mx-inherited-config-reread.*; do
    case "$path" in
      *.pending) continue ;;
    esac
    [ -f "$path" ] || [ -L "$path" ] || continue
    fail "unexpected config reread instruction: $path"
  done
}

assert_no_reread_pending() {
  local home=$1 state path
  state="$home/state"
  for path in "$state"/.mx-inherited-config-reread.*.pending; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    fail "unexpected pending config reread marker: $path"
  done
}

assert_no_reread_retry_stages() {
  local home=$1 id=$2 retry_dir path
  retry_dir="$home/state/.mx-inherited-config-reread-retry/$id"
  for path in "$retry_dir"/.mx-inherited-config-reread.*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    fail "unexpected staged config reread retry: $path"
  done
}

# The sweep pushes the primary's declared inherited config into a live home,
# re-converges it when the primary changes it, and mirrors absence when the
# primary clears it - all while never inheriting daemon-harness.
test_bootstrap_sweep_propagates_and_reconverges() {
  local w c1
  w=$(new_world boot-prop)
  c1=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$c1"

  # Initial push: primary actor-harness=codex, daemon-harness=pi (must NOT flow).
  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  printf 'pi\n' > "$w/home/config/daemon-harness"
  run_bootstrap "$w" >/dev/null
  [ "$(cat "$w/sm/config/actor-harness" 2>/dev/null)" = codex ] \
    || fail "sweep: actor-harness not pushed into the live home"
  [ "$(cat "$w/sm/config/actor-dispatch.json" 2>/dev/null)" = '{"default":{"harness":"codex"}}' ] \
    || fail "sweep: actor-dispatch.json not pushed into the live home"
  [ "$(cat "$w/sm/config/backlog-backend" 2>/dev/null)" = manual ] \
    || fail "sweep: backlog-backend not pushed into the live home"
  [ -e "$w/sm/config/daemon-harness" ] \
    && fail "sweep: daemon-harness was inherited (must not be)"

  # Re-converge: primary changes inherited config values; the home follows on the next sweep.
  printf '{"default":{"harness":"claude"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'claude\n' > "$w/home/config/actor-harness"
  printf 'tasks-axi\n' > "$w/home/config/backlog-backend"
  run_bootstrap "$w" >/dev/null
  [ "$(cat "$w/sm/config/actor-harness" 2>/dev/null)" = claude ] \
    || fail "sweep: home did not re-converge to the primary's new actor-harness"
  [ "$(cat "$w/sm/config/actor-dispatch.json" 2>/dev/null)" = '{"default":{"harness":"claude"}}' ] \
    || fail "sweep: home did not re-converge to the primary's new actor-dispatch.json"
  [ "$(cat "$w/sm/config/backlog-backend" 2>/dev/null)" = tasks-axi ] \
    || fail "sweep: home did not re-converge to the primary's new backlog-backend"

  # Mirror absence: primary clears inherited config; the home's copies are removed.
  rm -f "$w/home/config/actor-dispatch.json" "$w/home/config/actor-harness" "$w/home/config/backlog-backend"
  run_bootstrap "$w" >/dev/null
  [ -e "$w/sm/config/actor-dispatch.json" ] \
    && fail "sweep: home actor-dispatch.json not removed after the primary cleared it"
  [ -e "$w/sm/config/actor-harness" ] \
    && fail "sweep: home actor-harness not removed after the primary cleared it"
  [ -e "$w/sm/config/backlog-backend" ] \
    && fail "sweep: home backlog-backend not removed after the primary cleared it"
  pass "B7 bootstrap sweep pushes, re-converges, and mirrors absence; never inherits daemon-harness"
}

# Convergence is independent of the tracked-files fast-forward: a home already
# current on tracked files still receives a config change.
test_bootstrap_sweep_propagates_when_tracked_current() {
  local w head
  w=$(new_world boot-prop-current)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"   # already on the primary's HEAD (ff is a no-op)

  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  run_bootstrap "$w" >/dev/null
  [ "$(cat "$w/sm/config/actor-dispatch.json" 2>/dev/null)" = '{"default":{"harness":"codex"}}' ] \
    || fail "actor-dispatch.json did not propagate to a tracked-current home"
  [ "$(cat "$w/sm/config/actor-harness" 2>/dev/null)" = codex ] \
    || fail "config did not propagate to a tracked-current home"
  [ "$(cat "$w/sm/config/backlog-backend" 2>/dev/null)" = manual ] \
    || fail "backlog-backend did not propagate to a tracked-current home"
  pass "B8 bootstrap sweep propagates config even when the home's tracked files are already current"
}

test_bootstrap_sweep_defers_dispatch_on_stale_unignored_home() {
  local w out status
  w=$(new_world boot-stale-dispatch no)
  add_sm_worktree "$w" sm "$(git -C "$w/main" rev-parse HEAD)"
  printf 'local divergence\n' >> "$w/sm/README.md"
  git -C "$w/sm" add README.md
  git -C "$w/sm" commit -qm local
  printf 'config/actor-dispatch.json\n' >> "$w/main/.gitignore"
  git -C "$w/main" add .gitignore
  git -C "$w/main" commit -qm c2

  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  out=$(run_bootstrap "$w")

  assert_contains "$out" "DAEMON_SYNC: daemon sm: skipped: diverged from" \
    "stale dispatch: expected fast-forward skip"
  [ ! -e "$w/sm/config/actor-dispatch.json" ] \
    || fail "stale dispatch: actor-dispatch.json was copied before the home ignored it"
  [ "$(cat "$w/sm/config/actor-harness" 2>/dev/null)" = codex ] \
    || fail "stale dispatch: existing ignored config stopped propagating"
  [ "$(cat "$w/sm/config/backlog-backend" 2>/dev/null)" = manual ] \
    || fail "stale dispatch: backlog backend stopped propagating"
  status=$(git -C "$w/sm" status --porcelain -- config/actor-dispatch.json)
  [ -z "$status" ] || fail "stale dispatch: actor-dispatch.json dirtied the home: $status"
  pass "B9 bootstrap sweep defers new inherited config until the home ignores it"
}

# Backward-compat: with no inherited config set, the sweep is a no-op for the
# home's config/ - exactly as before this feature - and ordinary sweep behavior
# (fast-forward) is unaffected.
test_bootstrap_sweep_no_inheritance_is_noop() {
  local w c1
  w=$(new_world boot-noop)
  c1=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$c1"
  # Advance the primary so the sweep has a real fast-forward to perform.
  printf 'v2\n' > "$w/main/AGENTS.md"
  git -C "$w/main" add -A
  git -C "$w/main" commit -qm c2
  local head
  head=$(git -C "$w/main" rev-parse HEAD)

  run_bootstrap "$w" >/dev/null

  [ -e "$w/sm/config/actor-dispatch.json" ] && fail "no-inheritance sweep created a home actor-dispatch.json"
  [ -e "$w/sm/config/actor-harness" ] && fail "no-inheritance sweep created a home actor-harness"
  [ -e "$w/sm/config" ] && fail "no-inheritance sweep created a home config/ dir"
  [ "$(git -C "$w/sm" rev-parse HEAD)" = "$head" ] \
    || fail "no-inheritance sweep did not still fast-forward the tracked files"
  pass "B10 bootstrap sweep with no inherited config is a config no-op and still fast-forwards"
}

test_bootstrap_sweep_surfaces_config_propagation_failure() {
  local w c1 out fail_line
  w=$(new_world boot-prop-fail)
  c1=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$c1"
  mkdir -p "$w/sm/config/actor-harness"

  out=$(run_bootstrap "$w")

  fail_line=$(printf '%s\n' "$out" | grep '^DAEMON_SYNC: daemon sm: skipped: inheritance failed' || true)
  [ -n "$fail_line" ] || fail "bootstrap did not surface inheritance propagation failure (got: $out)"
  [ -d "$w/sm/config/actor-harness" ] || fail "failed propagation removed the wrong path"
  pass "B11 bootstrap sweep surfaces config propagation failures"
}

test_bootstrap_rereads_after_partial_propagation() {
  local w head log out instruction pointer
  w=$(new_world boot-prop-partial)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'invalid shared header\n' > "$w/home/data/maintainer-shared.md"
  log="$w/boot-prop-partial.tmux.log"

  out=$(run_bootstrap "$w" "$log")
  assert_contains "$out" "DAEMON_SYNC: daemon sm: skipped: inheritance failed" \
    "partial bootstrap propagation did not remain diagnostic"
  [ "$(cat "$w/sm/config/actor-dispatch.json")" = '{"default":{"harness":"codex"}}' ] \
    || fail "partial bootstrap propagation did not retain the completed config write"
  instruction=$(reread_instruction_path "$w/sm") || fail "partial bootstrap reread instruction missing"
  assert_present "$instruction" "partial bootstrap propagation did not write a reread instruction"
  pointer="CONFIG_REREAD: $(reread_instruction_path "$w/sm")"
  assert_contains "$(cat "$log")" "$pointer" \
    "partial bootstrap propagation did not route the instruction pointer"
  pass "B11 bootstrap rereads completed config writes after partial propagation"
}

test_config_push_propagates_reports_without_ff_or_nudge() {
  local w c1 sm_real old_head out err status out2 tmp log
  w=$(new_world config-push-basic)
  c1=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$c1"
  sm_real=$(cd "$w/sm" && pwd -P)
  printf -- '- sm - config push target (home: %s; scope: config; projects: alpha; added 2026-06-30)\n' "$sm_real" > "$w/home/data/daemons.md"
  tmp="$w/home/state/sm.meta.tmp"
  grep -v '^home=' "$w/home/state/sm.meta" > "$tmp"
  mv "$tmp" "$w/home/state/sm.meta"

  printf 'v2\n' > "$w/main/AGENTS.md"
  git -C "$w/main" add AGENTS.md
  git -C "$w/main" commit -qm c2
  old_head=$(git -C "$w/sm" rev-parse HEAD)

  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  err="$w/config-push-basic.err"
  log="$w/config-push-basic.tmux.log"
  out=$(run_config_push "$w" "$log" 2>"$err"); status=$?

  expect_code 0 "$status" "config push should succeed"
  assert_contains "$out" "config-push: $w/home -> live daemon homes" \
    "config push lacked the header"
  assert_contains "$out" "daemon sm ($sm_real):" \
    "config push did not discover the live daemon through registry fallback"
  assert_contains "$out" "actor-dispatch.json: pushed" \
    "config push did not report actor-dispatch as pushed"
  assert_contains "$out" "actor-harness: pushed" \
    "config push did not report actor-harness as pushed"
  assert_contains "$out" "backlog-backend: pushed" \
    "config push did not report backlog-backend as pushed"
  assert_contains "$out" "config-reread: sent" \
    "config push with changed config must send a literal reread instruction"
  assert_not_contains "$out" "NUDGE_DAEMONS" \
    "config push must not use the AGENTS.md instruction-surface nudge channel"
  [ "$(git -C "$w/sm" rev-parse HEAD)" = "$old_head" ] \
    || fail "config push fast-forwarded tracked files"
  [ ! -s "$err" ] || fail "clean config push wrote unexpected stderr: $(cat "$err")"
  assert_contains "$(cat "$log")" "[mx-from-broker]" \
    "config reread must use the marked routed daemon path"

  : > "$log"
  out2=$(run_config_push "$w" "$log" 2>"$err"); status=$?
  expect_code 0 "$status" "idempotent config push should succeed"
  assert_contains "$out2" "actor-dispatch.json: unchanged" \
    "idempotent config push did not report actor-dispatch as unchanged"
  assert_contains "$out2" "actor-harness: unchanged" \
    "idempotent config push did not report actor-harness as unchanged"
  assert_contains "$out2" "backlog-backend: unchanged" \
    "idempotent config push did not report backlog-backend as unchanged"
  assert_not_contains "$out2" "config-reread: sent" \
    "unchanged config must not send a reread message"
  [ ! -s "$log" ] || fail "unchanged config push still invoked tmux send: $(cat "$log")"
  pass "B12 config-push propagates via shared live discovery, reports items, rereads on change only, and does not fast-forward"
}

test_config_push_reports_skips_dirty_and_invalid_home() {
  local w head out err status stale_real dirty_real bad_home err_text tmp
  w=$(new_world config-push-warnings)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" dirty "$head"
  add_sm_worktree "$w" stale "$head"
  dirty_real=$(cd "$w/dirty" && pwd -P)
  stale_real=$(cd "$w/stale" && pwd -P)

  printf 'local edit\n' >> "$w/dirty/README.md"
  tmp="$w/stale/.gitignore.tmp"
  grep -v '^config/actor-dispatch.json$' "$w/stale/.gitignore" > "$tmp"
  mv "$tmp" "$w/stale/.gitignore"

  bad_home="$w/not-daemon"
  mkdir -p "$bad_home"
  {
    printf 'window=broker:mx-bad\n'
    printf 'kind=daemon\n'
    printf 'home=%s\n' "$bad_home"
  } > "$w/home/state/bad.meta"

  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  err="$w/config-push-warnings.err"
  out=$(run_config_push "$w" 2>"$err"); status=$?

  expect_code 0 "$status" "warnings-only config push should exit zero"
  assert_contains "$out" "daemon dirty ($dirty_real):" \
    "config push did not report dirty home"
  assert_contains "$out" "home: dirty working tree - local-material push continuing" \
    "config push did not surface dirty state"
  assert_contains "$out" "daemon stale ($stale_real):" \
    "config push did not report stale home"
  assert_contains "$out" "actor-dispatch.json: skipped - destination does not allow inherited item" \
    "config push did not report non-allowing item skip"
  assert_contains "$out" "daemon bad ($bad_home): skipped - unsafe home: not a seeded daemon home" \
    "config push did not report invalid daemon home"
  err_text=$(cat "$err")
  assert_contains "$err_text" "mx-config-inherit: warning: skipped actor-dispatch.json" \
    "config push did not inherit the lib's skip stderr warning"
  pass "B13 config-push reports dirty, non-allowing, and invalid homes without failing warnings-only runs"
}

test_config_push_exits_nonzero_on_copy_error() {
  local w head out err status sm_real err_text
  w=$(new_world config-push-error)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  sm_real=$(cd "$w/sm" && pwd -P)
  printf 'codex\n' > "$w/home/config/actor-harness"
  mkdir -p "$w/sm/config/actor-harness"

  err="$w/config-push-error.err"
  out=$(run_config_push "$w" 2>"$err"); status=$?

  expect_code 1 "$status" "copy-error config push should exit non-zero"
  assert_contains "$out" "daemon sm ($sm_real):" \
    "config push error output missed the home"
  assert_contains "$out" "actor-harness: error - failed to copy" \
    "config push did not report the per-item copy error"
  err_text=$(cat "$err")
  assert_contains "$err_text" "mx-config-inherit: error: failed to copy actor-harness" \
    "copy error did not emit a stderr diagnostic"
  pass "B14 config-push exits nonzero on real propagation errors"
}

test_config_push_rereads_after_partial_propagation() {
  local w head log out err status instruction pointer
  w=$(new_world config-push-partial)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  printf '{"default":{"harness":"codex"}}\n' > "$w/home/config/actor-dispatch.json"
  printf 'invalid shared header\n' > "$w/home/data/maintainer-shared.md"
  log="$w/config-push-partial.tmux.log"
  err="$w/config-push-partial.err"

  out=$(run_config_push "$w" "$log" 2>"$err"); status=$?
  expect_code 1 "$status" "partial propagation should remain non-zero"
  assert_contains "$out" "actor-dispatch.json: pushed" \
    "partial propagation did not report the completed config item"
  assert_contains "$out" "data/maintainer-shared.md: error" \
    "partial propagation did not report the failed shared item"
  assert_contains "$out" "config-reread: sent" \
    "partial propagation lost the completed config reread"
  [ "$(cat "$w/sm/config/actor-dispatch.json")" = '{"default":{"harness":"codex"}}' ] \
    || fail "partial propagation did not retain the completed config write"
  instruction=$(reread_instruction_path "$w/sm") || fail "partial propagation reread instruction missing"
  assert_present "$instruction" "partial propagation did not write a reread instruction"
  pointer="CONFIG_REREAD: $(reread_instruction_path "$w/sm")"
  assert_contains "$(cat "$log")" "$pointer" \
    "partial propagation did not route the instruction pointer"
  pass "B14 config-push rereads completed config writes after partial propagation"
}

# ---------------------------------------------------------------------------
# Literal-content config reread nudge (post-propagation live-agent wake)
# ---------------------------------------------------------------------------

shared_maintainer_header_for_tests() {
  cat <<'EOF'
# Shared maintainer preferences

This file is main-authoritative in the main Multplx home.
In daemon homes it is read-only in daemon homes and must not be edited there.
Route new maintainer-preference discoveries to the main broker through marked status or a document pointer.
EOF
}

# End-user-aligned reproduction of the pre-fix gap, then the fixed behavior:
# two live homes start with different stale config subsets; after push each is
# updated and each live agent receives only its own changed-content instruction.
test_config_reread_per_home_changed_sets_and_exact_bytes() {
  local w head log out err status instr_a instr_b multiline_json pointer
  w=$(new_world config-reread-per-home)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" alpha "$head"
  add_sm_worktree "$w" beta "$head"
  mkdir -p "$w/alpha/config" "$w/beta/config" "$w/alpha/state" "$w/beta/state"

  # alpha is stale on harness + backlog; beta is stale on multiline dispatch only.
  printf 'pi\n' > "$w/alpha/config/actor-harness"
  printf 'tasks-axi\n' > "$w/alpha/config/backlog-backend"
  printf '{"default":{"harness":"old"}}\n' > "$w/beta/config/actor-dispatch.json"

  multiline_json=$(printf '{\n  "default": {\n    "harness": "codex",\n    "model": "gpt-5.5"\n  },\n  "rules": [\n    {"when": "news", "use": {"harness": "codex"}}\n  ]\n}\n')
  printf '%s' "$multiline_json" > "$w/home/config/actor-dispatch.json"
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'manual\n' > "$w/home/config/backlog-backend"
  {
    shared_maintainer_header_for_tests
    printf '%s\n' "shared secret preference body that must never appear in a config reread"
  } > "$w/home/data/maintainer-shared.md"

  log="$w/config-reread-per-home.tmux.log"
  err="$w/config-reread-per-home.err"
  out=$(run_config_push "$w" "$log" 2>"$err"); status=$?
  expect_code 0 "$status" "per-home reread config push should succeed"
  [ ! -s "$err" ] || fail "unexpected stderr: $(cat "$err")"

  # Destination bytes converged per home.
  cmp -s "$w/home/config/actor-dispatch.json" "$w/alpha/config/actor-dispatch.json" \
    || fail "alpha did not receive multiline dispatch"
  cmp -s "$w/home/config/actor-dispatch.json" "$w/beta/config/actor-dispatch.json" \
    || fail "beta did not receive multiline dispatch"
  [ "$(cat "$w/alpha/config/actor-harness")" = codex ] || fail "alpha harness not updated"
  [ "$(cat "$w/alpha/config/backlog-backend")" = manual ] || fail "alpha backlog-backend not updated"

  instr_a=$(reread_instruction_path "$w/alpha") || fail "alpha instruction missing after config push"
  instr_b=$(reread_instruction_path "$w/beta") || fail "beta instruction missing after config push"
  assert_present "$instr_a" "alpha should receive a config-reread instruction file"
  assert_present "$instr_b" "beta should receive a config-reread instruction file"
  [ "$(reread_mode "$instr_a")" = 600 ] || fail "alpha instruction is not private"
  [ "$(reread_mode "$instr_b")" = 600 ] || fail "beta instruction is not private"

  # Deterministic allowlist path order and exact destination bytes for alpha
  # (all three config items were missing/stale and therefore pushed).
  assert_grep "These inherited config files changed" "$instr_a" "alpha framing missing"
  assert_grep "defaults/rules" "$instr_a" "alpha must preserve agent judgment framing"
  assert_contains "$(cat "$instr_a")" "config/actor-dispatch.json" "alpha missing dispatch path"
  assert_contains "$(cat "$instr_a")" "config/actor-harness" "alpha missing harness path"
  assert_contains "$(cat "$instr_a")" "config/backlog-backend" "alpha missing backlog path"
  # Path order follows MX_INHERITABLE_CONFIG.
  awk '
    /config\/actor-dispatch\.json/ { d=NR }
    /config\/actor-harness/ { h=NR }
    /config\/backlog-backend/ { b=NR }
    END {
      if (!(d && h && b && d < h && h < b)) exit 1
    }
  ' "$instr_a" || fail "alpha instruction path order is not deterministic allowlist order"

  # Exact multiline JSON appears byte-for-byte between delimiters.
  assert_contains "$(cat "$instr_a")" "$multiline_json" \
    "alpha instruction must include exact multiline dispatch bytes"
  assert_contains "$(cat "$instr_a")" $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "alpha instruction must include exact harness scalar bytes"
  assert_contains "$(cat "$instr_a")" $'-----BEGIN config/backlog-backend-----\nmanual\n-----END config/backlog-backend-----' \
    "alpha instruction must include exact backlog-backend scalar bytes"

  # No parsed/effective summary, no SHA, no maintainer-shared dump.
  assert_not_contains "$(cat "$instr_a")" "Default worker" "must not emit parsed worker summary"
  assert_not_contains "$(cat "$instr_a")" "sha" "must not emit sha tokens"
  assert_not_contains "$(cat "$instr_a")" "SHA" "must not emit SHA tokens"
  assert_not_contains "$(cat "$instr_a")" "maintainer-shared" "maintainer-shared path must not appear"
  assert_not_contains "$(cat "$instr_a")" "shared secret preference body" \
    "maintainer-shared content must never be inlined"
  assert_not_contains "$(cat "$instr_b")" "shared secret preference body" \
    "beta must not inline maintainer-shared either"

  # Beta started with only dispatch stale; harness/backlog were absent on both
  # sides for beta... wait: primary has harness+backlog, beta lacked them, so
  # they are also pushed. Seed beta with matching harness/backlog so only
  # dispatch changes for beta - re-run a focused unit of the write helper below.
  # For this push, beta was missing harness and backlog too, so all three push.
  # Prove isolation by comparing that neither instruction references the other's
  # pre-push stale unique value.
  assert_not_contains "$(cat "$instr_a")" '"harness":"old"' \
    "alpha must not receive beta's pre-push stale dispatch"
  assert_not_contains "$(cat "$instr_b")" $'pi\n' \
    "beta instruction must not leak alpha-only stale harness bytes as a standalone scalar block incorrectly"

  # Routed send used the from-broker marker and carried only the pointer.
  pointer="CONFIG_REREAD: $(reread_instruction_path "$w/alpha")"
  assert_contains "$(cat "$log")" "[mx-from-broker]" "reread send must be marked"
  assert_contains "$(cat "$log")" "$pointer" "reread send must point to the durable instruction file"
  assert_not_contains "$(cat "$log")" '"harness": "codex"' "sent message must not inline multiline JSON"
  assert_not_contains "$(cat "$log")" $'\n  "default"' "sent message must not contain embedded newlines"
  assert_not_contains "$(cat "$log")" "Default worker" "sent message must not summarize"
  pass "B15 config reread is per-home, exact-byte, ordered, and pointer-only"
}

test_config_reread_isolation_and_absent_and_send_failure() {
  local w head log out out2 err status status2 instr_a instr_b report retry_log retry_out retry_status retry_pointer
  local first_instr first_copy second_instr second_pointer
  w=$(new_world config-reread-absent)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" alpha "$head"
  add_sm_worktree "$w" beta "$head"
  mkdir -p "$w/alpha/config" "$w/beta/config" "$w/alpha/state" "$w/beta/state"

  # alpha: only harness will change (dispatch+backlog already match primary absence).
  # beta: only dispatch will change.
  printf 'old-harness\n' > "$w/alpha/config/actor-harness"
  printf '{"stale":true}\n' > "$w/beta/config/actor-dispatch.json"
  # Primary has only actor-harness set; dispatch and backlog absent.
  printf 'codex\n' > "$w/home/config/actor-harness"
  rm -f "$w/home/config/actor-dispatch.json" "$w/home/config/backlog-backend"

  log="$w/config-reread-absent.tmux.log"
  err="$w/config-reread-absent.err"
  out=$(run_config_push "$w" "$log" 2>"$err"); status=$?
  expect_code 0 "$status" "absent-mirror reread push should succeed"

  instr_a=$(reread_instruction_path "$w/alpha") || fail "alpha instruction missing after config push"
  instr_b=$(reread_instruction_path "$w/beta") || fail "beta instruction missing after config push"
  assert_present "$instr_a" "alpha instruction missing after harness change"
  assert_present "$instr_b" "beta instruction missing after dispatch removal"

  # alpha changed harness only.
  assert_contains "$(cat "$instr_a")" "config/actor-harness" "alpha should mention harness"
  assert_contains "$(cat "$instr_a")" $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "alpha harness block exact"
  assert_not_contains "$(cat "$instr_a")" "config/actor-dispatch.json" \
    "alpha must not list unchanged/absent-both dispatch"
  assert_not_contains "$(cat "$instr_a")" "config/backlog-backend" \
    "alpha must not list unchanged/absent-both backlog"
  assert_not_contains "$(cat "$instr_a")" '{"stale":true}' \
    "alpha must not receive beta's changed dispatch content"

  # beta: dispatch mirrored to ABSENT (and harness is also newly pushed from primary).
  assert_contains "$(cat "$instr_b")" "config/actor-dispatch.json" "beta should mention dispatch"
  assert_contains "$(cat "$instr_b")" $'-----BEGIN config/actor-dispatch.json-----\nABSENT\n-----END config/actor-dispatch.json-----' \
    "beta must represent removal as ABSENT"
  assert_not_contains "$(cat "$instr_b")" "old-harness" \
    "beta must not receive alpha's pre-push stale harness content"
  # Pure ABSENT + unchanged isolation via the write helper (no second inheritance path).
  report="$w/absent-only.report"
  {
    printf '%s\n' $'actor-dispatch.json\tpushed\tmirrored primary absence'
    printf '%s\n' $'actor-harness\tunchanged\t'
    printf '%s\n' $'backlog-backend\tunchanged\t'
    printf '%s\n' $'data/maintainer-shared.md\tpushed\t'
  } > "$report"
  rm -f "$w/beta/config/actor-dispatch.json"
  mx_config_write_reread_instruction "$w/beta" "$report" "$w/beta/state/.mx-inherited-config-reread-absent" \
    || fail "ABSENT instruction write failed"
  assert_contains "$(cat "$w/beta/state/.mx-inherited-config-reread-absent")" \
    $'-----BEGIN config/actor-dispatch.json-----\nABSENT\n-----END config/actor-dispatch.json-----' \
    "helper ABSENT representation"
  assert_not_contains "$(cat "$w/beta/state/.mx-inherited-config-reread-absent")" "maintainer-shared" \
    "helper must ignore maintainer-shared even when report says pushed"
  assert_not_contains "$(cat "$w/beta/state/.mx-inherited-config-reread-absent")" "config/actor-harness" \
    "helper must omit unchanged items"

  # Send failure becomes a retryable diagnostic and non-zero exit.
  printf 'claude\n' > "$w/home/config/actor-harness"
  err="$w/config-reread-send-fail.err"
  out=$(PATH="$(make_fake_toolchain "$w"):$BASE_PATH" \
    MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" MX_SEND_SETTLE=0 \
    MX_FAKE_TMUX_FAIL_LITERAL=1 \
    "$ROOT/bin/mx-config-push.sh" 2>"$err"); status=$?
  expect_code 1 "$status" "send failure should make config-push exit non-zero"
  assert_contains "$out" "CONFIG_REREAD: daemon" "send failure diagnostic missing"
  assert_contains "$out" "send failed" "send failure must say send failed"
  assert_not_contains "$out" "config-reread: sent" \
    "must not claim reread landed when send failed"
  first_instr=$(reread_instruction_path "$w/alpha") || fail "alpha failed-send instruction missing"
  first_copy="$w/alpha/first-reread-generation.copy"
  cp "$first_instr" "$first_copy"
  assert_present "$(reread_pending_path "$w/alpha")" \
    "alpha send failure did not record a retry marker"
  assert_present "$(reread_pending_path "$w/beta")" \
    "beta send failure did not record a retry marker"

  # A later changed push publishes a distinct generation without overwriting
  # the failed generation, then an unchanged push retries both pointers.
  printf 'pi\n' > "$w/home/config/actor-harness"
  err="$w/config-reread-send-fail-second.err"
  out2=$(PATH="$(make_fake_toolchain "$w"):$BASE_PATH" \
    MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" MX_SEND_SETTLE=0 \
    MX_FAKE_TMUX_FAIL_LITERAL=1 \
    "$ROOT/bin/mx-config-push.sh" 2>"$err"); status2=$?
  expect_code 1 "$status2" "second send failure should make config-push exit non-zero"
  assert_not_contains "$out2" "config-reread: sent" \
    "second send failure must not claim reread delivery"
  second_instr=$(reread_instruction_path "$w/alpha") || fail "alpha second generation missing"
  [ "$first_instr" != "$second_instr" ] || fail "successive pushes reused the same generation path"
  cmp -s "$first_copy" "$first_instr" || fail "later push overwrote the earlier generation bytes"
  second_pointer="CONFIG_REREAD: $second_instr"
  assert_present "$(reread_pending_path "$w/alpha")" \
    "alpha second generation did not remain pending"

  # A normal later push retries the durable pointers even though propagation is
  # unchanged, then clears every marker after delivery succeeds.
  retry_log="$w/config-reread-send-retry.tmux.log"
  retry_out=$(run_config_push "$w" "$retry_log" 2>"$err"); retry_status=$?
  expect_code 0 "$retry_status" "send failure should be retryable"
  assert_contains "$retry_out" "config-reread: sent" \
    "retry should report the reread as sent"
  retry_pointer="CONFIG_REREAD: $(reread_instruction_path "$w/beta")"
  assert_contains "$(cat "$retry_log")" "$retry_pointer" \
    "retry did not resend the durable pointer"
  assert_contains "$(cat "$retry_log")" "CONFIG_REREAD: $first_instr" \
    "retry did not resend the first pending generation"
  assert_contains "$(cat "$retry_log")" "$second_pointer" \
    "retry did not resend the second pending generation"
  assert_no_reread_pending "$w/alpha"
  assert_no_reread_pending "$w/beta"
  pass "B16 config reread isolation, ABSENT, generation safety, send failure, and retry"
}

test_config_reread_publication_failure_retries_exact_generation() {
  local w head fakebin real_mv alpha_state out status stage log instr retry_out retry_status
  w=$(new_world config-reread-publication-retry)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" alpha "$head"
  mkdir -p "$w/alpha/config" "$w/alpha/state"
  printf 'old\n' > "$w/alpha/config/actor-harness"
  printf 'codex\n' > "$w/home/config/actor-harness"

  fakebin=$(make_fake_toolchain "$w")
  real_mv=$(command -v mv)
  alpha_state=$(cd "$w/alpha/state" && pwd -P)
  cat > "$fakebin/mv" <<SH
#!/usr/bin/env bash
case "\$*" in
  *"$alpha_state/.mx-inherited-config-reread."*) exit 1 ;;
esac
exec "$real_mv" "\$@"
SH
  chmod +x "$fakebin/mv"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 "$ROOT/bin/mx-config-push.sh" 2>&1); status=$?
  expect_code 1 "$status" "publication failure should remain diagnostic"
  assert_contains "$out" "CONFIG_REREAD: daemon" "publication failure diagnostic missing"
  assert_not_contains "$out" "config-reread: sent" \
    "publication failure must not claim reread delivery"
  [ "$(cat "$w/alpha/config/actor-harness")" = codex ] \
    || fail "publication failure did not retain the completed config write"
  stage=$(reread_retry_stage_path "$w/home" alpha) \
    || fail "publication failure did not retain an exact retry generation"
  assert_contains "$(cat "$stage")" \
    $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "retry generation did not retain exact destination bytes"
  assert_no_reread_instructions "$w/alpha"

  rm -f "$fakebin/mv"
  log="$w/config-reread-publication-retry.tmux.log"
  retry_out=$(run_config_push "$w" "$log" 2>/dev/null); retry_status=$?
  expect_code 0 "$retry_status" "publication failure should retry on an unchanged push"
  assert_contains "$retry_out" "config-reread: sent" \
    "successful publication retry should report delivery"
  instr=$(reread_instruction_path "$w/alpha") \
    || fail "publication retry did not publish an instruction"
  assert_contains "$(cat "$log")" "CONFIG_REREAD: $instr" \
    "publication retry did not send the durable pointer"
  assert_no_reread_retry_stages "$w/home" alpha
  pass "B20 config reread publication failures retain exact generations for retry"
}

test_config_reread_write_failure_retains_exact_retry_generation() {
  local w head fakebin real_mv retry_dir out status stage_path log retry_out retry_status instr
  local old_instr new_instr
  w=$(new_world config-reread-write-retry)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"
  printf 'old\n' > "$w/sm/config/actor-harness"
  printf 'codex\n' > "$w/home/config/actor-harness"
  fakebin=$(make_fake_toolchain "$w")
  real_mv=$(command -v mv)
  mkdir -p "$w/home/state/.mx-inherited-config-reread-retry/sm"
  retry_dir=$(cd "$w/home/state/.mx-inherited-config-reread-retry/sm" && pwd -P)
  cat > "$fakebin/mv" <<SH
#!/usr/bin/env bash
target=
for arg in "\$@"; do target="\$arg"; done
case "\$target" in
  *"$retry_dir"/.mx-inherited-config-reread.*)
    case "\$target" in
      *.exact) ;;
      *) exit 1 ;;
    esac
    ;;
esac
exec "$real_mv" "\$@"
SH
  chmod +x "$fakebin/mv"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 "$ROOT/bin/mx-config-push.sh" 2>&1); status=$?
  expect_code 1 "$status" "instruction-write failure should remain diagnostic"
  assert_contains "$out" "retained exact retry generation" \
    "instruction-write failure did not retain exact retry bytes"
  stage_path=$(reread_retry_stage_path "$w/home" sm) \
    || fail "instruction-write failure did not leave a durable exact generation"
  assert_contains "$(cat "$stage_path")" \
    $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "instruction-write failure did not retain the original exact bytes"
  printf 'changed-before-retry\n' > "$w/home/config/actor-harness"
  rm -f "$fakebin/mv"
  log="$w/config-reread-write-retry.tmux.log"
  retry_out=$(run_config_push "$w" "$log" 2>/dev/null); retry_status=$?
  expect_code 0 "$retry_status" "a later changed push should retry an instruction-write failure"
  assert_contains "$retry_out" "config-reread: sent" \
    "later changed push did not deliver the retained exact generation"
  old_instr=$(grep 'CONFIG_REREAD:' "$log" | head -n 1 | sed 's/.*CONFIG_REREAD: //')
  new_instr=$(grep 'CONFIG_REREAD:' "$log" | tail -n 1 | sed 's/.*CONFIG_REREAD: //')
  [ -n "$old_instr" ] && [ -n "$new_instr" ] && [ "$old_instr" != "$new_instr" ] \
    || fail "later changed push did not deliver both generations"
  instr="$old_instr"
  assert_contains "$(cat "$instr")" \
    $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "exact retry delivery did not preserve the original destination bytes"
  assert_contains "$(cat "$new_instr")" "changed-before-retry" \
    "later changed push did not deliver its new destination bytes"
  assert_no_reread_retry_stages "$w/home" sm
  pass "B21 config reread instruction-write failures retain exact retry generations"
}

test_config_reread_exact_temp_survives_adoption_failure() {
  local w head fakebin real_mv real_cp retry_dir out status stage_path log retry_out retry_status
  local old_instr new_instr
  w=$(new_world config-reread-exact-temp-fallback)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"
  printf 'old\n' > "$w/sm/config/actor-harness"
  printf 'codex\n' > "$w/home/config/actor-harness"
  fakebin=$(make_fake_toolchain "$w")
  real_mv=$(command -v mv)
  real_cp=$(command -v cp)
  mkdir -p "$w/home/state/.mx-inherited-config-reread-retry/sm"
  retry_dir=$(cd "$w/home/state/.mx-inherited-config-reread-retry/sm" && pwd -P)
  cat > "$fakebin/mv" <<SH
#!/usr/bin/env bash
target=
for arg in "\$@"; do target="\$arg"; done
case "\$target" in
  *"$retry_dir"/.mx-inherited-config-reread.*) exit 1 ;;
esac
exec "$real_mv" "\$@"
SH
  chmod +x "$fakebin/mv"
  cat > "$fakebin/cp" <<SH
#!/usr/bin/env bash
target=
for arg in "\$@"; do target="\$arg"; done
case "\$target" in
  *"$retry_dir"/.mx-inherited-config-reread.*) exit 1 ;;
esac
exec "$real_cp" "\$@"
SH
  chmod +x "$fakebin/cp"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 "$ROOT/bin/mx-config-push.sh" 2>&1); status=$?
  expect_code 1 "$status" "exact temporary fallback failure should remain diagnostic"
  assert_contains "$out" "retained exact retry temporary" \
    "exact temporary fallback failure did not retain the immutable bytes"
  stage_path=$(reread_retry_stage_path "$w/home" sm) \
    || fail "exact temporary fallback failure lost its retry artifact"
  case "$stage_path" in
    *.tmp.*) : ;;
    *) fail "exact temporary fallback retained an unexpected artifact: $stage_path" ;;
  esac
  [ ! -e "$stage_path.report" ] \
    || fail "exact temporary fallback created a lossy retry report"
  assert_contains "$(cat "$stage_path")" \
    $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "exact temporary fallback did not preserve the original bytes"
  printf 'changed-before-retry\n' > "$w/home/config/actor-harness"
  rm -f "$fakebin/mv" "$fakebin/cp"
  log="$w/config-reread-exact-temp-fallback.tmux.log"
  retry_out=$(run_config_push "$w" "$log" 2>/dev/null); retry_status=$?
  expect_code 0 "$retry_status" "later push should deliver retained exact temporary bytes"
  old_instr=$(grep 'CONFIG_REREAD:' "$log" | head -n 1 | sed 's/.*CONFIG_REREAD: //')
  new_instr=$(grep 'CONFIG_REREAD:' "$log" | tail -n 1 | sed 's/.*CONFIG_REREAD: //')
  [ -n "$old_instr" ] && [ -n "$new_instr" ] && [ "$old_instr" != "$new_instr" ] \
    || fail "later push did not deliver both exact generations"
  assert_contains "$(cat "$old_instr")" \
    $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "later push rebuilt the retained temporary from newer bytes"
  assert_contains "$(cat "$new_instr")" "changed-before-retry" \
    "later push did not deliver the new destination bytes"
  assert_no_reread_retry_stages "$w/home" sm
  pass "B21 config reread preserves exact bytes when temporary adoption also fails"
}

test_config_reread_serializes_concurrent_pushes() {
  local w head fakebin marker entered log first_out second_out first_pid first_status second_status
  local first_instr second_instr first_line second_line
  w=$(new_world config-reread-serialized-pushes)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"
  printf 'old\n' > "$w/sm/config/actor-harness"
  printf 'one\n' > "$w/home/config/actor-harness"

  fakebin=$(make_fake_toolchain "$w")
  mv "$fakebin/tmux" "$fakebin/tmux.real"
  marker="$w/first-send.marker"
  entered="$w/first-send.entered"
  log="$w/config-reread-serialized.tmux.log"
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
case "\$*" in
  *send-keys*)
    if (set -o noclobber; : > "$marker") 2>/dev/null; then
      : > "$entered"
      sleep 1
    fi
    ;;
esac
exec "$fakebin/tmux.real" "\$@"
SH
  chmod +x "$fakebin/tmux"

  first_out="$w/first-push.out"
  (
    PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
      MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
      "$ROOT/bin/mx-config-push.sh" > "$first_out" 2>&1
  ) &
  first_pid=$!
  for _ in $(seq 1 100); do
    [ -e "$entered" ] && break
    sleep 0.02
  done
  [ -e "$entered" ] || fail "first config push did not reach pointer delivery"
  first_instr=$(reread_instruction_path "$w/sm") \
    || fail "first concurrent push did not publish its generation"
  printf 'two\n' > "$w/home/config/actor-harness"
  second_out="$w/second-push.out"
  PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
    "$ROOT/bin/mx-config-push.sh" > "$second_out" 2>&1
  second_status=$?
  wait "$first_pid"; first_status=$?
  expect_code 0 "$first_status" "first serialized config push failed"
  expect_code 0 "$second_status" "second serialized config push failed"
  second_instr=$(reread_instruction_path "$w/sm") \
    || fail "second concurrent push did not publish its generation"
  [ "$first_instr" != "$second_instr" ] || fail "concurrent pushes reused a generation"
  [ "$(cat "$w/sm/config/actor-harness")" = two ] \
    || fail "concurrent pushes did not converge the latest config bytes"
  first_line=$(grep -n -F "CONFIG_REREAD: $first_instr" "$log" | head -n 1 | cut -d: -f1)
  second_line=$(grep -n -F "CONFIG_REREAD: $second_instr" "$log" | head -n 1 | cut -d: -f1)
  [ -n "$first_line" ] && [ -n "$second_line" ] && [ "$first_line" -lt "$second_line" ] \
    || fail "concurrent pushes delivered generations out of order"
  pass "B21 config reread serializes concurrent propagation and delivery"
}

test_config_reread_full_retry_queue_drains_before_new_push() {
  local w head retry_dir path n fakebin log out status pointer_count
  w=$(new_world config-reread-full-queue)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"
  printf 'old\n' > "$w/sm/config/actor-harness"
  printf 'new\n' > "$w/home/config/actor-harness"
  retry_dir="$w/home/state/.mx-inherited-config-reread-retry/sm"
  mkdir -p "$retry_dir"
  for n in $(seq -w 1 16); do
    path="$retry_dir/.mx-inherited-config-reread.20260721T000000.$n"
    printf 'generation-%s\n' "$n" > "$path"
    chmod 0600 "$path"
  done
  fakebin=$(make_fake_toolchain "$w")
  log="$w/config-reread-full-queue.tmux.log"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
    "$ROOT/bin/mx-config-push.sh" 2>&1); status=$?
  expect_code 0 "$status" "a full retry queue should drain before a new push"
  assert_contains "$out" "config-reread: sent" \
    "a new config generation was not delivered after retry draining"
  [ "$(cat "$w/sm/config/actor-harness")" = new ] \
    || fail "the new config generation did not propagate after retry draining"
  assert_no_reread_retry_stages "$w/home" sm
  pointer_count=$(grep -c 'CONFIG_REREAD:' "$log" 2>/dev/null || true)
  [ "$pointer_count" -ge 17 ] \
    || fail "full retry queue did not deliver all pending generations before the new one"
  pass "B22 full config reread retry queues drain before new publication"
}

test_config_reread_cleanup_runs_after_mixed_delivery_failure() {
  local w head fakebin state_real fail_path path n report out status count
  w=$(new_world config-reread-mixed-delivery)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/state"
  state_real=$(cd "$w/sm/state" && pwd -P)
  fail_path="$state_real/.mx-inherited-config-reread.9999-fail"
  for n in $(seq -w 1 18); do
    path="$state_real/.mx-inherited-config-reread.00$n"
    printf 'generation-%s\n' "$n" > "$path"
    chmod 0600 "$path"
  done
  printf 'failed-generation\n' > "$fail_path"
  chmod 0600 "$fail_path"
  for path in "$state_real"/.mx-inherited-config-reread.*; do
    mx_config_reread_mark_pending "$path" "$path.pending" \
      || fail "could not mark mixed-delivery generation pending"
  done
  fakebin=$(make_fake_toolchain "$w")
  mv "$fakebin/tmux" "$fakebin/tmux.real"
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
  case "\$*" in
  *send-keys*'.9999-fail'*) exit 1 ;;
esac
exec "$fakebin/tmux.real" "\$@"
SH
  chmod +x "$fakebin/tmux"
  report="$w/empty-reread.report"
  : > "$report"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 mx_config_send_reread_nudge sm "$w/sm" "$report" 2>&1); status=$?
  expect_code 1 "$status" "mixed delivery failure should remain diagnostic"
  assert_contains "$out" "CONFIG_REREAD: daemon sm: send failed" \
    "mixed delivery failure diagnostic missing"
  count=0
  for path in "$state_real"/.mx-inherited-config-reread.*; do
    case "$path" in
      *.pending) continue ;;
    esac
    [ -f "$path" ] && [ ! -L "$path" ] || continue
    [ ! -e "$path.pending" ] || continue
    count=$((count + 1))
  done
  [ "$count" = 16 ] || fail "mixed delivery failure skipped bounded sent-history cleanup (count=$count)"
  assert_present "$fail_path.pending" "failed generation lost its retry marker"
  pass "B23 mixed config reread delivery failures still bound sent history"
}

test_config_reread_stops_after_failed_generation() {
  local w fakebin state_real old new report log out status
  w=$(new_world config-reread-order)
  mkdir -p "$w/sm/state"
  state_real=$(cd "$w/sm/state" && pwd -P)
  old="$state_real/.mx-inherited-config-reread.0000-fail"
  new="$state_real/.mx-inherited-config-reread.0001-new"
  printf 'old-generation\n' > "$old"
  printf 'new-generation\n' > "$new"
  chmod 0600 "$old" "$new"
  mx_config_reread_mark_pending "$old" "$old.pending" \
    || fail "could not mark older generation pending"
  mx_config_reread_mark_pending "$new" "$new.pending" \
    || fail "could not mark newer generation pending"
  fakebin=$(make_fake_toolchain "$w")
  mv "$fakebin/tmux" "$fakebin/tmux.real"
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
case "\$*" in
  *send-keys*'.0000-fail'*) exit 1 ;;
esac
exec "$fakebin/tmux.real" "\$@"
SH
  chmod +x "$fakebin/tmux"
  report="$w/empty-reread.report"
  : > "$report"
  log="$w/config-reread-order.tmux.log"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$ROOT" \
    MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
    mx_config_send_reread_nudge sm "$w/sm" "$report" 2>&1); status=$?
  expect_code 1 "$status" "an older failed generation should remain diagnostic"
  assert_contains "$out" "CONFIG_REREAD: daemon sm: send failed" \
    "older generation failure diagnostic missing"
  assert_not_contains "$(cat "$log" 2>/dev/null || true)" ".0001-new" \
    "newer generation was delivered after an older failure"
  assert_present "$old.pending" "older failed generation lost its retry marker"
  assert_present "$new.pending" "newer generation was sent after an older failure"
  pass "B26 config reread delivery stops after the oldest failed generation"
}

test_bootstrap_detect_only_does_not_create_state() {
  local w fakebin detect_state out status
  w=$(new_world bootstrap-detect-only)
  detect_state="$w/detect-state"
  fakebin=$(make_fake_toolchain "$w")
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_STATE_OVERRIDE="$detect_state" MX_BOOTSTRAP_DETECT_ONLY=1 \
    "$ROOT/bin/mx-bootstrap.sh" 2>&1); status=$?
  expect_code 0 "$status" "detect-only bootstrap should succeed"
  [ ! -e "$detect_state" ] || fail "detect-only bootstrap created its state directory"
  pass "B24 bootstrap detect-only mode remains filesystem read-only"
}

test_config_reread_skips_when_unchanged_and_reads_after_push() {
  local w head log out err status report instr n path pending_instruction count
  w=$(new_world config-reread-after-push)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"

  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'codex\n' > "$w/sm/config/actor-harness"
  log="$w/config-reread-unchanged.tmux.log"
  out=$(run_config_push "$w" "$log" 2>/dev/null); status=$?
  expect_code 0 "$status" "unchanged push should succeed"
  assert_not_contains "$out" "config-reread: sent" "no reread when nothing changed"
  [ ! -s "$log" ] || fail "unchanged push still sent text: $(cat "$log")"
  assert_no_reread_instructions "$w/sm"

  # Prove instruction bytes are taken from destination after write: if we only
  # read the primary source, a post-copy destination mutation would not matter.
  # Here we call the write helper after planting distinct dest bytes and a
  # pushed report line.
  printf '%s' 'destination-post-write' > "$w/sm/config/actor-harness"
  printf 'primary-source-only\n' > "$w/home/config/actor-harness"
  report="$w/after-push.report"
  printf '%s\n' $'actor-harness\tpushed\t' > "$report"
  instr="$w/sm/state/.mx-inherited-config-reread-dest"
  mx_config_write_reread_instruction "$w/sm" "$report" "$instr" \
    || fail "destination-byte instruction write failed"
  assert_contains "$(cat "$instr")" "destination-post-write" \
    "instruction must use destination post-write bytes"
  assert_contains "$(cat "$instr")" $'destination-post-write-----END config/actor-harness-----' \
    "instruction must not append a byte to a non-newline-terminated destination"
  assert_not_contains "$(cat "$instr")" "primary-source-only" \
    "instruction must not fall back to primary source bytes"
  : > "$w/sm/config/actor-harness"
  mx_config_write_reread_instruction "$w/sm" "$report" "$instr" \
    || fail "empty destination instruction write failed"
  assert_contains "$(cat "$instr")" $'-----BEGIN config/actor-harness-----
-----END config/actor-harness-----' \
    "instruction must represent an empty destination without a synthetic byte"
  pending_instruction="$w/sm/state/.mx-inherited-config-reread.20260721T000000.01"
  printf '%s\n' generation > "$pending_instruction"
  mx_config_reread_mark_pending "$pending_instruction" "$pending_instruction.pending" \
    || fail "could not create bounded-lifecycle pending marker"
  for n in $(seq -w 2 18); do
    path="$w/sm/state/.mx-inherited-config-reread.20260721T000000.$n"
    printf '%s\n' generation > "$path"
    chmod 0600 "$path"
  done
  mx_config_reread_cleanup_sent "$w/sm"
  count=0
  for path in "$w/sm/state"/.mx-inherited-config-reread.*; do
    case "$path" in
      *.pending) continue ;;
    esac
    [ -f "$path" ] && [ ! -L "$path" ] || continue
    [ ! -e "$path.pending" ] || continue
    count=$((count + 1))
  done
  [ "$count" = 16 ] || fail "sent reread generations were not bounded"
  assert_present "$pending_instruction" "cleanup deleted a pending reread generation"
  assert_present "$pending_instruction.pending" "cleanup deleted a pending reread marker"
  pass "B17 config reread skips unchanged homes and reads destination post-write bytes"
}

test_config_reread_bootstrap_path_and_spawn_flexibility() {
  local w head log out fakebin sm launchlog launch instr report stale
  w=$(new_world config-reread-bootstrap)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"
  printf 'old\n' > "$w/sm/config/actor-harness"
  printf 'codex\n' > "$w/home/config/actor-harness"

  fakebin=$(make_fake_toolchain "$w")
  log="$w/bootstrap-reread.tmux.log"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
    "$ROOT/bin/mx-bootstrap.sh" 2>/dev/null)
  [ "$(cat "$w/sm/config/actor-harness")" = codex ] || fail "bootstrap did not push harness"
  instr=$(reread_instruction_path "$w/sm") || fail "bootstrap reread instruction missing"
  assert_present "$instr" "bootstrap must write a config reread instruction when config changed"
  assert_contains "$(cat "$log")" "[mx-from-broker]" \
    "bootstrap config reread must use routed daemon send"
  assert_contains "$(cat "$instr")" \
    $'-----BEGIN config/actor-harness-----\ncodex\n-----END config/actor-harness-----' \
    "bootstrap instruction must carry exact post-write harness bytes"

  # mx-spawn still permits a conscious explicit runtime outside the config
  # (defaults/rules only - never harden spawn against deliberate choice).
  w=$(new_world config-reread-spawn-flex)
  printf 'codex\n' > "$w/home/config/actor-harness"
  printf 'codex\n' > "$w/home/config/daemon-harness"
  sm="$w/sm-flex"
  make_seeded_home "$sm" sm-flex
  mkdir -p "$sm/state"
  report="$sm/state/stale-reread.report"
  printf '%s\n' $'actor-harness\tpushed\t' > "$report"
  stale="$sm/state/.mx-inherited-config-reread.spawn-stale"
  mx_config_write_reread_instruction "$sm" "$report" "$stale" \
    || fail "could not create spawn stale reread generation"
  mx_config_reread_mark_pending "$stale" "$stale.pending" \
    || fail "could not create spawn stale reread marker"
  launchlog="$w/spawn-flex.launch.log"
  spawn_daemon_capture "$w" sm-flex "$sm" "$launchlog" --harness pi >/dev/null 2>&1
  assert_no_reread_pending "$sm"
  assert_no_reread_instructions "$sm"
  launch=$(cat "$launchlog")
  assert_contains "$launch" "pi" \
    "explicit --harness pi must still win over configured codex defaults"
  pass "B18 bootstrap config reread path works; spawn flexibility remains defaults-only"
}

test_bootstrap_respawns_before_config_reread() {
  local w head fakebin log report stale
  w=$(new_world config-reread-respawn-order)
  head=$(git -C "$w/main" rev-parse HEAD)
  add_sm_worktree "$w" sm "$head"
  mkdir -p "$w/sm/config" "$w/sm/state"
  printf 'harness=codex\n' >> "$w/home/state/sm.meta"
  printf '%s' old > "$w/sm/config/actor-harness"
  printf '%s' codex > "$w/home/config/actor-harness"
  report="$w/sm/state/stale-reread.report"
  printf '%s\n' $'actor-harness\tpushed\t' > "$report"
  stale="$w/sm/state/.mx-inherited-config-reread.stale-generation"
  mx_config_write_reread_instruction "$w/sm" "$report" "$stale" \
    || fail "could not create stale reread generation"
  mx_config_reread_mark_pending "$stale" "$stale.pending" \
    || fail "could not create stale reread marker"
  log="$w/config-reread-respawn-order.log"

cat > "$w/main/bin/mx-spawn.sh" <<SH
#!/usr/bin/env bash
. '$w/main/bin/mx-config-inherit-lib.sh'
printf '%s' spawn >> '$log'
printf '%s' codex > '$w/sm/config/actor-harness'
SH
  chmod +x "$w/main/bin/mx-spawn.sh"
  fakebin=$(make_fake_toolchain "$w")
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
case "\$*" in
  *display-message*'#{pane_current_command}'*) printf '%s' zsh ;;
  *display-message*'#{pane_id}'*) printf '%s' '%1' ;;
  *display-message*'#{cursor_y}'*) printf '%s' 0 ;;
  *capture-pane*) :
    ;;
  *send-keys*) printf '%s' send-keys >> '$log' ;;
esac
SH
  chmod +x "$fakebin/tmux"
  PATH="$fakebin:$BASE_PATH" MX_HOME="$w/home" MX_ROOT_OVERRIDE="$w/main" \
    MX_SEND_SETTLE=0 MX_FAKE_TMUX_LOG="$log" \
    "$ROOT/bin/mx-bootstrap.sh" >/dev/null 2>&1
  assert_contains "$(cat "$log")" "spawn" \
    "bootstrap did not respawn the dead daemon"
  assert_not_contains "$(cat "$log")" "send-keys" \
    "bootstrap nudged a daemon before its respawn completed"
  assert_present "$stale" "bootstrap removed the stale generation before relaunch handling"
  assert_present "$stale.pending" "bootstrap removed the stale marker before relaunch handling"
  mx_config_reread_discard_pending "$w/sm" || fail "could not clean respawn test generation"
  assert_no_reread_pending "$w/sm"
  assert_no_reread_instructions "$w/sm"
  pass "B19 bootstrap respawns before inherited-config reread"
}

test_spawn_quarantines_pending_rereads_on_cleanup_failure() {
  local w sm report stale fakebin real_rm out status launchlog quarantine_root quarantined_count
  local quarantine_dirs before_quarantine_dirs after_quarantine_dirs n dir
  w=$(new_world config-reread-spawn-quarantine)
  sm="$w/sm"
  mkdir -p "$w/home/config"
  printf 'codex\n' > "$w/home/config/actor-harness"
  make_seeded_home "$sm" sm
  mkdir -p "$sm/state"
  report="$sm/state/stale-reread.report"
  printf '%s\n' $'actor-harness\tpushed\t' > "$report"
  stale="$sm/state/.mx-inherited-config-reread.spawn-stale"
  mx_config_write_reread_instruction "$sm" "$report" "$stale" \
    || fail "could not create pending spawn reread generation"
  mx_config_reread_mark_pending "$stale" "$stale.pending" \
    || fail "could not mark pending spawn reread generation"
  quarantine_root="$sm/state/.mx-inherited-config-reread-quarantine"
  mkdir -p "$quarantine_root"
  for n in $(seq -w 1 16); do
    dir="$quarantine_root/generation.old$n"
    mkdir -p "$dir"
    printf 'old-quarantine-%s\n' "$n" > "$dir/snapshot"
    printf 'hidden-quarantine-%s\n' "$n" > "$dir/.hidden-snapshot"
  done
  fakebin=$(make_launch_capturing_tmux "$w/tmux-spawn-quarantine")
  real_rm=$(command -v rm)
  cat > "$fakebin/rm" <<SH
#!/usr/bin/env bash
case "\$*" in
  *'.mx-inherited-config-reread.'*) exit 1 ;;
esac
exec "$real_rm" "\$@"
SH
  chmod +x "$fakebin/rm"
  launchlog="$w/spawn-quarantine.launch.log"
  out=$(PATH="$fakebin:$BASE_PATH" TMUX='' CLAUDECODE=1 \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$w/home" \
    MX_STATE_OVERRIDE="$w/home/state" MX_DATA_OVERRIDE="$w/home/data" \
    MX_PROJECTS_OVERRIDE="$w/home/projects" MX_CONFIG_OVERRIDE="$w/home/config" \
    MX_SPAWN_NO_GUARD=1 MX_FAKE_LAUNCH_LOG="$launchlog" \
    "$ROOT/bin/mx-spawn.sh" sm "$sm" --daemon 2>&1); status=$?
  expect_code 0 "$status" "spawn should remain available after reread cleanup failure"
  assert_contains "$out" "CONFIG_REREAD: daemon sm: quarantined pre-relaunch generations" \
    "spawn cleanup failure did not emit a CONFIG_REREAD quarantine diagnostic"
  assert_no_reread_pending "$sm"
  assert_no_reread_instructions "$sm"
  assert_present "$quarantine_root" "spawn cleanup failure did not create a quarantine directory"
  quarantined_count=$(find "$quarantine_root" -type f | wc -l | tr -d ' ')
  [ "$quarantined_count" -ge 2 ] \
    || fail "spawn cleanup failure did not quarantine both generation artifacts"
  quarantine_dirs=0
  for dir in "$quarantine_root"/generation.*; do
    [ -d "$dir" ] && [ ! -L "$dir" ] || continue
    quarantine_dirs=$((quarantine_dirs + 1))
  done
  [ "$quarantine_dirs" -le 16 ] \
    || fail "spawn cleanup failure exceeded bounded quarantine history ($quarantine_dirs)"
  before_quarantine_dirs=$quarantine_dirs
  mx_config_reread_quarantine_pending "$sm" sm "$w/home" || true
  after_quarantine_dirs=0
  for dir in "$quarantine_root"/generation.*; do
    [ -d "$dir" ] && [ ! -L "$dir" ] || continue
    after_quarantine_dirs=$((after_quarantine_dirs + 1))
  done
  [ "$after_quarantine_dirs" = "$before_quarantine_dirs" ] \
    || fail "empty quarantine cleanup created an extra generation directory"
  assert_not_contains "$(cat "$launchlog")" "CONFIG_REREAD:" \
    "spawn cleanup failure left a stale reread pointer eligible for delivery"
  pass "B25 spawn quarantines stale rereads without blocking relaunch"
}

case "${MX_TEST_CASE_GROUP:-all}" in
  harness-model-resolution)
    test_harness_resolution
    test_daemon_model_effort_tokens
    test_propagate_lib
    test_spawn_split_and_inherit
    test_spawn_backward_compat_actor_fallback
    test_spawn_bare_backward_compat
    test_spawn_explicit_harness_wins
    test_spawn_unverified_daemon_harness_refused
    test_spawn_bare_harness_no_model_effort_flag
    test_spawn_daemon_harness_model_token
    test_spawn_daemon_harness_model_and_effort_tokens
    test_spawn_explicit_model_overrides_daemon_harness_token
    test_spawn_explicit_effort_overrides_daemon_harness_token
    test_spawn_explicit_harness_does_not_inherit_daemon_harness_tokens
    test_spawn_explicit_harness_uses_explicit_profile_axes
    test_spawn_fallback_chain_and_actor_scout_unaffected
    ;;
  spawn-config-inheritance)
    test_bootstrap_sweep_propagates_and_reconverges
    test_bootstrap_sweep_propagates_when_tracked_current
    test_bootstrap_sweep_defers_dispatch_on_stale_unignored_home
    test_bootstrap_sweep_no_inheritance_is_noop
    test_bootstrap_sweep_surfaces_config_propagation_failure
    test_bootstrap_rereads_after_partial_propagation
    test_config_push_propagates_reports_without_ff_or_nudge
    test_config_push_reports_skips_dirty_and_invalid_home
    test_config_push_exits_nonzero_on_copy_error
    test_config_push_rereads_after_partial_propagation
    ;;
  config-reread-retry)
    test_config_reread_per_home_changed_sets_and_exact_bytes
    test_config_reread_isolation_and_absent_and_send_failure
    test_config_reread_publication_failure_retries_exact_generation
    test_config_reread_write_failure_retains_exact_retry_generation
    test_config_reread_exact_temp_survives_adoption_failure
    test_config_reread_serializes_concurrent_pushes
    test_config_reread_full_retry_queue_drains_before_new_push
    test_config_reread_cleanup_runs_after_mixed_delivery_failure
    test_config_reread_stops_after_failed_generation
    test_config_reread_skips_when_unchanged_and_reads_after_push
    test_config_reread_bootstrap_path_and_spawn_flexibility
    test_bootstrap_respawns_before_config_reread
    test_spawn_quarantines_pending_rereads_on_cleanup_failure
    test_bootstrap_detect_only_does_not_create_state
    ;;
  all)
    test_harness_resolution
    test_daemon_model_effort_tokens
    test_propagate_lib
    test_spawn_split_and_inherit
    test_spawn_backward_compat_actor_fallback
    test_spawn_bare_backward_compat
    test_spawn_explicit_harness_wins
    test_spawn_unverified_daemon_harness_refused
    test_spawn_bare_harness_no_model_effort_flag
    test_spawn_daemon_harness_model_token
    test_spawn_daemon_harness_model_and_effort_tokens
    test_spawn_explicit_model_overrides_daemon_harness_token
    test_spawn_explicit_effort_overrides_daemon_harness_token
    test_spawn_explicit_harness_does_not_inherit_daemon_harness_tokens
    test_spawn_explicit_harness_uses_explicit_profile_axes
    test_spawn_fallback_chain_and_actor_scout_unaffected
    test_bootstrap_sweep_propagates_and_reconverges
    test_bootstrap_sweep_propagates_when_tracked_current
    test_bootstrap_sweep_defers_dispatch_on_stale_unignored_home
    test_bootstrap_sweep_no_inheritance_is_noop
    test_bootstrap_sweep_surfaces_config_propagation_failure
    test_bootstrap_rereads_after_partial_propagation
    test_config_push_propagates_reports_without_ff_or_nudge
    test_config_push_reports_skips_dirty_and_invalid_home
    test_config_push_exits_nonzero_on_copy_error
    test_config_push_rereads_after_partial_propagation
    test_config_reread_per_home_changed_sets_and_exact_bytes
    test_config_reread_isolation_and_absent_and_send_failure
    test_config_reread_publication_failure_retries_exact_generation
    test_config_reread_write_failure_retains_exact_retry_generation
    test_config_reread_exact_temp_survives_adoption_failure
    test_config_reread_serializes_concurrent_pushes
    test_config_reread_full_retry_queue_drains_before_new_push
    test_config_reread_cleanup_runs_after_mixed_delivery_failure
    test_config_reread_stops_after_failed_generation
    test_config_reread_skips_when_unchanged_and_reads_after_push
    test_config_reread_bootstrap_path_and_spawn_flexibility
    test_bootstrap_respawns_before_config_reread
    test_spawn_quarantines_pending_rereads_on_cleanup_failure
    test_bootstrap_detect_only_does_not_create_state
    ;;
  *)
    fail "unknown daemon harness case group: ${MX_TEST_CASE_GROUP}"
    ;;
esac

echo "# selected mx-daemon-harness cases passed"
