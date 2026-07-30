#!/usr/bin/env bash
# Deterministically read one actor's current state.
#
# state/<id>.status is append-only event history, not current truth.
# For delivery tasks this reader first attributes state/<id>.gate/run.json to
# the recorded worktree, branch, and exact current HEAD.
# A valid deep-review run outranks schema-valid self-report and pane heuristics,
# while a native runtime event remains the highest-precedence signal.
#
# Output:
#   state: <working|parked|done|blocked|paused|failed|unknown> ·
#   source: <native-event|run-step|pane|status-log|none> · <detail>
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-tmux-lib.sh
. "$SCRIPT_DIR/mx-tmux-lib.sh"
# shellcheck source=bin/mx-backend.sh
. "$SCRIPT_DIR/mx-backend.sh"
# shellcheck source=bin/mx-classify-lib.sh
. "$SCRIPT_DIR/mx-classify-lib.sh"

ID=${1:-}
[ -n "$ID" ] || {
  echo "usage: mx-actor-state.sh <id>" >&2
  exit 2
}

META="$STATE/$ID.meta"
LOG="$STATE/$ID.status"
GATE_RUN="$STATE/$ID.gate/run.json"
SEP=' · '

emit() {
  local line="state: $1${SEP}source: $2"
  [ -n "${3:-}" ] && line="$line${SEP}$3"
  printf '%s\n' "$line"
  exit 0
}

[ -f "$META" ] || emit unknown none "no metadata for $ID"

meta_value() {
  grep "^$1=" "$META" 2>/dev/null | tail -1 | cut -d= -f2- || true
}

WT=$(meta_value worktree)
KIND=$(meta_value kind)
[ -n "$KIND" ] || KIND=delivery
if [ -z "$WT" ] || [ ! -d "$WT" ]; then
  emit unknown none "worktree gone (torn down?)"
fi
WT=$(cd "$WT" && pwd -P)

log_last_line() {
  [ -f "$LOG" ] || return 1
  grep -v '^[[:space:]]*$' "$LOG" 2>/dev/null | tail -1
}

map_log_state() {
  if status_is_paused "$1"; then
    echo paused
    return
  fi
  case "$(status_line_verb "$1")" in
    working) echo working ;;
    needs-decision) echo parked ;;
    blocked) echo blocked ;;
    done) echo done ;;
    failed) echo failed ;;
    *) echo unknown ;;
  esac
}

LOG_LINE=$(log_last_line || true)
LOG_VERB=$(status_line_verb "$LOG_LINE")
TASK_BACKEND=$(mx_backend_of_meta "$META")
BACKEND_TARGET=$(mx_backend_target_of_meta "$META")
EXPECTED_LABEL="mx-$ID"
NATIVE_STATE=$(mx_backend_native_state "$TASK_BACKEND" "$BACKEND_TARGET" 2>/dev/null)
case "$NATIVE_STATE" in
  working|blocked|done) NATIVE_SIGNAL=$NATIVE_STATE ;;
  *) NATIVE_SIGNAL= ;;
esac

pane_readable() {
  case "$TASK_BACKEND" in
    tmux) tmux display-message -p -t "$1" '#{pane_id}' >/dev/null 2>&1 ;;
    *) mx_backend_capture "$TASK_BACKEND" "$1" 1 "$EXPECTED_LABEL" >/dev/null 2>&1 ;;
  esac
}

actor_pane_heuristic_is_busy() {
  case "$TASK_BACKEND" in
    tmux) mx_pane_is_busy "$1" ;;
    *)
      local tail40
      tail40=$(mx_backend_capture "$TASK_BACKEND" "$1" 40 "$EXPECTED_LABEL" 2>/dev/null) \
        || return 1
      printf '%s' "$tail40" | grep -v '^[[:space:]]*$' | tail -6 \
        | grep -qiE "${MX_BUSY_REGEX:-$MX_TMUX_BUSY_REGEX_DEFAULT}"
      ;;
  esac
}

gate_run_valid() {
  local branch head
  [ "$KIND" = delivery ] || return 1
  [ -f "$GATE_RUN" ] && [ ! -L "$GATE_RUN" ] || return 1
  command -v jq >/dev/null 2>&1 || return 1
  jq -e '
    .version == 1 and
    (.task | type == "string" and length > 0) and
    (.worktree | type == "string" and length > 0) and
    (.branch | type == "string" and length > 0) and
    (.approved_head | type == "string" and test("^[0-9a-f]{40}([0-9a-f]{24})?$")) and
    (.status == "running" or .status == "parked" or .status == "passed" or .status == "failed") and
    (.step == "intent" or .step == "rebase" or .step == "review" or
     .step == "test" or .step == "document" or .step == "lint") and
    (.round | type == "number" and floor == . and . >= 1)
  ' "$GATE_RUN" >/dev/null 2>&1 || return 2
  [ "$(jq -r '.task' "$GATE_RUN")" = "$ID" ] || return 2
  [ "$(jq -r '.worktree' "$GATE_RUN")" = "$WT" ] || return 2
  branch=$(git -C "$WT" symbolic-ref --quiet --short HEAD 2>/dev/null) || return 1
  [ "$(jq -r '.branch' "$GATE_RUN")" = "$branch" ] || return 1
  head=$(git -C "$WT" rev-parse --verify HEAD 2>/dev/null) || return 1
  [ "$(jq -r '.approved_head' "$GATE_RUN")" = "$head" ] || return 1
}

