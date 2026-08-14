#!/usr/bin/env bash
# Rust transport adapter for the Herdr runtime, presentation, cleanup, and wire
# operations used by remaining sourced shell callers.
# shellcheck disable=SC2034

# Sourced cleanup callers use this stable journal filename ABI.
MX_BACKEND_HERDR_PRESENTATION_JOURNAL_SUFFIX=.herdr-presentation

mx_backend_herdr_session() { printf '%s' "${HERDR_SESSION:-default}"; }

# Narrow raw transport retained for sourced passive probes and cleanup tests.
# Active backend operations below use typed Rust owners.
mx_backend_herdr_cli() {
  local session=$1
  shift
  HERDR_SESSION="$session" herdr "$@" --session "$session"
}

mx_backend_herdr_rust() {
  local rust_bin
  rust_bin=$(mx_rust_runtime_bin) || return $?
  "$rust_bin" herdr "$@"
}

mx_backend_herdr_workspace_label() { mx_backend_herdr_rust workspace-label; }
mx_backend_herdr_tool_check() { mx_backend_herdr_rust tool-check; }
mx_backend_herdr_version_check() { mx_backend_herdr_rust version-check; }
mx_backend_herdr_server_ensure() { mx_backend_herdr_rust server-ensure "$1"; }
mx_backend_herdr_workspace_find() { mx_backend_herdr_rust workspace-find "$1"; }
mx_backend_herdr_container_ensure() { mx_backend_herdr_rust container-ensure "${1:-$PWD}"; }
mx_backend_herdr_create_task() { mx_backend_herdr_rust task-create "$1" "$2" "$3" "${4:-}"; }
mx_backend_herdr_target_ready() { mx_backend_herdr_rust target-ready "$1"; }
mx_backend_herdr_current_path() { mx_backend_herdr_rust current-path "$1"; }
mx_backend_herdr_capture() { mx_backend_herdr_rust capture "$1" "${2:-200}"; }
mx_backend_herdr_capture_ansi() { mx_backend_herdr_rust capture-ansi "$1" "${2:-200}"; }
mx_backend_herdr_composer_state() { mx_backend_herdr_rust composer-state "$1"; }
mx_backend_herdr_send_literal() { mx_backend_herdr_rust send-literal "$1" "$2"; }
mx_backend_herdr_send_key() { mx_backend_herdr_rust send-key "$1" "$2"; }
mx_backend_herdr_send_text_line() { mx_backend_herdr_rust send-text-line "$1" "$2"; }
mx_backend_herdr_send_text_submit() {
  mx_backend_herdr_rust send-submit "$1" "$2" "$3" "$4" "$5"
}
mx_backend_herdr_native_state() { mx_backend_herdr_rust native-state "$1"; }
mx_backend_herdr_busy_state() { mx_backend_herdr_rust busy-state "$1"; }
mx_backend_herdr_pane_agent_state() { mx_backend_herdr_rust pane-agent-state "$1" "$2"; }
mx_backend_herdr_agent_state() { mx_backend_herdr_rust agent-state "$1"; }
mx_backend_herdr_agent_alive() { mx_backend_herdr_rust agent-alive "$1"; }
mx_backend_herdr_kill() { mx_backend_herdr_rust kill "$1" >/dev/null 2>&1 || true; }
mx_backend_herdr_list_live() { mx_backend_herdr_rust list-live "${1:-$(mx_backend_herdr_session)}"; }
mx_backend_herdr_events_capable() { mx_backend_herdr_rust events-capable "$1"; }
mx_backend_herdr_wait_transition() { mx_backend_herdr_rust wait-transition "$@"; }
mx_backend_herdr_commit_transition() { mx_backend_herdr_rust transition-commit "$@"; }
mx_backend_herdr_clear_transition() { mx_backend_herdr_rust transition-clear "$@"; }

mx_backend_herdr_projection_id() { mx_backend_herdr_rust projection-id; }
mx_backend_herdr_projection_journal_path() { mx_backend_herdr_rust journal-path "$1" "$2"; }
mx_backend_herdr_projection_journal_create() { mx_backend_herdr_rust journal-create "$1" "$2"; }
mx_backend_herdr_projection_workspace_label() { mx_backend_herdr_rust projection-label "$1" "$2"; }
mx_backend_herdr_projection_concise_task_label() { mx_backend_herdr_rust concise-task-label "$1"; }
mx_backend_herdr_projection_home_identity() { mx_backend_herdr_rust home-identity "$1"; }
mx_backend_herdr_normalize_key() { mx_backend_herdr_rust normalize-key "$1"; }

