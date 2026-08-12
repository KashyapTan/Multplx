#!/usr/bin/env bash
# Turn-end guard for any broker PRIMARY session: the main home OR a
# daemon's own home. A daemon runs its own primary broker session and
# is guarded exactly like the main primary; only child actor/scout worktrees are
# exempt (see the scoping block below and docs/turnend-guard.md).
#
# mx-guard.sh (bin/mx-guard.sh) is pull-based: it only warns when some other
# supervision script happens to run. A primary session that ends a turn without
# resuming its harness supervision protocol, and then never runs another
# system-touching command itself, can sit blind for hours.
# This script is push-based: verified harness turn-end hooks invoke it every time
# the primary is about to end a turn.
# Claude and codex can block directly by preserving exit status 2 and stderr.
# The pi adapter uses the same predicate and forces one bounded
# follow-up because its turn-end event is passive.
# See docs/turnend-guard.md for the per-harness mechanics, validation evidence,
# and fail-open tradeoffs.
#
# Delivers with TRACKED harness hook files at the repo root, so this file is
# checked out into every worktree of this repo: the primary checkout, every
# daemon home (treehouse-leased or git-cloned), and any actor/scout task
# worktree spawned to work on Multplx itself (the recursive "broker
# improving itself" case). A daemon home runs its OWN primary broker
# session, so it must be guarded like the main primary; only child actor/scout
# worktrees are exempt. It must therefore scope itself at runtime to a real
# primary checkout - the main home or a genuinely marked daemon home - and
# stay a silent, fast no-op inside child task worktrees.
#
# Loop-guard, codex (default) mode: never block twice in the same turn. Codex
# Stop payloads carry stop_hook_active=true when the CURRENT stop attempt was
# itself already forced by an earlier block this turn; on that signal we always
# allow the stop, whether or not watcher supervision actually got resumed.
# Passive harness adapters provide their own one-follow-up guard before calling
# this script.
# That bounds those harnesses to at most one forced continuation per turn -
# never a wedged, un-endable session - while still nagging again on a later turn
# if the problem persists.
#
# Loop-guard, --claude mode (Stop-owned auto-arm cooperation): Claude Code
# marks EVERY stop after ANY stop-hook-driven continuation stop_hook_active=true,
# including turns started by the asyncRewake auto-arm, so the one-shot allow
# would re-open the exact blind window this guard exists to close
# (docs/turnend-guard.md records the 2026-07-21 incident). In --claude mode this
# guard ignores stop_hook_active and instead cooperates with the Stop-owned
# auto-arm (bin/mx-claude-stop-autoarm.sh), which fires on the same Stop event:
#   1. a live identity-matched watcher with a fresh beacon allows immediately;
#   2. otherwise wait briefly (MX_CLAUDE_AUTOARM_SYNC_WAIT_MS, default 800ms)
#      for the auto-arm to claim this home (state/.claude-autoarm.lock owner
#      alive) or to record a fresh rewake outcome (state/.claude-autoarm-epoch)
#      for this event epoch - either proof allows without consuming a
#      continuation, so one event epoch yields exactly one recovery turn;
#   3. only when neither materializes is the auto-arm genuinely absent: re-block
#      with the repair banner, bounded to MX_CLAUDE_TURNEND_BLOCK_BUDGET
#      (default 3) consecutive blocks per session - safely below Claude Code's
#      hard 8-consecutive-block override - then allow degraded with a visible
#      systemMessage so the session can always end.
# Any allow resets the consecutive-block budget.
set -u

