#!/usr/bin/env bash
# Compatibility transport for the Rust launcher installer.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'multplx: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_LAUNCHER_DEFAULT_ROOT=$ROOT MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" launcher-install "$@"
