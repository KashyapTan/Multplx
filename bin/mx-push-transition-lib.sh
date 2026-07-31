#!/usr/bin/env bash
# Shared owner of the watcher's native runtime push-transition escalation.
#
# The watcher and event-wait smoke tests source this library instead of loading
# the whole watcher to obtain handle_push_transition. Its source list is limited
# to the four production boundaries the transition handler actually calls.
# Remote git pushes now originate only in bin/mx-deliver.sh outside agent
# sessions and flow back through mx-pr-check.sh. They do not call this handler;
# "push" here is the backend's native agent-state transition vocabulary.

MX_PUSH_TRANSITION_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=bin/mx-wake-lib.sh
. "$MX_PUSH_TRANSITION_LIB_DIR/mx-wake-lib.sh"
# shellcheck source=bin/mx-classify-lib.sh
. "$MX_PUSH_TRANSITION_LIB_DIR/mx-classify-lib.sh"
# shellcheck source=bin/mx-backend.sh
. "$MX_PUSH_TRANSITION_LIB_DIR/mx-backend.sh"
# shellcheck source=bin/mx-transition-lib.sh
. "$MX_PUSH_TRANSITION_LIB_DIR/mx-transition-lib.sh"
# shellcheck source=bin/mx-journal-lib.sh
. "$MX_PUSH_TRANSITION_LIB_DIR/mx-journal-lib.sh"

TRIAGE_LOG="$STATE/.watch-triage.log"
TRIAGE_LOG_MAX_BYTES=${MX_WATCH_TRIAGE_LOG_MAX_BYTES:-262144}

# Append one bounded best-effort line for an absorbed supervision event.
triage_log() {
  local sz
  printf '[%s] %s\n' "$(date '+%Y-%m-%dT%H:%M:%S%z')" "$1" >> "$TRIAGE_LOG" 2>/dev/null || return 0
  sz=$(wc -c < "$TRIAGE_LOG" 2>/dev/null | tr -d '[:space:]')
  case "$sz" in ''|*[!0-9]*) return 0 ;; esac
  if [ "$sz" -ge "$TRIAGE_LOG_MAX_BYTES" ]; then
    tail -n 2000 "$TRIAGE_LOG" > "$TRIAGE_LOG.tmp" 2>/dev/null && mv -f "$TRIAGE_LOG.tmp" "$TRIAGE_LOG" 2>/dev/null
    rm -f "$TRIAGE_LOG.tmp" 2>/dev/null || true
  fi
}

# Exit after reporting one actionable wake. Tests override this callback.
wake() {
  case "$1" in
    heartbeat*) echo $(( $(cat "$STATE/.heartbeat-streak" 2>/dev/null || echo 0) + 1 )) > "$STATE/.heartbeat-streak" ;;
    *) echo 0 > "$STATE/.heartbeat-streak" ;;
  esac
  echo "$1"
  exit 0
}

_hb_surfaced_path() {
  printf '%s/.hb-surfaced-%s' "$STATE" "$(printf '%s' "$1" | tr ':/.' '___')"
}

# Record a maintainer-relevant status after its durable wake has been enqueued.
mark_surfaced() {  # <status-file>
  local f=$1 task last
  task=$(basename "$f"); task="${task%.status}"
  last=$(last_status_line "$f")
  [ -n "$last" ] || return 0
  status_is_maintainer_relevant "$last" || return 0
  printf '%s' "$last" > "$(_hb_surfaced_path "$task")"
}

# Act on a fresh actionable transition from a push-capable backend.
handle_push_transition() {  # <backend> <session> <record>
  local backend=$1 session=$2 record=$3 pane_id to window task reason last verb winner detail
  pane_id=$(mx_transition_pane_id "$record")
  to=$(mx_transition_to_status "$record")
  [ -n "$pane_id" ] || { sleep 1; return; }
  window="$session:$pane_id"
  task=$(window_to_task "$window" "$STATE")
  last=$(last_status_line "$STATE/$task.status")
  verb=$(status_line_verb "$last")
  winner=$(mx_signal_resolve "$to" "" "$verb" "")
  case "$winner" in
    native:*)
      if [ -n "$verb" ]; then
        triage_log "native $to overruled self-report $verb: $window"
      fi
      if [ "${MX_JOURNAL_SOURCE:-}" = mx-watch ]; then
        if detail=$(jq -cn --arg verdict "$to" --arg report "$verb" '
            {
              verdict:$verdict,
              tier:"native-event",
              conflicts:(
                if $report != "" and $report != $verdict
                then [{tier:"validated-report",signal:$report}]
                else []
                end
              )
            }
          ' 2>/dev/null); then
          MX_STATE_OVERRIDE="$STATE" \
            mx_journal_try "$task" status.classified "$detail"
        else
          mx_journal_warn_once "could not compose status.classified for $task"
        fi
      fi
      ;;
    *)
      triage_log "ignored push $to with unresolved precedence ($winner): $window"
      mx_backend_commit_transition "$backend" "$STATE" "$session" "$record" || exit 1
      return
      ;;
  esac
  if status_is_paused "$last"; then
    reason="stale: $window (native-event=$to; herdr: agent $to - native event overruled declared pause, waiting on human)"
  else
    reason="stale: $window (native-event=$to; herdr: agent $to - waiting on human, escalated immediately, not via wedge timer)"
  fi
  mx_wake_append stale "$window" "$reason" || exit 1
  mx_backend_commit_transition "$backend" "$STATE" "$session" "$record" || exit 1
  mark_surfaced "$STATE/$task.status"
  wake "$reason"
}
