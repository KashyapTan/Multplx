#!/usr/bin/env bash
# Read-only per-installation namespace ABI; Rust owns current and legacy labels.
_mx_hometag_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
_mx_hometag_binary=${MX_RUST_BIN:-${MX_RUST_SOURCE_ROOT:-$(cd -- "$_mx_hometag_dir/.." && pwd -P)}/target/release/mx}

mx_backend_hometag() {
  "$_mx_hometag_binary" primitive backend-home-tag "$MX_ROOT" "$MX_HOME"
}

# Compatibility lookup only: never use this namespace when creating containers.
mx_backend_legacy_hometag() {
  "$_mx_hometag_binary" primitive backend-legacy-home-tag "$MX_ROOT" "$MX_HOME"
}
