#!/usr/bin/env bash
# Shadow transport adapter for the Portion 04 Rust tmux backend.
# Every function selects the already-resolved Rust implementation for the whole
# operation and never falls back after the command begins.

mx_backend_tmux_rust() {
  local rust_bin
  rust_bin=$(mx_rust_runtime_bin) || return $?
  "$rust_bin" backend "$@"
}

mx_backend_tmux_resolve_bare_selector() { mx_backend_tmux_rust resolve-bare "$1"; }
mx_backend_tmux_capture() { mx_backend_tmux_rust capture "$1" "$2"; }
mx_backend_tmux_send_key() { mx_backend_tmux_rust send-key "$1" "$2"; }
mx_backend_tmux_send_text_submit() {
  mx_backend_tmux_rust send-submit "$1" "$2" "$3" "$4" "$5"
}
mx_backend_tmux_container_ensure() { mx_backend_tmux_rust container-ensure; }
mx_backend_tmux_create_task() { mx_backend_tmux_rust task-create "$1" "$2" "$3"; }
mx_backend_tmux_target_ready() { mx_backend_tmux_rust target-ready "$1"; }
mx_backend_tmux_current_path() { mx_backend_tmux_rust current-path "$1"; }
mx_backend_tmux_send_text_line() { mx_backend_tmux_rust send-text-line "$1" "$2"; }
mx_backend_tmux_send_literal() { mx_backend_tmux_rust send-literal "$1" "$2"; }
mx_backend_tmux_kill() { mx_backend_tmux_rust kill "$1" >/dev/null 2>&1 || true; }
mx_backend_tmux_current_command() { mx_backend_tmux_rust current-command "$1"; }
mx_backend_tmux_agent_state() { mx_backend_tmux_rust agent-state "$1"; }
mx_backend_tmux_agent_alive() { mx_backend_tmux_rust agent-alive "$1"; }
mx_tmux_composer_state() { mx_backend_tmux_rust composer-state "$1"; }
mx_pane_input_pending() { [ "$(mx_tmux_composer_state "$1")" = pending ]; }
mx_pane_is_busy() {
  local tail40
  tail40=$(mx_backend_tmux_capture "$1" 40 2>/dev/null) || return 1
  printf '%s' "$tail40" | grep -v '^[[:space:]]*$' | tail -6 \
    | grep -qiE "${MX_BUSY_REGEX:-esc (to )?interrupt|Working(\.\.\.)?|ctrl\+c to stop}"
}
mx_tmux_submit_core() { mx_backend_tmux_send_text_submit "$@"; }