if [ -e "$GATE_RUN" ] || [ -L "$GATE_RUN" ]; then
  gate_run_valid
  GATE_VALID=$?
  if [ "$GATE_VALID" -eq 2 ]; then
    emit unknown none "invalid deep-review run record"
  fi
else
  GATE_VALID=1
fi

if [ "$GATE_VALID" -eq 0 ]; then
  RUN_STATUS=$(jq -r '.status' "$GATE_RUN")
  RUN_STEP=$(jq -r '.step' "$GATE_RUN")
  RUN_ROUND=$(jq -r '.round' "$GATE_RUN")
  case "$RUN_STATUS" in
    running)
      RUN_STATE=working
      RUN_DETAIL="validating ($RUN_STEP round $RUN_ROUND)"
      ;;
    parked)
      RUN_STATE=parked
      RUN_DETAIL="parked at $RUN_STEP round $RUN_ROUND"
      FINDING_COUNT=0
      for FINDING_FILE in "$STATE/$ID.gate/findings"/*.json; do
        [ -f "$FINDING_FILE" ] || continue
        ONE_COUNT=$(jq -r '.findings | length' "$FINDING_FILE" 2>/dev/null || printf 0)
        case "$ONE_COUNT" in ''|*[!0-9]*) ONE_COUNT=0 ;; esac
        FINDING_COUNT=$((FINDING_COUNT + ONE_COUNT))
      done
      [ "$FINDING_COUNT" -eq 0 ] \
        || RUN_DETAIL="$RUN_DETAIL: $FINDING_COUNT recorded finding(s)"
      ;;
    passed)
      RUN_STATE=done
      RUN_DETAIL="validated local branch"
      ;;
    failed)
      RUN_STATE=failed
      RUN_DETAIL="validation failed at $RUN_STEP"
      ;;
  esac

  case "$LOG_VERB" in
    needs-decision|blocked)
      if [ "$RUN_STATE" != parked ] && [ "$NATIVE_SIGNAL" != blocked ]; then
        RUN_DETAIL="$RUN_DETAIL${SEP}status-log superseded by deep-review run"
      fi
      ;;
  esac

  WINNER=$(mx_signal_resolve "$NATIVE_SIGNAL" "$RUN_STATE" "$LOG_VERB" "")
  case "$WINNER" in
    native:*)
      RESOLVED_STATE=${WINNER#native:}
      emit "$RESOLVED_STATE" native-event \
        "runtime $RESOLVED_STATE${SEP}run-step still $RUN_DETAIL"
      ;;
    *)
      emit "$RUN_STATE" run-step "$RUN_DETAIL"
      ;;
  esac
fi

[ -n "$BACKEND_TARGET" ] || emit unknown none "no backend target recorded"
pane_readable "$BACKEND_TARGET" || emit unknown none "backend target gone: $BACKEND_TARGET"

LOG_STATE=unknown
SELF_REPORT_SIGNAL=
if [ -n "$LOG_VERB" ]; then
  LOG_STATE=$(map_log_state "$LOG_LINE")
  [ "$LOG_STATE" != unknown ] && SELF_REPORT_SIGNAL=$LOG_VERB
fi

HEURISTIC_SIGNAL=
if [ "$KIND" != daemon ]; then
  if actor_pane_heuristic_is_busy "$BACKEND_TARGET"; then
    HEURISTIC_SIGNAL=busy
  else
    HEURISTIC_SIGNAL=idle
  fi
fi

WINNER=$(mx_signal_resolve "$NATIVE_SIGNAL" "" "$SELF_REPORT_SIGNAL" "$HEURISTIC_SIGNAL")
case "$WINNER" in
  native:*)
    RESOLVED_STATE=${WINNER#native:}
    emit "$RESOLVED_STATE" native-event "runtime $RESOLVED_STATE"
    ;;
  self-report:*)
    emit "$LOG_STATE" status-log "$(status_line_note "$LOG_LINE")"
    ;;
  heuristic:busy)
    emit working pane "harness busy"
    ;;
  *)
    emit unknown none "no current-state source available"
    ;;
esac
