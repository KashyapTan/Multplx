#!/usr/bin/env bash
# Compatibility transport for native task and daemon retirement.
# Native teardown owns the `rm -f` removal path for its append-only timeline artifact.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-teardown: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" teardown "$@"
