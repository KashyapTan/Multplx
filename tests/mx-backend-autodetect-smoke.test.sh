#!/usr/bin/env bash
# tests/mx-backend-autodetect-smoke.test.sh - real herdr smoke test for runtime
# backend AUTO-DETECTION (bin/mx-backend.sh's mx_backend_detect, wired into
# mx_backend_name between config/backend and the tmux default).
#
# Unlike tests/mx-backend-herdr.test.sh (fake herdr CLI) and
# tests/mx-backend-herdr-smoke.test.sh (real herdr, adapter primitives called
# directly), this suite drives the REAL bin/mx-spawn.sh and bin/mx-teardown.sh
# end to end, because auto-detection is a mx-spawn-TIME decision, not an
# adapter primitive - it has to be proven where mx_backend_name is actually
# called. The real spawn runs in a helper-provisioned, per-run named Herdr lab
# session, with a scratch MX_HOME and scratch local-only project. Concurrent
# copies therefore never share the default session or a workspace namespace.
#
# The complementary "tmux nested inside herdr resolves to tmux, silently" case
# is covered as a fast, deterministic fake-tmux mx-spawn.sh test in
# tests/mx-backend.test.sh (test_spawn_autodetect_nesting_resolves_tmux_silently).
# Reproducing a genuinely nested real-tmux-inside-real-herdr pane here would
# need a live attached tmux client, which a background test script cannot
# manufacture; the selection LOGIC for that case is already exercised for real
# by mx_backend_detect's own unit coverage plus that fake-tmux mx-spawn test.
#
# Safety (2026-07-02 incident): every test-owned Herdr operation goes through
# bin/mx-herdr-lab.sh, which appends the named session flag and verifies the
# default system session is unchanged after teardown. Never replace the helper
# with an ambient HERDR_SESSION-only command.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail() { printf 'not ok - %s\n' "$1" >&2; exit 1; }
pass() { printf 'ok - %s\n' "$1"; }
assert_contains_local() {  # <haystack> <needle> <msg>
  case "$1" in
    *"$2"*) : ;;
    *) fail "$3"$'\n'"--- got ---"$'\n'"$1" ;;
  esac
}

command -v herdr >/dev/null 2>&1 || { echo "skip: herdr not found"; exit 0; }
command -v jq >/dev/null 2>&1 || { echo "skip: jq not found (required by the herdr adapter)"; exit 0; }
command -v treehouse >/dev/null 2>&1 || { echo "skip: treehouse not found (required by mx-spawn.sh)"; exit 0; }

# shellcheck source=tests/herdr-test-safety.sh
. "$ROOT/tests/herdr-test-safety.sh"

# TMP_ROOT is physically resolved (mktemp -d "$(pwd -P)"-relative) to keep this
# real-herdr smoke fixture free of unrelated OS symlink noise.
# The old mx-spawn bug that originally motivated this fixture shape was fixed in
# mx-spawn-symlink-guard-s8: mx-spawn.sh now normalizes PROJ_ABS and observed
# backend cwd reads before the worktree-discovery comparison.
# The dedicated regression is
# tests/mx-backend.test.sh:test_spawn_symlinked_project_prefix_avoids_false_refusal.
TMP_ROOT=$(mktemp -d "$(cd "${TMPDIR:-/tmp}" && pwd -P)/mx-backend-autodetect-smoke.XXXXXX")
HERDR_LAB_HELPER="$ROOT/bin/mx-herdr-lab.sh"
HERDR_LAB_SESSION=$("$HERDR_LAB_HELPER" name mx-autodetect-smoke-concurrency-h3) || {
  rm -rf "$TMP_ROOT"
  fail "could not generate an isolated Herdr lab session name"
}
export HERDR_SESSION="$HERDR_LAB_SESSION"
ID="autodetectsmoke1"
WT=
cleanup_all() {
  local cleanup_status=0
  [ -n "$WT" ] && command -v treehouse >/dev/null 2>&1 && treehouse return --force "$WT" >/dev/null 2>&1
  "$HERDR_LAB_HELPER" teardown "$HERDR_LAB_SESSION" || cleanup_status=$?
  rm -rf "$TMP_ROOT"
  return "$cleanup_status"
}
on_exit() {
  local status=$?
  cleanup_all || status=$?
  trap - EXIT
  exit "$status"
}
trap on_exit EXIT
"$HERDR_LAB_HELPER" provision "$HERDR_LAB_SESSION" || fail "could not provision isolated Herdr lab session"

# --- scratch world: MX_HOME with NO backend config, one throwaway project ---

STATE="$TMP_ROOT/state"; DATA="$TMP_ROOT/data"; CONFIG="$TMP_ROOT/config"
mkdir -p "$STATE" "$DATA/$ID" "$CONFIG"
printf 'trivial autodetect-smoke brief: nothing to do.\n' > "$DATA/$ID/brief.md"

PROJ="$TMP_ROOT/scratch-project"
mkdir -p "$PROJ"
git -C "$PROJ" init -q
printf '# scratch\n' > "$PROJ/README.md"
git -C "$PROJ" add README.md
git -C "$PROJ" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm initial

