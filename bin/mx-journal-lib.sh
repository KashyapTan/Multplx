#!/usr/bin/env bash
# Append one structured event to a task's observability-only journal.
#
# Callers use:
#   mx_journal_emit <task-id> <event> <detail-json>
#   mx_journal_try  <task-id> <event> <detail-json>
#
# The strict entry point validates the task id, closed event vocabulary, and
# object-shaped detail JSON, then performs one O_APPEND write to
# state/<task-id>.journal.
# It returns nonzero for malformed calls or write failures so tests can expose
# writer bugs.
# The production entry point always returns zero after calling the strict path.
# A warning is printed at most once per shell process.
#
# Journals are a best-effort observability projection.
# No operational path may read one or let an emit failure change its result.

MX_JOURNAL_EVENTS='task.spawned
status.reported
status.classified
gate.step.started
gate.step.finished
hold.opened
hold.resolved
workflow.stage.entered
workflow.stage.gated
delivery.queued
delivery.pushed
delivery.pr_opened'

_MX_JOURNAL_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd 2>/dev/null)" \
  || _MX_JOURNAL_LIB_DIR=.
MX_JOURNAL_WARNED=${MX_JOURNAL_WARNED:-0}

mx_journal_event_valid() { # <event>
  local want=${1:-} event
  while IFS= read -r event; do
    [ "$event" = "$want" ] && return 0
  done <<EOF
$MX_JOURNAL_EVENTS
EOF
  return 1
}

mx_journal_task_valid() { # <task-id>
  local id=${1:-}
  case "$id" in
    ''|.*|*[!A-Za-z0-9._-]*) return 1 ;;
  esac
  [ "${#id}" -le 64 ]
}

mx_journal_warn_once() {
  [ "$MX_JOURNAL_WARNED" -eq 0 ] || return 0
  MX_JOURNAL_WARNED=1
  printf 'mx-journal: %s\n' "$*" >&2
}

_mx_journal_state_dir() {
  local root home
  root=${MX_ROOT_OVERRIDE:-$(cd "$_MX_JOURNAL_LIB_DIR/.." && pwd)}
  home=${MX_HOME:-${MX_ROOT_OVERRIDE:-$root}}
  printf '%s\n' "${MX_STATE_OVERRIDE:-$home/state}"
}

_mx_journal_detail_compact() { # <detail-json>
  local detail=$1 compact
  if command -v jq >/dev/null 2>&1; then
    compact=$(printf '%s' "$detail" | jq -ce 'select(type == "object")' 2>/dev/null) \
      || return 1
    printf '%s' "$compact"
    return 0
  fi
  case "$detail" in
    *$'\n'*|*$'\r'*) return 1 ;;
  esac
  compact=${detail#"${detail%%[![:space:]]*}"}
  compact=${compact%"${compact##*[![:space:]]}"}
  case "$compact" in
    \{*\}) printf '%s' "$compact" ;;
    *) return 1 ;;
  esac
}

_mx_journal_append() { # <path> <line>
  local path=$1 line=$2
  [ ! -L "$path" ] || return 1
  [ ! -e "$path" ] || [ -f "$path" ] || return 1
  if command -v perl >/dev/null 2>&1; then
    printf '%s' "$line" | perl -MFcntl=:DEFAULT -e '
      use strict;
      use warnings;
      my $path = shift @ARGV;
      local $/;
      my $line = <STDIN>;
      sysopen(my $fh, $path, O_WRONLY | O_CREAT | O_APPEND, 0600) or exit 1;
      my $payload = $line . "\n";
      my $written = syswrite($fh, $payload);
      exit 1 if !defined($written) || $written != length($payload);
      close($fh) or exit 1;
    ' "$path"
    return $?
  fi
  printf '%s\n' "$line" >>"$path"
}

mx_journal_emit() { # <task-id> <event> <detail-json>
  local task=${1:-} event=${2:-} detail=${3:-} compact state source ts line path
  [ "$#" -eq 3 ] || {
    mx_journal_warn_once "emit requires task id, event, and detail JSON"
    return 2
  }
  mx_journal_task_valid "$task" || {
    mx_journal_warn_once "refusing unsafe task id"
    return 2
  }
  mx_journal_event_valid "$event" || {
    mx_journal_warn_once "refusing unknown event '$event'"
    return 2
  }
  compact=$(_mx_journal_detail_compact "$detail") || {
    mx_journal_warn_once "refusing malformed detail JSON for $event"
    return 2
  }
  state=$(_mx_journal_state_dir) || {
    mx_journal_warn_once "could not resolve the state directory"
    return 1
  }
  [ -d "$state" ] && [ ! -L "$state" ] || {
    mx_journal_warn_once "state directory is unavailable"
    return 1
  }
  source=${MX_JOURNAL_SOURCE:-$(basename "$0")}
  source=${source%.sh}
  case "$source" in
    ''|*[!A-Za-z0-9._-]*)
      mx_journal_warn_once "refusing unsafe journal source"
      return 2
      ;;
  esac
  ts=${MX_JOURNAL_NOW:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}
  printf '%s\n' "$ts" \
    | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' || {
      mx_journal_warn_once "refusing malformed UTC timestamp"
      return 2
    }
  if command -v jq >/dev/null 2>&1; then
    line=$(jq -cn --arg ts "$ts" --arg task "$task" --arg source "$source" \
      --arg event "$event" --argjson detail "$compact" \
      '{ts:$ts,task:$task,source:$source,event:$event,detail:$detail}') || {
      mx_journal_warn_once "could not compose $event"
      return 1
    }
  else
    line=$(printf '{"ts":"%s","task":"%s","source":"%s","event":"%s","detail":%s}' \
      "$ts" "$task" "$source" "$event" "$compact")
  fi
  path="$state/$task.journal"
  _mx_journal_append "$path" "$line" || {
    mx_journal_warn_once "could not append $event for $task"
    return 1
  }
}

mx_journal_try() {
  if mx_journal_emit "$@"; then
    return 0
  fi
  return 0
}
