#!/usr/bin/env bash
# Stable fail-open adapter for the Rust-owned session-start nudge.
set -u
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P) || exit 0
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P) || exit 0
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || exit 0
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" session mx-sessionstart-nudge.sh "$@"
