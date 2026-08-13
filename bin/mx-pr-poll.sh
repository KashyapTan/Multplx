#!/usr/bin/env bash
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=${MX_RUST_SOURCE_ROOT:-${MX_ROOT_OVERRIDE:-${MX_HOME:-$(cd "$SCRIPT_DIR/.." && pwd -P)}}}
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-pr-poll: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT MX_PR_POLL_CHECK_PATH=$0
exec "$BINARY" review mx-pr-poll.sh "$@"
