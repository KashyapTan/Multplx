#!/usr/bin/env bash
# Shared Rust runtime selection for ported entry points.

mx_rust_runtime_bin() {
  local script_dir root candidate
  script_dir=${BASH_SOURCE[0]%/*}
  [ "$script_dir" != "${BASH_SOURCE[0]}" ] || script_dir=.
  script_dir=$(CDPATH='' cd -- "$script_dir" 2>/dev/null && pwd) || return 1
  root=${MX_RUST_SOURCE_ROOT:-${MX_ROOT_OVERRIDE:-$(cd "$script_dir/.." && pwd)}}
  if [ -n "${MX_RUST_BIN:-}" ]; then
    candidate=$MX_RUST_BIN
  else
    candidate="$root/target/release/mx"
  fi
  if [ ! -x "$candidate" ]; then
    echo "error: Multplx Rust runtime is unavailable at $candidate" >&2
    echo "       run cargo build --release --workspace --locked or reinstall Multplx" >&2
    return 1
  fi
  printf '%s\n' "$candidate"
}
