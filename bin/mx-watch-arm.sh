#!/usr/bin/env bash
# Stable executable adapter for the Rust-owned watcher lifecycle transaction.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-watch-arm: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" supervision mx-watch-arm.sh "$@"
