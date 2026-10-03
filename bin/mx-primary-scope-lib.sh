#!/usr/bin/env bash
# Read-only home identity and scope ABI; Rust owns canonical/legacy validation.
_mx_scope_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
_mx_scope_binary=${MX_RUST_BIN:-${MX_RUST_SOURCE_ROOT:-$(cd -- "$_mx_scope_dir/.." && pwd -P)}/target/release/mx}

mx_root_is_agent_home() {
  local id
  id=$("$_mx_scope_binary" primitive agent-home-id "$1") || return 1
  [ -n "$id" ]
}
# Source-compatible historical function name; no legacy identity is emitted.
mx_root_is_daemon_home() { mx_root_is_agent_home "$@"; }

mx_primary_scope_matches() {
  "$_mx_scope_binary" primitive primary-scope "$1" "$2"
}
