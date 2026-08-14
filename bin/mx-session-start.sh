#!/usr/bin/env bash
# Stable executable adapter for the Rust-owned ordered session-start digest.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
MX_RUST_SOURCE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
rust_bin=$(mx_rust_runtime_bin) || exit $?
exec "$rust_bin" session mx-session-start.sh "$@"
