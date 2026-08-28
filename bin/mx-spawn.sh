#!/usr/bin/env bash
# Transport-only adapter for native task and daemon spawn.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
export MX_RUST_SOURCE_ROOT=$ROOT
rust_bin=$(mx_rust_runtime_bin) || exit $?
exec "$rust_bin" spawn "$@"
