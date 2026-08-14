#!/usr/bin/env bash
# Compatibility transport to the Rust upstream-review command.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "${BASH_SOURCE[0]%/*}" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-upstream-diff: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT MX_MULTICALL_EXPLICIT=1
exec "$BINARY" upstream-diff "$@"