# --- spawn with NO explicit backend config; HERDR_ENV=1 is the only marker --

OUT_FILE="$TMP_ROOT/spawn.out"; ERR_FILE="$TMP_ROOT/spawn.err"
env -u TMUX -u MX_BACKEND PATH="$PATH" HERDR_ENV=1 \
  MX_ROOT_OVERRIDE="$ROOT" MX_STATE_OVERRIDE="$STATE" MX_DATA_OVERRIDE="$DATA" \
  MX_CONFIG_OVERRIDE="$CONFIG" MX_PROJECTS_OVERRIDE="$TMP_ROOT/unused-projects" \
  MX_SPAWN_NO_GUARD=1 \
  "$ROOT/bin/mx-spawn.sh" "$ID" "$PROJ" "sh -c 'echo autodetect-smoke-ok'" \
  >"$OUT_FILE" 2>"$ERR_FILE"
status=$?
[ "$status" -eq 0 ] || fail "mx-spawn.sh did not succeed auto-detecting herdr"$'\n'"--- stdout ---"$'\n'"$(cat "$OUT_FILE")"$'\n'"--- stderr ---"$'\n'"$(cat "$ERR_FILE")"

assert_contains_local "$(cat "$ERR_FILE")" "NOTICE" \
  "mx-spawn.sh did not print the auto-detect notice to stderr when selecting herdr"
assert_contains_local "$(cat "$ERR_FILE")" "EXPERIMENTAL herdr backend" \
  "mx-spawn.sh's auto-detect notice did not flag herdr as experimental"
pass "real herdr: mx-spawn.sh auto-detects herdr from HERDR_ENV=1 (no explicit config) and prints the loud notice"

META="$STATE/$ID.meta"
[ -f "$META" ] || fail "mx-spawn.sh did not write a meta file for $ID"
assert_contains_local "$(cat "$META")" "backend=herdr" \
  "auto-detected spawn did not record backend=herdr in meta"
assert_contains_local "$(cat "$META")" "herdr_session=$HERDR_LAB_SESSION" \
  "auto-detected spawn did not record the isolated herdr_session in meta"

WORKSPACE=$(grep '^herdr_workspace_id=' "$META" | cut -d= -f2-)
[ -n "$WORKSPACE" ] || fail "auto-detected spawn meta is missing herdr_workspace_id"

TAB=$(grep '^herdr_tab_id=' "$META" | cut -d= -f2-)
[ -n "$TAB" ] || fail "auto-detected spawn meta is missing herdr_tab_id"

WT=$(grep '^worktree=' "$META" | cut -d= -f2-)
if [ -z "$WT" ] || [ ! -d "$WT" ]; then
  fail "auto-detected spawn did not report a real worktree path"
fi

PANE=$(grep '^herdr_pane_id=' "$META" | cut -d= -f2-)
[ -n "$PANE" ] || fail "auto-detected spawn meta is missing herdr_pane_id"
pass "real herdr: auto-detected spawn records backend=herdr and herdr_session/workspace/tab/pane fields in meta"

# --- confirm the trivial launch command actually ran in the herdr pane ------

sleep 1
CAPTURED=$("$HERDR_LAB_HELPER" run "$HERDR_LAB_SESSION" pane read "$PANE" --source recent --lines 200) || \
  fail "capture failed on the auto-detected herdr pane"
CAPTURED=$(printf '%s\n' "$CAPTURED" | tail -n 30)
case "$CAPTURED" in
  *autodetect-smoke-ok*) : ;;
  *) fail "the raw launch command did not run in the auto-detected herdr pane"$'\n'"$CAPTURED" ;;
esac
pass "real herdr: the auto-detected spawn's launch command actually ran in the herdr pane"

# --- teardown completes the trivial spawn/teardown cycle --------------------

TEARDOWN_OUT="$TMP_ROOT/teardown.out"
MX_ROOT_OVERRIDE="$ROOT" MX_STATE_OVERRIDE="$STATE" MX_DATA_OVERRIDE="$DATA" \
  MX_CONFIG_OVERRIDE="$CONFIG" \
  "$ROOT/bin/mx-teardown.sh" "$ID" >"$TEARDOWN_OUT" 2>&1
status=$?
[ "$status" -eq 0 ] || fail "mx-teardown.sh failed for the auto-detected herdr task"$'\n'"$(cat "$TEARDOWN_OUT")"
[ -f "$META" ] && fail "mx-teardown.sh did not remove $META"
if "$HERDR_LAB_HELPER" run "$HERDR_LAB_SESSION" pane get "$PANE" >/dev/null 2>&1; then
  fail "mx-teardown.sh did not close the auto-detected herdr pane"
fi
WT=
pass "real herdr: teardown completes the auto-detected spawn/teardown cycle (meta cleared, pane closed)"

if ! cleanup_all; then
  trap - EXIT
  fail "isolated Herdr lab teardown failed or the default system session changed"
fi
trap - EXIT
pass "real herdr: isolated lab session removed and default system session unchanged"
