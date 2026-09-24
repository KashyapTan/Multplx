#!/usr/bin/env bash
# Stable executable adapter for the Rust-owned canonical system snapshot.
set -u
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$SCRIPT_DIR/mx-rust-runtime.sh"
MX_RUST_SOURCE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
rust_bin=$(mx_rust_runtime_bin) || exit $?
MX_MULTICALL_EXPLICIT=1 exec "$rust_bin" session mx-system-snapshot.sh "$@"
