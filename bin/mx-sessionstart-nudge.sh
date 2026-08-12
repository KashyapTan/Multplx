#!/usr/bin/env bash
# Print the one-line session-start instruction only for a genuine broker
# primary whose current harness session has not already acquired the home lock.
# Every silence and error path exits 0 because Claude SessionStart exit 2 blocks
# session initialization.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Portion 09 Rust-default adapter. All nudge paths remain fail-open and exit 0.
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_session_implementation) || exit 0
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit 0
  exec "$rust_bin" session mx-sessionstart-nudge.sh "$@"
fi
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-gate-refuse-lib.sh
. "$SCRIPT_DIR/mx-gate-refuse-lib.sh"
# shellcheck source=bin/mx-primary-scope-lib.sh
. "$SCRIPT_DIR/mx-primary-scope-lib.sh"
# shellcheck source=bin/mx-operational-input.sh
. "$SCRIPT_DIR/mx-operational-input.sh"

mx_is_gate_agent && exit 0
mx_primary_scope_matches "$MX_ROOT" "$STATE" || exit 0

lock_is_in_ancestry() {
  local lock_pid pid=$$ _
  [ -f "$STATE/.lock" ] || return 1
  IFS= read -r lock_pid < "$STATE/.lock" 2>/dev/null || return 1
  case "$lock_pid" in
    ''|*[!0-9]*|1) return 1 ;;
  esac
  kill -0 "$lock_pid" 2>/dev/null || return 1
  for _ in 1 2 3 4 5 6 7 8; do
    [ "$pid" = "$lock_pid" ] && return 0
    pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
    [ -n "$pid" ] && [ "$pid" -gt 1 ] || return 1
  done
  return 1
}

lock_is_in_ancestry && exit 0
nudge=
mx_operational_input_encode session-start \
  "Run \`bin/mx-session-start.sh\` now, exactly once, before executing any other instructions." \
  nudge || exit 0
printf '%s\n' "$nudge"
exit 0
