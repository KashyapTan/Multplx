#!/usr/bin/env bash
# Print the tail of an actor endpoint (bounded, for cheap diagnosis).
# Usage: mx-peek.sh <target> [lines=40]
#   <target> may be an exact task id, a legacy mx-<id> task label resolved
#   through this home's state/<id>.meta, or an explicit backend target.
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

RAW_TARGET=$1
N=${2:-40}

# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
MX_PEEK_BACKEND_IMPLEMENTATION=$(mx_backend_implementation) || exit $?

# The Rust path owns tmux selector validation and resolution end to end.
# Enter it before the legacy facade is loaded or can invoke tmux.
if [ "$MX_PEEK_BACKEND_IMPLEMENTATION" = rust ]; then
  mx_backend_compatibility_backend_of_selector "$RAW_TARGET" "$STATE" assign
  COMPATIBILITY_BACKEND=$MX_BACKEND_COMPATIBILITY_SELECTED
  case "$COMPATIBILITY_BACKEND" in
    herdr|cmux) ;;
    *)
      "$SCRIPT_DIR/mx-guard.sh" || true
      rust_bin=$(mx_rust_runtime_bin) || exit $?
      exec "$rust_bin" peek "$RAW_TARGET" "$N"
      ;;
  esac
fi

# shellcheck source=bin/mx-backend.sh
. "$SCRIPT_DIR/mx-backend.sh"

"$SCRIPT_DIR/mx-guard.sh" || true

T=$(mx_backend_resolve_selector "$RAW_TARGET" "$STATE")
BACKEND=$(mx_backend_of_selector "$RAW_TARGET" "$T" "$STATE")
EXPECTED_LABEL=$(mx_backend_expected_label_of_selector "$RAW_TARGET" "$STATE")

mx_backend_capture "$BACKEND" "$T" "$N" "$EXPECTED_LABEL"