mx_backend_herdr_projection_journal_snapshot() {
  local record rest
  record=$(mx_backend_herdr_rust journal-snapshot "$1" "$2") || return 1
  MX_BACKEND_HERDR_JOURNAL_VERSION=${record%%$'\t'*}
  rest=${record#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_TASK_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_PROJECTION_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_HOME=""
  MX_BACKEND_HERDR_JOURNAL_SESSION=""
  MX_BACKEND_HERDR_JOURNAL_WORKSPACE_ID=""
  MX_BACKEND_HERDR_JOURNAL_TAB_ID=""
  MX_BACKEND_HERDR_JOURNAL_PANE_ID=""
  MX_BACKEND_HERDR_JOURNAL_PARENT_WORKSPACE_ID=""
  MX_BACKEND_HERDR_JOURNAL_PARENT_LABEL=""
  MX_BACKEND_HERDR_JOURNAL_WORKSPACE_LABEL=""
  MX_BACKEND_HERDR_JOURNAL_TASK_LABEL=""
  [ "$MX_BACKEND_HERDR_JOURNAL_VERSION" = 2 ] || return 0
  MX_BACKEND_HERDR_JOURNAL_HOME=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_SESSION=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_WORKSPACE_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_TAB_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_PANE_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_PARENT_WORKSPACE_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_PARENT_LABEL=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_WORKSPACE_LABEL=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_JOURNAL_TASK_LABEL=$rest
}

mx_backend_herdr_projection_journal_token() {
  mx_backend_herdr_projection_journal_snapshot "$1" "$2" || return 1
  printf '%s' "$MX_BACKEND_HERDR_JOURNAL_PROJECTION_ID"
}

mx_backend_herdr_projection_journal_bind() {
  mx_backend_herdr_rust journal-bind "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9" "${10}" "${11}"
}

mx_backend_herdr_projection_journal_write_v2() {
  mx_backend_herdr_rust journal-write-v2 "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9" "${10}" "${11}" "${12}"
}

mx_backend_herdr_projection_journal_replace_endpoint() {
  mx_backend_herdr_rust journal-replace "$1" "$2" "$3" "$4" "$5" "$6"
}

mx_backend_herdr_projection_focus_snapshot() { mx_backend_herdr_rust focus-snapshot "$1"; }
mx_backend_herdr_projection_focus_restore() { mx_backend_herdr_rust focus-restore "$1" "$2"; }
mx_backend_herdr_presentation_session_lock_path() { mx_backend_herdr_rust presentation-lock-path "$1"; }
mx_backend_herdr_projection_session_socket_path() { mx_backend_herdr_rust presentation-socket-path "$1"; }
mx_backend_herdr_projection_parent_workspace_exact() { mx_backend_herdr_rust parent-workspace "$1" "$2"; }

mx_backend_herdr_projection_close_pane_focus_preserving() {
  local state="${3:-}"
  MX_BACKEND_HERDR_PROJECTION_CLOSE_AGENT_STATE=""
  if [ -n "$state" ]; then
    MX_BACKEND_HERDR_PROJECTION_CLOSE_AGENT_STATE=$(mx_backend_herdr_pane_agent_state "$1" "$2")
    [ "$MX_BACKEND_HERDR_PROJECTION_CLOSE_AGENT_STATE" = "$state" ] || return 1
  fi
  mx_backend_herdr_rust close-pane-focus "$1" "$2" "$state"
}

mx_backend_herdr_projection_cleanup_exact() {
  [ -z "$2" ] || mx_backend_herdr_projection_close_pane_focus_preserving "$1" "$2" || true
  [ -z "$3" ] || [ "$3" = "$2" ] || mx_backend_herdr_projection_close_pane_focus_preserving "$1" "$3" || true
}

mx_backend_herdr_projection_create_task() {
  local record rest
  MX_BACKEND_HERDR_PROJECTION_CLEANUP_SAFE=0
  record=$(mx_backend_herdr_rust projection-create "$1" "$2" "$3") || return 1
  MX_BACKEND_HERDR_PROJECTION_SESSION=${record%%$'\t'*}; rest=${record#*$'\t'}
  MX_BACKEND_HERDR_PROJECTION_WORKSPACE_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_PROJECTION_SEEDED_TAB_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_PROJECTION_SEEDED_PANE_ID=${rest%%$'\t'*}; rest=${rest#*$'\t'}
  MX_BACKEND_HERDR_PROJECTION_TAB_ID=${rest%%$'\t'*}
  MX_BACKEND_HERDR_PROJECTION_PANE_ID=${rest#*$'\t'}
  MX_BACKEND_HERDR_PROJECTION_CLEANUP_SAFE=1
}

mx_backend_herdr_projection_order_best_effort() {
  if ! mx_backend_herdr_rust projection-order "$1" "$2" "$3"; then
    echo "warning: herdr presentation workspace move failed or had an ambiguous response; leaving worker running without cleanup" >&2
  fi
  return 0
}

mx_backend_herdr_projection_live_binding_matches() {
  mx_backend_herdr_rust projection-live-binding "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9"
}

mx_backend_herdr_projection_recovery_allows_flat() {
  mx_backend_herdr_rust projection-recovery-allows-flat "$1" "$2" "$3"
}

mx_backend_herdr_projection_endpoint_matches_journal() {
  mx_backend_herdr_rust projection-endpoint-matches "$1" "$2" "$3" "$4"
}

mx_backend_herdr_projection_reclaim_task() {
  local record status
  MX_BACKEND_HERDR_PROJECTION_TAB_ID=""
  MX_BACKEND_HERDR_PROJECTION_PANE_ID=""
  record=$(mx_backend_herdr_rust projection-reclaim "$1" "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9" "${10}")
  status=$?
  if [ "$status" -eq 0 ]; then
    MX_BACKEND_HERDR_PROJECTION_TAB_ID=${record%%$'\t'*}
    MX_BACKEND_HERDR_PROJECTION_PANE_ID=${record#*$'\t'}
  fi
  return "$status"
}

# The Python compatibility helpers are replaced by direct Rust wire commands.
mx_backend_herdr_event_reader_cmd() {
  local rust_bin
  rust_bin=$(mx_rust_runtime_bin) || return $?
  printf '%s\n' "$rust_bin" herdr event-reader
}