# Portion 08 Rust-default adapter. Keep the body below as the explicit bounded
# rollback path and as the sourced-function ABI where this file is sourceable.
MX_SUPERVISION_ADAPTER_DIR=${BASH_SOURCE[0]%/*}
[ "$MX_SUPERVISION_ADAPTER_DIR" != "${BASH_SOURCE[0]}" ] || MX_SUPERVISION_ADAPTER_DIR=.
MX_SUPERVISION_ADAPTER_DIR="$(CDPATH='' cd -- "$MX_SUPERVISION_ADAPTER_DIR" 2>/dev/null && pwd)" || exit 1
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  # shellcheck source=bin/mx-rust-runtime.sh
  . "$MX_SUPERVISION_ADAPTER_DIR/mx-rust-runtime.sh"
  mx_supervision_adapter_implementation=$(mx_supervision_implementation) || exit $?
  if [ "$mx_supervision_adapter_implementation" = rust ]; then
    MX_RUST_SOURCE_ROOT="$(cd "$MX_SUPERVISION_ADAPTER_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
    mx_supervision_adapter_bin=$(mx_rust_runtime_bin) || exit $?
    exec "$mx_supervision_adapter_bin" supervision mx-turnend-guard.sh "$@"
  fi
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
GRACE=${MX_GUARD_GRACE:-300}
WATCH="$SCRIPT_DIR/mx-watch.sh"
CLAUDE_MODE=0
SYNC_WAIT_MS=${MX_CLAUDE_AUTOARM_SYNC_WAIT_MS:-800}
EPOCH_FRESH=${MX_CLAUDE_AUTOARM_EPOCH_FRESH:-15}
BLOCK_BUDGET=${MX_CLAUDE_TURNEND_BLOCK_BUDGET:-3}
case "$SYNC_WAIT_MS" in ''|*[!0-9]*) SYNC_WAIT_MS=800 ;; esac
case "$EPOCH_FRESH" in ''|*[!0-9]*|0) EPOCH_FRESH=15 ;; esac
case "$BLOCK_BUDGET" in ''|*[!0-9]*|0) BLOCK_BUDGET=3 ;; esac

for arg in "$@"; do
  case "$arg" in
    --claude) CLAUDE_MODE=1 ;;
    *) echo "usage: $(basename "$0") [--claude]" >&2; exit 2 ;;
  esac
done

# shellcheck source=bin/mx-supervision-lib.sh
. "$SCRIPT_DIR/mx-supervision-lib.sh"
# shellcheck source=bin/mx-primary-scope-lib.sh
. "$SCRIPT_DIR/mx-primary-scope-lib.sh"

# Read the whole turn-end hook payload once; never block on unreadable/absent
# stdin.
PAYLOAD=$(cat 2>/dev/null || true)
[ -n "$PAYLOAD" ] || exit 0

# jq is the repo's established JSON dependency (missing jq degrades to a
# silent no-op). Without it we cannot safely read the
# loop-guard field, so we must never block - fail open, not noisy.
command -v jq >/dev/null 2>&1 || exit 0

STOP_HOOK_ACTIVE=$(printf '%s' "$PAYLOAD" | jq -r '.stop_hook_active // false' 2>/dev/null) || exit 0
if [ "$CLAUDE_MODE" -eq 0 ] && [ "$STOP_HOOK_ACTIVE" = "true" ]; then
  exit 0
fi

# --- scope precisely to a PRIMARY checkout ----------------------------------
# A genuinely-marked daemon home runs its OWN primary broker session, so
# force-INCLUDE it as a guarded primary whether treehouse leased it as a linked
# worktree (git-dir != git-common-dir) or it is a git-cloned plain checkout. This
# mirrors the cd-guard's intent that a daemon's own session is a guarded
# primary. Only an UNMARKED checkout (or one with an invalid marker) falls
# through to the linked-worktree exemption: broker hands out actor/scout
# task worktrees as genuine linked `git worktree`s (bin/mx-spawn.sh aborts
# otherwise), whose git-dir lives under the parent repo's .git/worktrees/<name>
# and differs from the common (shared) git-dir, while a main, non-worktree
# checkout has the two equal. Child worktrees never carry the gitignored marker,
# so this exempts them while guarding every real daemon home.
mx_primary_scope_matches "$MX_ROOT" "$STATE" || exit 0

# --- the actual predicate ----------------------------------------------------
# shellcheck source=bin/mx-wake-lib.sh
. "$SCRIPT_DIR/mx-wake-lib.sh"

BUDGET_FILE="$STATE/.turnend-claude-blocks"
budget_reset() {
  [ "$CLAUDE_MODE" -eq 1 ] || return 0
  rm -f "$BUDGET_FILE" 2>/dev/null || true
}

mx_supervision_status "$STATE" "$GRACE"
if [ "$CLAUDE_MODE" -eq 1 ]; then
  if [ "$MX_SUP_NEEDED" = false ]; then
    budget_reset
    exit 0
  fi
else
  if [ "$MX_SUP_IN_FLIGHT" -eq 0 ]; then
    budget_reset
    exit 0
  fi
fi
if mx_watcher_healthy "$STATE" "$WATCH" "$GRACE" "$MX_HOME"; then
  budget_reset
  exit 0
fi

block_stop() {
  local afk reason rule
  afk=0
  [ -e "$STATE/.afk" ] && afk=1
  reason=$("$SCRIPT_DIR/mx-supervision-instructions.sh" --afk "$afk" --repair-line 2>/dev/null \
    || printf '%s\n' 'tasks in flight, no live watcher - repair missing watcher supervision according to the session-start operating block before ending the turn')
  rule='━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━'
  {
    printf '●%s\n' "$rule"
    printf '●  TURN WOULD END BLIND - SUPERVISION IS OFF\n'
    printf '●  %s task(s) in flight, but no live watcher holds this home lock (last beat: %s).\n' "$MX_SUP_IN_FLIGHT" "$MX_SUP_BEACON_DESC"
    if [ "$CLAUDE_MODE" -eq 1 ]; then
      printf '●  The Stop-owned auto-arm did not claim this home either, so recovery is NOT already under way.\n'
    fi
    printf '●  %s\n' "$reason"
    printf '●%s\n' "$rule"
  } >&2
  exit 2
}

if [ "$CLAUDE_MODE" -eq 0 ]; then
  block_stop
fi

# --- --claude cooperative path -----------------------------------------------
# The Stop-owned auto-arm fires on the same Stop event. Give it a brief bounded
# window to prove it owns recovery for this event epoch before consuming one of
# Claude's bounded continuations.
autoarm_owns_recovery() {
  local pid outcome age
  mx_watcher_healthy "$STATE" "$WATCH" "$GRACE" "$MX_HOME" && return 0
  pid=$(cat "$STATE/.claude-autoarm.lock/pid" 2>/dev/null || true)
  mx_pid_alive "$pid" && return 0
  outcome=$(sed -n 's/^.*outcome=\([a-z][a-z]*\) .*$/\1/p' "$STATE/.claude-autoarm-epoch" 2>/dev/null || true)
  if [ "$outcome" = rewake ]; then
    age=$(mx_path_age "$STATE/.claude-autoarm-epoch")
    [ "$age" -lt "$EPOCH_FRESH" ] && return 0
  fi
  return 1
}

i=0
while [ "$i" -lt $((SYNC_WAIT_MS / 100)) ]; do
  if autoarm_owns_recovery; then
    budget_reset
    exit 0
  fi
  sleep 0.1
  i=$((i + 1))
done
if autoarm_owns_recovery; then
  budget_reset
  exit 0
fi

# The auto-arm genuinely failed to establish: re-block, but never past the
# budget so the session can always end and Claude's 8-block override is never
# approached.
SESSION_ID=$(printf '%s' "$PAYLOAD" | jq -r '.session_id // "unknown"' 2>/dev/null || printf 'unknown')
COUNT=0
if [ -f "$BUDGET_FILE" ]; then
  old_session=$(sed -n '1s/^session=//p' "$BUDGET_FILE" 2>/dev/null || true)
  old_count=$(sed -n '2s/^count=//p' "$BUDGET_FILE" 2>/dev/null || true)
  case "$old_count" in
    ''|*[!0-9]*) old_count=0 ;;
  esac
  [ "$old_session" = "$SESSION_ID" ] && COUNT=$old_count
fi
COUNT=$((COUNT + 1))
if [ "$COUNT" -gt "$BLOCK_BUDGET" ]; then
  budget_reset
  NEED_DESC="$MX_SUP_IN_FLIGHT task(s) in flight"
  printf '{"systemMessage":"broker turn-end guard: %s with no live watcher and no Stop auto-arm claim; block budget exhausted, allowing this stop. Repair supervision (bin/mx-watch-arm.sh as a Claude Code background task) or investigate why bin/mx-claude-stop-autoarm.sh is not claiming this home."}\n' "$NEED_DESC"
  exit 0
fi
printf 'session=%s\ncount=%s\n' "$SESSION_ID" "$COUNT" > "$BUDGET_FILE" 2>/dev/null || true
block_stop
