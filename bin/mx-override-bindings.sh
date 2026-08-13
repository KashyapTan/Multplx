#!/usr/bin/env bash
set -eu
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
. "$SCRIPT_DIR/mx-rust-runtime.sh"
mx_authority_implementation >/dev/null || exit $?
MX_RUST_SOURCE_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd -P)
export MX_RUST_SOURCE_ROOT
rust_bin=$(mx_rust_runtime_bin) || exit $?
exec "$rust_bin" authority mx-override-bindings.sh "$@"
