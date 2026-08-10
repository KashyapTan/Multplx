#!/usr/bin/env bash
# Acquire or inspect the per-home broker session lock.
# Writes the harness (agent) process PID found by walking the shell's ancestry,
# which lives as long as the broker session - unlike the transient subshell
# PID of any one tool call, which is dead moments after it is written.
# Usage: mx-lock.sh           acquire; exit 1 unless ownership is verified
#        mx-lock.sh status    print holder and liveness; always exits 0
#        mx-lock.sh --terminate-owner <request-id>
#          consume one exact session.terminate-owner grant, TERM the verified
#          competing harness, prove exit, then acquire the ordinary lock
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
LOCK="$STATE/.lock"
mkdir -p "$STATE" 2>/dev/null || {
  echo "error: cannot create session-lock state directory $STATE; operate read-only until resolved" >&2
  exit 1
}

# Harness identity (MX_HARNESS_RE, ancestry walk, holder liveness) is owned by
# the shared session-lock lib so the Claude Stop auto-arm applies the exact
# same identity contract.
# shellcheck source=bin/mx-session-lock-lib.sh
. "$SCRIPT_DIR/mx-session-lock-lib.sh"
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$SCRIPT_DIR/mx-maintainer-override-lib.sh"

TERMINATE_OVERRIDE=
case "${1:-}" in
  '') ;;
  status)
    [ "$#" -eq 1 ] || { echo "error: status accepts no arguments" >&2; exit 2; }
    ;;
  --terminate-owner)
    if [ "$#" -ne 2 ] || ! mx_override_slug_valid "${2:-}"; then
      echo "error: --terminate-owner requires one valid request id" >&2
      exit 2
    fi
    TERMINATE_OVERRIDE=$2
    ;;
  --force)
    echo "REFUSED: --force cannot bypass session ownership; use one exact session.terminate-owner grant." >&2
    exit 2
    ;;
  *) echo "error: usage: mx-lock.sh [status|--terminate-owner <request-id>]" >&2; exit 2 ;;
esac

if [ "${1:-}" = "status" ]; then
  if [ ! -f "$LOCK" ]; then echo "lock: free"; exit 0; fi
  old=$(cat "$LOCK" 2>/dev/null) || {
    echo "lock: unreadable"
    exit 0
  }
  if mx_harness_pid_alive "$old"; then echo "lock: held by live harness pid $old"; else echo "lock: stale (pid $old dead or not a harness)"; fi
  exit 0
fi

me=$(mx_harness_ancestry_pid) || { echo "error: cannot locate harness process in ancestry" >&2; exit 1; }
probe=$(mktemp "$STATE/.lock-write.XXXXXX" 2>/dev/null) || {
  echo "error: cannot write session lock; operate read-only until resolved" >&2
  exit 1
}
rm -f "$probe" 2>/dev/null || {
  echo "error: cannot clean session-lock publication probe; operate read-only until resolved" >&2
  exit 1
}
# shellcheck source=bin/mx-wake-lib.sh
. "$SCRIPT_DIR/mx-wake-lib.sh"
CLAIM_LOCK="$STATE/.lock.acquire"
CLAIM_LOCK_HELD=0
TERMINATE_CONSUMED=0
TERMINATE_ACTION_COMPLETED=0
release_claim_lock() {
  if [ "$CLAIM_LOCK_HELD" -eq 1 ]; then
    mx_lock_release "$CLAIM_LOCK"
    CLAIM_LOCK_HELD=0
  fi
}
lock_exit() {
  local status=$?
  trap - EXIT
  release_claim_lock
  if [ "$TERMINATE_CONSUMED" -eq 1 ]; then
    if [ "$status" -eq 0 ] && [ "$TERMINATE_ACTION_COMPLETED" -eq 1 ]; then
      outcome=succeeded
    else
      outcome=failed
    fi
    MX_STATE_OVERRIDE="$STATE" mx_override_result "$TERMINATE_OVERRIDE" "$outcome" \
      "terminate-owner lock acquisition exited with status $status" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap lock_exit EXIT
trap 'exit 1' HUP INT TERM
mx_lock_acquire_wait "$CLAIM_LOCK"
CLAIM_LOCK_HELD=1

if [ -e "$LOCK" ] || [ -L "$LOCK" ]; then
  if [ ! -f "$LOCK" ] || [ -L "$LOCK" ]; then
    echo "error: session lock is not a regular file; operate read-only until resolved" >&2
    exit 1
  fi
  old=$(cat "$LOCK" 2>/dev/null) || {
    echo "error: session lock is unreadable; operate read-only until resolved" >&2
    exit 1
  }
  if [ "$old" != "$me" ] && mx_harness_pid_alive "$old"; then
    if [ -z "$TERMINATE_OVERRIDE" ]; then
      echo "error: another live broker session holds the lock (pid $old); operate read-only or request an exact session.terminate-owner grant" >&2
      exit 1
    fi
    bindings=$(MX_ROOT_OVERRIDE="$MX_ROOT" MX_HOME="$MX_HOME" MX_STATE_OVERRIDE="$STATE" \
      "$SCRIPT_DIR/mx-override-bindings.sh" terminate-owner "$old") || exit 1
    operation=$(printf '%s' "$bindings" | jq -r '.operation')
    target=$(printf '%s' "$bindings" | jq -r '.target')
    digest=$(printf '%s' "$bindings" | jq -r '.expected_state_digest')
    MX_STATE_OVERRIDE="$STATE" mx_override_consume "$TERMINATE_OVERRIDE" \
      session.terminate-owner broker-session multplx "$operation" "$target" "$digest" >/dev/null || exit 1
    TERMINATE_CONSUMED=1
    kill -TERM "$old" 2>/dev/null || {
      echo "error: verified harness pid $old could not be terminated" >&2
      exit 1
    }
    terminated=0
    for _ in $(seq 1 50); do
      if ! mx_harness_pid_alive "$old"; then terminated=1; break; fi
      sleep 0.1
    done
    [ "$terminated" -eq 1 ] || {
      echo "error: verified harness pid $old did not exit after TERM; lock remains owned" >&2
      exit 1
    }
  fi
fi
if [ -n "$TERMINATE_OVERRIDE" ] && [ "$TERMINATE_CONSUMED" -eq 0 ]; then
  echo "error: exact terminate-owner grant was supplied but no different live owner matched it" >&2
  exit 1
fi
if ! { printf '%s\n' "$me" > "$LOCK"; } 2>/dev/null; then
  echo "error: cannot write session lock; operate read-only until resolved" >&2
  exit 1
fi
written=$(cat "$LOCK" 2>/dev/null) || {
  echo "error: cannot verify session lock ownership; operate read-only until resolved" >&2
  exit 1
}
if [ ! -f "$LOCK" ] || [ -L "$LOCK" ] || [ "$written" != "$me" ]; then
  echo "error: session lock ownership verification failed; operate read-only until resolved" >&2
  exit 1
fi
release_claim_lock
TERMINATE_ACTION_COMPLETED=1
echo "lock acquired: harness pid $me"
