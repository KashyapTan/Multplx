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

# Portion 04 remains shadow-only until every supported backend is Rust-native.
# Selection is process-wide and happens before a backend operation starts.
mx_backend_implementation() {
  case "${MX_BACKEND_IMPLEMENTATION:-legacy}" in
    rust|legacy) printf '%s\n' "${MX_BACKEND_IMPLEMENTATION:-legacy}" ;;
    *)
      echo "error: MX_BACKEND_IMPLEMENTATION must be rust or legacy" >&2
      return 2
      ;;
  esac
}

mx_backend_shadow_meta_get() {  # <meta-file> <key>
  local meta=$1 key=$2
  [ -f "$meta" ] || return 0
  grep "^$key=" "$meta" 2>/dev/null | tail -1 | cut -d= -f2- || true
}

mx_backend_shadow_meta_for_window() {  # <target> <state-dir>
  local target=$1 state=$2 meta window
  for meta in "$state"/*.meta; do
    [ -e "$meta" ] || continue
    window=$(mx_backend_shadow_meta_get "$meta" window)
    [ -n "$window" ] && [ "$window" = "$target" ] || continue
    printf '%s' "$meta"
    return 0
  done
  return 1
}

# Preflight only for a Rust shadow caller that must leave recorded Herdr/cmux
# tasks on their legacy adapters. It never invokes a backend and validates task
# ids before joining them to the state path; tmux resolution remains in Rust.
mx_backend_compatibility_backend_of_selector() {  # <raw-target> <state-dir> [assign]
  local raw=$1 state=$2 id='' meta='' backend=''
  case "$raw" in
    *:*) meta=$(mx_backend_shadow_meta_for_window "$raw" "$state" 2>/dev/null || true) ;;
    *)
      case "$raw" in
        ''|.*|*[!A-Za-z0-9._-]*) ;;
        *)
          [ "${#raw}" -le 64 ] && [ -f "$state/$raw.meta" ] && meta="$state/$raw.meta"
          if [ -z "$meta" ]; then
            case "$raw" in
              mx-*)
                id=${raw#mx-}
                case "$id" in
                  ''|.*|*[!A-Za-z0-9._-]*) ;;
                  *) [ "${#id}" -le 64 ] && [ -f "$state/$id.meta" ] && meta="$state/$id.meta" ;;
                esac
                ;;
            esac
          fi
          [ -n "$meta" ] || meta=$(mx_backend_shadow_meta_for_window "$raw" "$state" 2>/dev/null || true)
          ;;
      esac
      ;;
  esac
  if [ -n "$meta" ]; then
    backend=$(mx_backend_shadow_meta_get "$meta" backend)
  fi
  MX_BACKEND_COMPATIBILITY_SELECTED=${backend:-tmux}
  [ "${3:-}" = assign ] || printf '%s' "$MX_BACKEND_COMPATIBILITY_SELECTED"
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
