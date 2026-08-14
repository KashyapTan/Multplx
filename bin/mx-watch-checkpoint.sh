#!/usr/bin/env bash
# Compatibility transport for the Rust bounded watcher checkpoint.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-watch-checkpoint: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" supervision mx-watch-checkpoint.sh "$@"
