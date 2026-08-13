#!/usr/bin/env bash
set -eu
case "${MX_AUTHORITY_IMPLEMENTATION:-rust}" in rust|legacy) ;; *) printf 'error: MX_AUTHORITY_IMPLEMENTATION must be rust or legacy\n' >&2; exit 2 ;; esac
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
BINARY=${MX_RUST_BIN:-$ROOT/target/release/mx}
[ -x "$BINARY" ] || { printf 'mx-maintainer-override: Rust release binary is unavailable at %s\n' "$BINARY" >&2; exit 1; }
export MX_RUST_SOURCE_ROOT=$ROOT
exec "$BINARY" authority mx-maintainer-override.sh "$@"
