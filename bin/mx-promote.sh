#!/usr/bin/env bash
# Promote a scout task to a delivery task in place: the actor keeps its window,
# worktree, and loaded context; only the contract changes. Flips kind= to delivery in
# state/<task-id>.meta so mx-teardown.sh applies the full delivery-task teardown protection
# again. After promoting, send the actor its delivery instructions via mx-send.sh
# (inventory scratch state, reset to a clean default-branch base, carry over only
# intended fix changes, create branch mx/<task-id>, implement, then report done
# according to the project's delivery mode).
# Usage: mx-promote.sh <task-id>
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
"$MX_ROOT/bin/mx-guard.sh" || true
ID=$1
META="$STATE/$ID.meta"
[ -f "$META" ] || { echo "error: no meta for task $ID at $META" >&2; exit 1; }
grep -qx 'kind=scout' "$META" || { echo "error: task $ID is not a scout task (kind=scout not in meta)" >&2; exit 1; }

TMP="$META.tmp"
grep -v '^kind=' "$META" > "$TMP"
echo "kind=delivery" >> "$TMP"
mv "$TMP" "$META"

HOME_Q=$(printf '%q' "$MX_HOME")
echo "promoted $ID to delivery (teardown protection restored)"
echo "next: MX_HOME=$HOME_Q bin/mx-send.sh mx-$ID '<delivery instructions: review scratch state with git status and git log; reset to a clean default-branch base; carry over only intended fix changes; create branch mx/$ID; implement; report done>'"
