#!/usr/bin/env bash
# Shared Rust runtime selection for ported entry points.

mx_local_state_implementation() {
  case "${MX_LOCAL_STATE_IMPLEMENTATION:-rust}" in
    rust|legacy) printf '%s\n' "${MX_LOCAL_STATE_IMPLEMENTATION:-rust}" ;;
    *)
      echo "error: MX_LOCAL_STATE_IMPLEMENTATION must be rust or legacy" >&2
      return 2
      ;;
  esac
}

mx_rust_runtime_bin() {
  local script_dir root candidate
  script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  root=${MX_ROOT_OVERRIDE:-$(cd "$script_dir/.." && pwd)}
  if [ -n "${MX_RUST_BIN:-}" ]; then
    candidate=$MX_RUST_BIN
  else
    candidate="$root/target/release/mx"
  fi
  if [ ! -x "$candidate" ]; then
    echo "error: Multplx Rust runtime is unavailable at $candidate" >&2
    echo "       run cargo build --workspace --release or select MX_LOCAL_STATE_IMPLEMENTATION=legacy" >&2
    return 1
  fi
  printf '%s\n' "$candidate"
}
