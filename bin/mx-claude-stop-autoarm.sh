#!/usr/bin/env bash
# Stable executable adapter for the Rust-owned Claude Stop watcher continuity hook.
set -eu
SCRIPT_DIR=${BASH_SOURCE[0]%/*}
[ "$SCRIPT_DIR" != "${BASH_SOURCE[0]}" ] || SCRIPT_DIR=.
SCRIPT_DIR=$(CDPATH='' cd -- "$SCRIPT_DIR" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-claude-stop-autoarm: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" supervision mx-claude-stop-autoarm.sh "$@"
