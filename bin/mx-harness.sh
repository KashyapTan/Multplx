#!/usr/bin/env bash
# Compatibility transport for the Rust harness resolver.
# Usage: mx-harness.sh [subagent|actor|standing-agent|persistent-subagent|daemon]
# Model/effort selectors and --help are documented by the Rust owner.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-harness: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" harness "$@"
