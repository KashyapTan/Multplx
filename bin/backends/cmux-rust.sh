#!/usr/bin/env bash
# Rust transport adapter for the cmux backend.
# The legacy body remains in cmux.sh as the explicit rollback implementation.
# shellcheck disable=SC1091

mx_backend_cmux_rust() {
  local rust_bin
  rust_bin=$(mx_rust_runtime_bin) || return $?
  "$rust_bin" cmux "$@"
}

mx_backend_cmux_bin() { mx_backend_cmux_rust bin; }
mx_backend_cmux_tool_check() { mx_backend_cmux_rust tool-check; }
mx_backend_cmux_password() { mx_backend_cmux_rust password; }
mx_backend_cmux_cli() { mx_backend_cmux_rust cli "$@"; }
mx_backend_cmux_version_check() { mx_backend_cmux_rust version-check; }
mx_backend_cmux_ping_state() { mx_backend_cmux_rust ping-state; }
mx_backend_cmux_ensure_running() { mx_backend_cmux_rust ensure-running; }
mx_backend_cmux_container_ensure() { mx_backend_cmux_rust container-ensure; }
mx_backend_cmux_home_label() { mx_backend_cmux_rust home-label; }
mx_backend_cmux_scoped_title() { mx_backend_cmux_rust scoped-title "$1"; }
mx_backend_cmux_workspace_id_for_label() { mx_backend_cmux_rust workspace-id-for-label "$1"; }
mx_backend_cmux_surface_id_for_workspace() { mx_backend_cmux_rust surface-id-for-workspace "$1"; }
mx_backend_cmux_create_task() { mx_backend_cmux_rust create-task "$1" "$2"; }

mx_backend_cmux_parse_target() {
  local record
  record=$(mx_backend_cmux_rust parse-target "$1") || return 1
  MX_BACKEND_CMUX_WORKSPACE=${record%%$'\t'*}
  MX_BACKEND_CMUX_SURFACE=${record#*$'\t'}
}

mx_backend_cmux_surface_exists() { mx_backend_cmux_rust surface-exists "$1" "$2"; }
mx_backend_cmux_target_ready() { mx_backend_cmux_rust target-ready "$1" "${2:-}"; }
mx_backend_cmux_current_path() { mx_backend_cmux_rust current-path "$1" "${2:-}"; }
mx_backend_cmux_send_literal() { mx_backend_cmux_rust send-literal "$1" "$2" "${3:-}"; }
mx_backend_cmux_normalize_key() { mx_backend_cmux_rust normalize-key "$1"; }
mx_backend_cmux_send_key() { mx_backend_cmux_rust send-key "$1" "$2" "${3:-}"; }
mx_backend_cmux_send_text_line() { mx_backend_cmux_rust send-text-line "$1" "$2" "${3:-}"; }
mx_backend_cmux_capture() { mx_backend_cmux_rust capture "$1" "${2:-200}" "${3:-}"; }
mx_backend_cmux_composer_state() { mx_backend_cmux_rust composer-state "$1" "${2:-}"; }
mx_backend_cmux_send_text_submit() {
  mx_backend_cmux_rust send-submit "$1" "$2" "$3" "$4" "$5" "${6:-}"
}
mx_backend_cmux_window_of_workspace() { mx_backend_cmux_rust window-of-workspace "$1"; }
mx_backend_cmux_kill() { mx_backend_cmux_rust kill "$1" "${2:-}" "${3:-}" >/dev/null 2>&1 || true; }
mx_backend_cmux_list_live() { mx_backend_cmux_rust list-live; }
