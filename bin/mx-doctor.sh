#!/usr/bin/env bash
# Read-only, on-demand Multplx invariant sweep.
#
# Usage:
#   mx-doctor.sh
#   mx-doctor.sh --json
#   mx-doctor.sh --fix
#   mx-doctor.sh --check <name>
#
# Exit status is the worst reported severity: 0 for all OK, 1 for WARN, and 2
# for FAIL. Default mode never mutates Multplx state. --fix is a closed
# whitelist with exactly two entries:
# - clear state/.watch.lock only after mx-lock-lib.sh proves it stale and the
#   race-safe mx-wake-lib.sh lock acquisition rechecks ownership;
# - prune wake rows whose task metadata is provably absent, while holding the
#   queue's own lock and re-evaluating every row under that lock.
#
# Doctor never kills a process, tears down a task or worktree, closes a hold,
# resumes a gate/workflow, changes a backlog item, or touches compatibility
# links. Exact environment knobs are documented by --help through this header:
#   MX_DOCTOR_WATCHER_GRACE=300
#   MX_DOCTOR_LOCK_STALE_SECS=2
#   MX_DOCTOR_DISPATCH_MAX_AGE_SECS=172800
#   MX_DOCTOR_COMPAT_PATHS=<newline-separated paths>  (test/operator override)
#   MX_DOCTOR_TREEHOUSE_STATUS_FILE=<combined status fixture> (test seam)
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd -P)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
CONFIG="${MX_CONFIG_OVERRIDE:-$MX_HOME/config}"
PROJECTS="${MX_PROJECTS_OVERRIDE:-$MX_HOME/projects}"
DOCTOR_STATE=$STATE

# Source the shared PID/lock primitives without letting mx-wake-lib.sh create an
# absent state directory in read-only mode. Its functions use STATE dynamically,
# so restore the requested state path immediately afterward.
if [ ! -d "$STATE" ]; then
  STATE=$MX_ROOT
fi
# shellcheck source=bin/mx-wake-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-wake-lib.sh"
STATE=$DOCTOR_STATE
MX_WAKE_QUEUE="$STATE/.wake-queue"
MX_WAKE_QUEUE_LOCK="$STATE/.wake-queue.lock"
# shellcheck source=bin/mx-lock-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-lock-lib.sh"
# shellcheck source=bin/mx-backend.sh disable=SC1091
. "$SCRIPT_DIR/mx-backend.sh"
# shellcheck source=bin/mx-supervision-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-supervision-lib.sh"
# shellcheck source=bin/mx-probe-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-probe-lib.sh"
# shellcheck source=bin/mx-backlog-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-backlog-lib.sh"

DOCTOR_WATCHER_GRACE=${MX_DOCTOR_WATCHER_GRACE:-${MX_GUARD_GRACE:-300}}
DOCTOR_LOCK_STALE_SECS=${MX_DOCTOR_LOCK_STALE_SECS:-${MX_LOCK_STALE_AFTER:-2}}
DOCTOR_DISPATCH_MAX_AGE_SECS=${MX_DOCTOR_DISPATCH_MAX_AGE_SECS:-172800}

usage() {
  sed -n '2,/^set -u$/s/^# \{0,1\}//p' "$0"
}

die_usage() {
  printf 'mx-doctor: %s\n' "$*" >&2
  usage >&2
  exit 2
}

for numeric_value in \
  "$DOCTOR_WATCHER_GRACE" \
  "$DOCTOR_LOCK_STALE_SECS" \
  "$DOCTOR_DISPATCH_MAX_AGE_SECS"; do
  case "$numeric_value" in
    ''|*[!0-9]*) die_usage "thresholds must be non-negative integers" ;;
  esac
done

OUTPUT_JSON=0
FIX_MODE=0
SELECTED_CHECK=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --json)
      OUTPUT_JSON=1
      ;;
    --fix)
      FIX_MODE=1
      ;;
    --check)
      shift
      [ "$#" -gt 0 ] || die_usage "--check requires a name"
      [ -z "$SELECTED_CHECK" ] || die_usage "--check may be specified only once"
      SELECTED_CHECK=$1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die_usage "unknown argument: $1"
      ;;
  esac
  shift
done

RESULTS=$(mktemp "${TMPDIR:-/tmp}/mx-doctor-results.XXXXXX") \
  || { printf 'mx-doctor: could not create result buffer\n' >&2; exit 2; }
FIXES=$(mktemp "${TMPDIR:-/tmp}/mx-doctor-fixes.XXXXXX") \
  || { rm -f "$RESULTS"; printf 'mx-doctor: could not create fix buffer\n' >&2; exit 2; }
TREEHOUSE_ROWS=
cleanup() {
  rm -f "$RESULTS" "$FIXES"
  [ -z "${TREEHOUSE_ROWS:-}" ] || rm -f "$TREEHOUSE_ROWS"
}
trap cleanup EXIT HUP INT TERM

doctor_clean_field() {
  LC_ALL=C tr '\t\r\n' '   '
}

doctor_add() { # <severity> <category> <name> <message> [suggestion] [fixable]
  local severity=$1 category=$2 name=$3 message=$4 suggestion=${5:-} fixable=${6:-false}
  message=$(printf '%s' "$message" | doctor_clean_field)
  suggestion=$(printf '%s' "$suggestion" | doctor_clean_field)
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$severity" "$category" "$name" "$message" "$suggestion" "$fixable" >>"$RESULTS"
}

doctor_fix_log() {
  printf '%s\n' "$*" >>"$FIXES"
}

doctor_meta_value() {
  mx_meta_get "$1" "$2"
}

doctor_real_path() {
  local path=$1
  if [ -d "$path" ]; then
    (cd "$path" 2>/dev/null && pwd -P) || printf '%s\n' "$path"
  else
    printf '%s\n' "$path"
  fi
}

doctor_endpoint_exists_for_meta() {
  local meta=$1 id backend target
  id=$(basename "$meta" .meta)
  backend=$(mx_backend_of_meta "$meta")
  target=$(mx_backend_target_of_meta "$meta")
  [ -n "$target" ] || return 1
  mx_backend_target_exists "$backend" "$target" "mx-$id"
}

doctor_check_watcher_lock() {
  local lock="$STATE/.watch.lock" pid expected actual age
  if [ ! -e "$lock" ] && [ ! -L "$lock" ]; then
    doctor_add OK "locks & liveness" watcher-lock "watcher lock absent" "" true
    return
  fi
  if [ ! -d "$lock" ]; then
    doctor_add FAIL "locks & liveness" watcher-lock \
      "$lock is not a lock directory" \
      "inspect $lock; bin/mx-watch-arm.sh owns watcher recovery" true
    return
  fi
  pid=$(cat "$lock/pid" 2>/dev/null || true)
  expected=$(cat "$lock/pid-identity" 2>/dev/null || true)
  if mx_pid_alive "$pid" && [ -n "$expected" ]; then
    actual=$(mx_pid_identity "$pid" 2>/dev/null || true)
    if [ -n "$actual" ] && [ "$actual" = "$expected" ]; then
      doctor_add OK "locks & liveness" watcher-lock \
        "lock names live pid $pid (identity verified)" "" true
      return
    fi
  fi
  age=$(mx_lock_age "$lock" 2>/dev/null || true)
  if mx_lock_is_provably_stale "$lock" "$STATE" "$DOCTOR_LOCK_STALE_SECS"; then
    doctor_add FAIL "locks & liveness" watcher-lock \
      "watcher lock names a dead or reused pid and is provably stale${age:+ (age ${age}s)}" \
      "run bin/mx-doctor.sh --fix or inspect and re-arm with bin/mx-watch-arm.sh" true
  else
    doctor_add FAIL "locks & liveness" watcher-lock \
      "watcher lock identity is not live, but staleness cannot be proven safely" \
      "inspect $lock and re-arm with bin/mx-watch-arm.sh; doctor will not clear it" true
  fi
}

doctor_check_watcher_beacon() {
  mx_supervision_status "$STATE" "$DOCTOR_WATCHER_GRACE"
  if [ "$MX_SUP_IN_FLIGHT" -eq 0 ]; then
    doctor_add OK "locks & liveness" watcher-beacon \
      "no in-flight tasks require a fresh watcher beacon"
  elif [ "$MX_SUP_WATCHER_FRESH" = true ]; then
    doctor_add OK "locks & liveness" watcher-beacon \
      "$MX_SUP_IN_FLIGHT in-flight task(s); beacon $MX_SUP_BEACON_DESC"
  else
    doctor_add WARN "locks & liveness" watcher-beacon \
      "$MX_SUP_IN_FLIGHT in-flight task(s), but the watcher beacon is $MX_SUP_BEACON_DESC (grace ${DOCTOR_WATCHER_GRACE}s)" \
      "inspect the watcher and re-arm it with bin/mx-watch-arm.sh"
  fi
}

doctor_treehouse_parse() {
  awk '
    {
      line=$0
      gsub(/\033\[[0-9;]*[[:alpha:]]/, "", line)
      status=""
      if (match(line, /^[^[:space:]]+[[:space:]]+available[[:space:]]+/)) status="available"
      else if (match(line, /^[^[:space:]]+[[:space:]]+in use[[:space:]]+/)) status="in use"
      else if (match(line, /^[^[:space:]]+[[:space:]]+dirty[[:space:]]+/)) status="dirty"
      else if (match(line, /^[^[:space:]]+[[:space:]]+leased[[:space:]]+/)) status="leased"
      else if (match(line, /^[^[:space:]]+[[:space:]]+you.re here[[:space:]]+/)) status="you are here"
      if (status == "") next
      path=substr(line, RLENGTH + 1)
      sub(/[[:space:]]+\(held by [^)]*\)$/, "", path)
      print status "\t" path
    }
  '
}

doctor_meta_owns_worktree() {
  local wanted=$1 meta path home
  wanted=$(doctor_real_path "$wanted")
  for meta in "$STATE"/*.meta; do
    [ -f "$meta" ] || continue
    path=$(doctor_meta_value "$meta" worktree)
    home=$(doctor_meta_value "$meta" home)
    [ -n "$path" ] && [ "$(doctor_real_path "$path")" = "$wanted" ] && return 0
    [ -n "$home" ] && [ "$(doctor_real_path "$home")" = "$wanted" ] && return 0
  done
  return 1
}

doctor_collect_treehouse_rows() {
  local roots root output
  TREEHOUSE_ROWS=$(mktemp "${TMPDIR:-/tmp}/mx-doctor-treehouse.XXXXXX") || return 1
  if [ -n "${MX_DOCTOR_TREEHOUSE_STATUS_FILE:-}" ]; then
    [ -f "$MX_DOCTOR_TREEHOUSE_STATUS_FILE" ] || return 1
    doctor_treehouse_parse <"$MX_DOCTOR_TREEHOUSE_STATUS_FILE" >"$TREEHOUSE_ROWS"
    return 0
  fi
  command -v treehouse >/dev/null 2>&1 || return 2
  roots=$MX_ROOT
  for root in "$PROJECTS"/*; do
    [ -d "$root" ] || continue
    roots="$roots
$root"
  done
  for root in "$STATE"/*.meta; do
    [ -f "$root" ] || continue
    output=$(doctor_meta_value "$root" project)
    [ -n "$output" ] || continue
    roots="$roots
$output"
  done
  while IFS= read -r root; do
    [ -n "$root" ] && [ -d "$root" ] || continue
    git -C "$root" rev-parse --show-toplevel >/dev/null 2>&1 || continue
    if ! output=$(cd "$root" && NO_COLOR=1 treehouse status 2>/dev/null); then
      return 1
    fi
    printf '%s\n' "$output" | doctor_treehouse_parse >>"$TREEHOUSE_ROWS"
  done <<EOF
$(printf '%s\n' "$roots" | LC_ALL=C sort -u)
EOF
  LC_ALL=C sort -u "$TREEHOUSE_ROWS" -o "$TREEHOUSE_ROWS"
}

doctor_check_orphan_worktrees() {
  local meta id path missing=0 active=0 orphan=0 status worktree issues='' rc
  for meta in "$STATE"/*.meta; do
    [ -f "$meta" ] || continue
    id=$(basename "$meta" .meta)
    path=$(doctor_meta_value "$meta" worktree)
    if [ -z "$path" ] || [ ! -d "$path" ]; then
      missing=$((missing + 1))
      issues="${issues}${issues:+; }$id records missing worktree ${path:-<empty>}"
    fi
  done
  doctor_collect_treehouse_rows
  rc=$?
  if [ "$rc" -eq 2 ]; then
    if [ "$missing" -gt 0 ]; then
      doctor_add FAIL "tasks & worktrees" orphan-worktrees \
        "$issues; treehouse is unavailable, so active pool paths were not checked" \
        "repair the recorded task path, install treehouse using the tools finding, and rerun this check"
    else
      doctor_add WARN "tasks & worktrees" orphan-worktrees \
        "treehouse is unavailable; recorded task worktree paths were checked only" \
        "install treehouse using the tools finding, then rerun this check"
    fi
    return
  fi
  if [ "$rc" -ne 0 ]; then
    if [ "$missing" -gt 0 ]; then
      doctor_add FAIL "tasks & worktrees" orphan-worktrees \
        "$issues; treehouse inventory could not be read" \
        "repair the recorded task path, then run treehouse status in the affected project"
    else
      doctor_add WARN "tasks & worktrees" orphan-worktrees \
        "treehouse inventory could not be read" \
        "run treehouse status in the affected project and inspect its pool"
    fi
    return
  fi
  while IFS=$'\t' read -r status worktree; do
    [ -n "$status" ] || continue
    case "$worktree" in
      "~/"*) worktree="$HOME/${worktree#\~/}" ;;
    esac
    case "$status" in
      leased|"in use"|"you are here")
        active=$((active + 1))
        if ! doctor_meta_owns_worktree "$worktree"; then
          orphan=$((orphan + 1))
          issues="${issues}${issues:+; }active treehouse path $worktree has no task metadata"
        fi
        ;;
    esac
  done <"$TREEHOUSE_ROWS"
  if [ "$missing" -gt 0 ] || [ "$orphan" -gt 0 ]; then
    doctor_add FAIL "tasks & worktrees" orphan-worktrees "$issues" \
      "inspect treehouse status and use bin/mx-teardown.sh <id> for owned cleanup"
  else
    doctor_add OK "tasks & worktrees" orphan-worktrees \
      "$active active treehouse path(s) and all recorded task worktrees are accounted for"
  fi
}

doctor_pid_record_result() { # <label> <pid> <identity>
  local label=$1 pid=$2 identity=$3 actual
  [ -n "$pid" ] || return 1
  if ! mx_pid_alive "$pid"; then
    printf '%s records dead pid %s' "$label" "$pid"
    return 0
  fi
  [ -n "$identity" ] || {
    printf '%s records pid %s without an identity' "$label" "$pid"
    return 0
  }
  actual=$(mx_pid_identity "$pid" 2>/dev/null || true)
  [ -n "$actual" ] && [ "$actual" = "$identity" ] || {
    printf '%s records reused or unverifiable pid %s' "$label" "$pid"
    return 0
  }
  return 1
}

doctor_check_dangling_pids() {
  local meta id pid identity issue issues='' count=0 lock
  for meta in "$STATE"/*.meta; do
    [ -f "$meta" ] || continue
    id=$(basename "$meta" .meta)
    pid=$(doctor_meta_value "$meta" pid)
    [ -n "$pid" ] || continue
    identity=$(doctor_meta_value "$meta" pid_identity)
    [ -n "$identity" ] || identity=$(doctor_meta_value "$meta" pid-identity)
    if issue=$(doctor_pid_record_result "$id.meta" "$pid" "$identity"); then
      count=$((count + 1))
      issues="${issues}${issues:+; }$issue"
    fi
  done
  for lock in "$STATE"/.supervise-daemon.lock "$STATE"/.afk-launch.lock "$STATE"/.subsuper-*.lock; do
    [ -d "$lock" ] || continue
    pid=$(cat "$lock/pid" 2>/dev/null || true)
    identity=$(cat "$lock/pid-identity" 2>/dev/null || true)
    if issue=$(doctor_pid_record_result "$(basename "$lock")" "$pid" "$identity"); then
      count=$((count + 1))
      issues="${issues}${issues:+; }$issue"
    fi
  done
  if [ "$count" -gt 0 ]; then
    doctor_add FAIL "tasks & worktrees" dangling-pids "$issues" \
      "use the owning lifecycle command; bin/mx-teardown.sh <id> owns task cleanup"
  else
    doctor_add OK "tasks & worktrees" dangling-pids \
      "all persisted task and supervisor pid identities are live"
  fi
}

doctor_queue_line_state() { # 0 orphan, 1 owned/global, 2 malformed
  local line=$1 epoch seq kind key payload id='' meta
  DOCTOR_QUEUE_ID=
  IFS=$'\t' read -r epoch seq kind key payload <<EOF
$line
EOF
  case "$epoch:$seq" in
    *[!0-9:]*|:*) return 2 ;;
  esac
  [ -n "$payload" ] || return 2
  case "$kind" in
    signal)
      case "$key" in
        *.status) id=${key%.status} ;;
        *.turn-ended) id=${key%.turn-ended} ;;
        *) return 2 ;;
      esac
      ;;
    check)
      case "$key" in
        dispatch-queue|pr-poll-retirement|unauthenticated-state-checks) return 1 ;;
        *.check.sh) id=$(basename "$key" .check.sh) ;;
        *) return 1 ;;
      esac
      ;;
    stale)
      for meta in "$STATE"/*.meta; do
        [ -f "$meta" ] || continue
        [ "$(doctor_meta_value "$meta" window)" = "$key" ] || continue
        id=$(basename "$meta" .meta)
        break
      done
      [ -n "$id" ] || {
        case "$key" in
          mx-*) id=${key#mx-} ;;
          *:mx-*) id=${key##*:mx-} ;;
          *) return 2 ;;
        esac
      }
      ;;
    heartbeat)
      return 1
      ;;
    *)
      return 2
      ;;
  esac
  case "$id" in
    ''|.*|*[!A-Za-z0-9._-]*) return 2 ;;
  esac
  DOCTOR_QUEUE_ID=$id
  if [ ! -e "$STATE/$id.meta" ] && [ ! -L "$STATE/$id.meta" ]; then
    return 0
  fi
  return 1
}

doctor_check_wake_queue_orphans() {
  local queue="$STATE/.wake-queue" line rc orphan=0 malformed=0 ids=''
  if [ ! -e "$queue" ] && [ ! -L "$queue" ]; then
    doctor_add OK "queues, holds & runs" wake-queue-orphans "wake queue absent" "" true
    return
  fi
  if [ ! -f "$queue" ] || [ -L "$queue" ]; then
    doctor_add FAIL "queues, holds & runs" wake-queue-orphans \
      "wake queue is not a regular non-symlink file" \
      "inspect $queue; bin/mx-wake-drain.sh owns queue consumption" true
    return
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    if doctor_queue_line_state "$line"; then
      orphan=$((orphan + 1))
      ids="${ids}${ids:+, }${DOCTOR_QUEUE_ID:-unknown}"
    else
      rc=$?
      [ "$rc" -ne 2 ] || malformed=$((malformed + 1))
    fi
  done <"$queue"
  if [ "$malformed" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" wake-queue-orphans \
      "$malformed malformed wake row(s); $orphan provably orphaned row(s)${ids:+ for $ids}" \
      "inspect $queue; doctor preserves malformed or uncertain rows" true
  elif [ "$orphan" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" wake-queue-orphans \
      "$orphan wake row(s) reference absent task metadata: $ids" \
      "run bin/mx-doctor.sh --fix to prune only the proven orphan rows" true
  else
    doctor_add OK "queues, holds & runs" wake-queue-orphans \
      "every task-scoped wake row has task metadata" "" true
  fi
}

doctor_check_stateless_sessions() {
  local meta id backend target missing=0 issues=''
  for meta in "$STATE"/*.meta; do
    [ -f "$meta" ] || continue
    id=$(basename "$meta" .meta)
    backend=$(mx_backend_of_meta "$meta")
    target=$(mx_backend_target_of_meta "$meta")
    if [ -z "$target" ] || ! doctor_endpoint_exists_for_meta "$meta"; then
      missing=$((missing + 1))
      issues="${issues}${issues:+; }$id has no live $backend endpoint ${target:-<empty>}"
    fi
  done
  if [ "$missing" -gt 0 ]; then
    doctor_add FAIL "tasks & worktrees" stateless-sessions "$issues" \
      "reconcile the recorded endpoint, then use bin/mx-teardown.sh <id> if cleanup is intended"
  else
    doctor_add OK "tasks & worktrees" stateless-sessions \
      "every task metadata record has a live backend endpoint"
  fi
}

doctor_backlog_has_id() {
  local id=$1
  [ -f "$DATA/backlog.md" ] \
    && grep -Eq "^- \\[[ x]\\] ${id//./\\.}([[:space:]]|$)" "$DATA/backlog.md"
}

doctor_check_open_holds() {
  local backlog="$DATA/backlog.md" origin meta reviewed invalid=0 count=0 issues='' verify_out
  if [ ! -e "$backlog" ] && [ ! -L "$backlog" ]; then
    doctor_add OK "queues, holds & runs" open-holds "backlog absent; no open decision holds"
    return
  fi
  if ! mx_backlog_validate "$backlog" >/dev/null 2>&1; then
    doctor_add FAIL "queues, holds & runs" open-holds \
      "backlog is unreadable or invalid" \
      "repair data/backlog.md through bin/mx-backlog.sh before resolving holds"
    return
  fi
  while IFS= read -r origin; do
    [ -n "$origin" ] || continue
    count=$((count + 1))
    meta="$STATE/$origin.meta"
    if [ -f "$meta" ]; then
      reviewed=$(doctor_meta_value "$meta" decisions_reviewed)
      if [ "$reviewed" = 1 ]; then
        if ! verify_out=$(MX_HOME="$MX_HOME" MX_STATE_OVERRIDE="$STATE" \
          MX_DATA_OVERRIDE="$DATA" "$SCRIPT_DIR/mx-decision-hold.sh" verify "$origin" 2>&1); then
          invalid=$((invalid + 1))
          issues="${issues}${issues:+; }$origin attestation failed: ${verify_out%%$'\n'*}"
        fi
      fi
      continue
    fi
    if [ -f "$DATA/$origin/report.md" ] || doctor_backlog_has_id "$origin"; then
      continue
    fi
    invalid=$((invalid + 1))
    issues="${issues}${issues:+; }hold origin $origin has no task metadata, report, or backlog record"
  done < <(awk '
    /^- \[ \] / {
      active = ($0 ~ /\(kind: maintainer\)/ && $0 ~ /\(hold-kind: maintainer\)/)
      next
    }
    active && /^  Origin: / {
      print substr($0, 11)
      active=0
    }
  ' "$backlog")
  if [ "$invalid" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" open-holds "$issues" \
      "inspect with bin/mx-backlog.sh and resolve only through bin/mx-decision-hold.sh resolve"
  else
    doctor_add OK "queues, holds & runs" open-holds \
      "$count open decision hold(s) have a live or durably preserved origin"
  fi
}

doctor_request_value() {
  local file=$1 key=$2
  sed -n "s/^${key}=//p" "$file" 2>/dev/null | tail -1
}

doctor_check_dispatch_queue_age() {
  local dir="$STATE/.dispatch-queue" record id enqueued now age old=0 invalid=0 issues=''
  now=$(date +%s)
  for record in "$dir"/*.request; do
    [ -e "$record" ] || [ -L "$record" ] || continue
    id=$(basename "$record" .request)
    if [ ! -f "$record" ] || [ -L "$record" ]; then
      invalid=$((invalid + 1))
      issues="${issues}${issues:+; }$id has an unsafe queue record"
      continue
    fi
    enqueued=$(doctor_request_value "$record" enqueued_at)
    case "$enqueued" in
      ''|*[!0-9]*)
        invalid=$((invalid + 1))
        issues="${issues}${issues:+; }$id has an invalid enqueue time"
        continue
        ;;
    esac
    age=$((now - enqueued))
    if [ "$age" -gt "$DOCTOR_DISPATCH_MAX_AGE_SECS" ]; then
      old=$((old + 1))
      issues="${issues}${issues:+; }$id queued ${age}s"
    fi
  done
  if [ "$invalid" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" dispatch-queue-age "$issues" \
      "inspect with bin/mx-headroom.sh --queue; cancel only with --queue-cancel <id>"
  elif [ "$old" -gt 0 ]; then
    doctor_add WARN "queues, holds & runs" dispatch-queue-age \
      "$old dispatch request(s) exceed ${DOCTOR_DISPATCH_MAX_AGE_SECS}s: $issues" \
      "inspect with bin/mx-headroom.sh --queue; cancel stale intent with --queue-cancel <id>"
  else
    doctor_add OK "queues, holds & runs" dispatch-queue-age \
      "no dispatch request exceeds ${DOCTOR_DISPATCH_MAX_AGE_SECS}s"
  fi
}

doctor_check_gate_runs() {
  local dir run id status meta bad=0 count=0 issues=''
  if ! command -v jq >/dev/null 2>&1; then
    doctor_add WARN "queues, holds & runs" gate-runs \
      "jq is unavailable; gate records were not evaluated" \
      "install jq using the tools finding"
    return
  fi
  for dir in "$STATE"/*.gate; do
    [ -d "$dir" ] && [ ! -L "$dir" ] || continue
    id=$(basename "$dir" .gate)
    run="$dir/run.json"
    count=$((count + 1))
    if [ ! -f "$run" ] || [ -L "$run" ] || ! jq -e \
      --arg id "$id" \
      '.version == 1 and .task == $id and
       (.status == "running" or .status == "parked" or
        .status == "passed" or .status == "failed")' "$run" >/dev/null 2>&1; then
      bad=$((bad + 1))
      issues="${issues}${issues:+; }$id has an invalid gate run record"
      continue
    fi
    status=$(jq -r '.status' "$run")
    case "$status" in
      passed|failed) continue ;;
    esac
    meta="$STATE/$id.meta"
    if [ ! -f "$meta" ] || ! doctor_endpoint_exists_for_meta "$meta"; then
      bad=$((bad + 1))
      issues="${issues}${issues:+; }$id is $status with no live task endpoint"
    fi
  done
  if [ "$bad" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" gate-runs "$issues" \
      "resume with bin/mx-deep-review.sh <id>, or reconcile the task before teardown"
  else
    doctor_add OK "queues, holds & runs" gate-runs \
      "$count gate run record(s) are terminal or backed by a live task"
  fi
}

doctor_workflow_lock_live() {
  local lock=$1 pid
  [ -d "$lock" ] || return 1
  pid=$(cat "$lock/pid" 2>/dev/null || true)
  mx_pid_alive "$pid"
}

doctor_check_workflow_runs() {
  local dir run id status stage record stage_status task meta bad=0 count=0 issues=''
  if ! command -v jq >/dev/null 2>&1; then
    doctor_add WARN "queues, holds & runs" workflow-runs \
      "jq is unavailable; workflow records were not evaluated" \
      "install jq using the tools finding"
    return
  fi
  for dir in "$STATE"/*.workflow; do
    [ -d "$dir" ] && [ ! -L "$dir" ] || continue
    id=$(basename "$dir" .workflow)
    run="$dir/run.json"
    count=$((count + 1))
    if [ ! -f "$run" ] || [ -L "$run" ] || ! jq -e \
      --arg id "$id" \
      '.version == 1 and .run == $id and
       (.status == "running" or .status == "waiting" or .status == "failed" or
        .status == "completed" or .status == "aborted")' "$run" >/dev/null 2>&1; then
      bad=$((bad + 1))
      issues="${issues}${issues:+; }$id has an invalid workflow run record"
      continue
    fi
    status=$(jq -r '.status' "$run")
    case "$status" in
      completed|aborted|failed) continue ;;
      running)
        if ! doctor_workflow_lock_live "$dir/.reconcile.lock"; then
          bad=$((bad + 1))
          issues="${issues}${issues:+; }$id says running without a live reconcile lock"
        fi
        ;;
      waiting)
        stage=$(jq -r '.current_stage // empty' "$run")
        record="$dir/stages/$stage.json"
        [ -f "$record" ] || continue
        stage_status=$(jq -r '.status // empty' "$record" 2>/dev/null || true)
        if [ "$stage_status" = waiting-agent ]; then
          task=$(jq -r '.task_id // empty' "$record" 2>/dev/null || true)
          meta="$STATE/$task.meta"
          if [ -z "$task" ] || [ ! -f "$meta" ] || ! doctor_endpoint_exists_for_meta "$meta"; then
            bad=$((bad + 1))
            issues="${issues}${issues:+; }$id waits for actor ${task:-<empty>} with no live endpoint"
          fi
        fi
        ;;
    esac
  done
  if [ "$bad" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" workflow-runs "$issues" \
      "reconstruct with bin/mx-workflow.sh resume <run-id> or explicitly abort it"
  else
    doctor_add OK "queues, holds & runs" workflow-runs \
      "$count workflow run record(s) are terminal, intentionally waiting, or live"
  fi
}

doctor_record_value() {
  local record=$1 key=$2 value
  value=$(sed -n "s/^${key}=//p" "$record" 2>/dev/null | head -1)
  if [ -n "$value" ]; then
    printf '%s\n' "$value"
    return
  fi
  if command -v jq >/dev/null 2>&1; then
    jq -r --arg key "$key" '.[$key] // empty' "$record" 2>/dev/null || true
  fi
}

doctor_check_orphan_servers() {
  local record pid identity actual artifact bad=0 count=0 issues='' known_pids='' listener
  for record in "$STATE"/.vplan/*.run "$STATE"/.viz/server.run; do
    [ -e "$record" ] || [ -L "$record" ] || continue
    count=$((count + 1))
    if [ ! -f "$record" ] || [ -L "$record" ]; then
      bad=$((bad + 1))
      issues="${issues}${issues:+; }$record is not a regular run record"
      continue
    fi
    pid=$(doctor_record_value "$record" pid)
    identity=$(doctor_record_value "$record" pid_identity)
    [ -n "$identity" ] || identity=$(doctor_record_value "$record" pid-identity)
    actual=$(mx_pid_identity "$pid" 2>/dev/null || true)
    if ! mx_pid_alive "$pid" || [ -z "$identity" ] || [ "$actual" != "$identity" ]; then
      bad=$((bad + 1))
      artifact=$(doctor_record_value "$record" artifact)
      case "$record" in
        */.vplan/*)
          issues="${issues}${issues:+; }stale vplan record ${artifact:-$record}"
          ;;
        *)
          issues="${issues}${issues:+; }stale viz server record $record"
          ;;
      esac
    else
      known_pids="${known_pids}${known_pids:+ }$pid"
    fi
  done
  if command -v lsof >/dev/null 2>&1; then
    while IFS= read -r listener; do
      case " $known_pids " in
        *" $listener "*) ;;
        *)
          bad=$((bad + 1))
          issues="${issues}${issues:+; }listener pid $listener occupies a reserved review/dashboard port without a matching record"
          ;;
      esac
    done < <(lsof -nP -iTCP:4870-4909 -sTCP:LISTEN -Fp 2>/dev/null \
      | sed -n 's/^p//p' | LC_ALL=C sort -u)
  fi
  if [ "$bad" -gt 0 ]; then
    doctor_add FAIL "queues, holds & runs" orphan-servers "$issues" \
      "use bin/mx-vplan.sh stop <file> or bin/mx-viz.sh stop; those owners verify identity before cleanup"
  else
    doctor_add OK "queues, holds & runs" orphan-servers \
      "$count loopback server run record(s) have live matching identities"
  fi
}

doctor_check_tools() {
  local backend code subject detail count=0 messages='' suggestions=''
  backend=$(mx_backend_name)
  while IFS=$'\t' read -r code subject detail; do
    [ -n "$code" ] || continue
    count=$((count + 1))
    case "$code" in
      MISSING)
        messages="${messages}${messages:+; }missing $subject"
        suggestions="${suggestions}${suggestions:+; }$detail"
        ;;
      MISSING_MANUAL)
        messages="${messages}${messages:+; }missing $subject"
        suggestions="${suggestions}${suggestions:+; }see $detail"
        ;;
      BACKEND_INVALID)
        messages="${messages}${messages:+; }invalid backend $subject (known: $detail)"
        suggestions="${suggestions}${suggestions:+; }correct config/backend or MX_BACKEND"
        ;;
    esac
  done < <(mx_probe_tool_records "$backend")
  if [ "$count" -gt 0 ]; then
    doctor_add FAIL "tools & environment" tools "$messages" "$suggestions"
  else
    doctor_add OK "tools & environment" tools \
      "required tools are present and treehouse supports durable leases"
  fi
}

doctor_check_primary_tangle() {
  local record branch default
  record=$(mx_probe_tangle_record "$MX_ROOT")
  if [ -z "$record" ]; then
    doctor_add OK "tools & environment" primary-tangle \
      "primary checkout is on its default branch or is not a named primary checkout"
    return
  fi
  IFS=$'\t' read -r branch default <<EOF
$record
EOF
  doctor_add FAIL "tools & environment" primary-tangle \
    "primary checkout is on feature branch $branch (expected $default)" \
    "restore only from the owning session: git -C $MX_ROOT checkout $default"
}

doctor_default_compat_paths() {
  local legacy encoded
  legacy="$(dirname "$MX_ROOT")/Computer"
  encoded=$(printf '%s' "$legacy" | sed 's#/#-#g')
  printf '%s\n' "$legacy" "$HOME/.claude/projects/$encoded"
}

doctor_check_compat_symlinks() {
  local paths path count=0 bad=0 issues=''
  if [ "${MX_DOCTOR_COMPAT_PATHS+x}" = x ]; then
    paths=$MX_DOCTOR_COMPAT_PATHS
  else
    paths=$(doctor_default_compat_paths)
  fi
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    if [ -L "$path" ]; then
      count=$((count + 1))
      if [ ! -e "$path" ]; then
        bad=$((bad + 1))
        issues="${issues}${issues:+; }dangling compatibility link $path"
      fi
    elif [ -e "$path" ]; then
      count=$((count + 1))
      bad=$((bad + 1))
      issues="${issues}${issues:+; }compatibility path $path exists but is not a symlink"
    fi
  done <<EOF
$paths
EOF
  if [ "$bad" -gt 0 ]; then
    doctor_add WARN "tools & environment" compat-symlinks "$issues" \
      "repair with ln -sfn or remove the retired compatibility path manually"
  else
    doctor_add OK "tools & environment" compat-symlinks \
      "$count present compatibility link(s) resolve; absent retired links are healthy"
  fi
}

doctor_lock_signature() {
  local lock=$1 link pid identity mtime
  link=$(readlink "$lock" 2>/dev/null || true)
  pid=$(cat "$lock/pid" 2>/dev/null || true)
  identity=$(cat "$lock/pid-identity" 2>/dev/null || true)
  mtime=$(mx_lock_path_mtime "$lock" 2>/dev/null || true)
  printf '%s\t%s\t%s\t%s\n' "$link" "$pid" "$identity" "$mtime"
}

doctor_fix_watcher_lock() {
  local lock="$STATE/.watch.lock" before after
  [ -e "$lock" ] || [ -L "$lock" ] || return 0
  [ -d "$lock" ] || return 0
  before=$(doctor_lock_signature "$lock")
  mx_lock_is_provably_stale "$lock" "$STATE" "$DOCTOR_LOCK_STALE_SECS" || return 0
  after=$(doctor_lock_signature "$lock")
  [ "$before" = "$after" ] || return 0
  if mx_lock_try_acquire "$lock"; then
    mx_lock_release "$lock"
    doctor_fix_log "cleared provably stale watcher lock $lock"
  fi
}

doctor_queue_lock_acquire_bounded() {
  local tries=0
  while [ "$tries" -lt 50 ]; do
    mx_lock_try_acquire "$MX_WAKE_QUEUE_LOCK" && return 0
    sleep 0.02 2>/dev/null || sleep 1
    tries=$((tries + 1))
  done
  return 1
}

doctor_fix_wake_queue() {
  local queue="$STATE/.wake-queue" temporary line rc pruned=0 candidate=0
  [ -f "$queue" ] && [ ! -L "$queue" ] || return 0
  while IFS= read -r line || [ -n "$line" ]; do
    if doctor_queue_line_state "$line"; then
      candidate=1
      break
    fi
  done <"$queue"
  [ "$candidate" -eq 1 ] || return 0
  doctor_queue_lock_acquire_bounded || return 0
  temporary=$(mktemp "$STATE/.wake-queue.doctor.XXXXXX" 2>/dev/null) || {
    mx_lock_release "$MX_WAKE_QUEUE_LOCK"
    return 0
  }
  while IFS= read -r line || [ -n "$line" ]; do
    if doctor_queue_line_state "$line"; then
      pruned=$((pruned + 1))
      continue
    fi
    rc=$?
    : "$rc"
    printf '%s\n' "$line" >>"$temporary"
  done <"$queue"
  if [ "$pruned" -gt 0 ]; then
    chmod 600 "$temporary" 2>/dev/null || true
    mv -f "$temporary" "$queue"
    doctor_fix_log "pruned $pruned wake queue row(s) whose task metadata is absent"
  else
    rm -f "$temporary"
  fi
  mx_lock_release "$MX_WAKE_QUEUE_LOCK"
}

CHECK_REGISTRY='watcher-lock|locks & liveness|doctor_check_watcher_lock|doctor_fix_watcher_lock
watcher-beacon|locks & liveness|doctor_check_watcher_beacon|
orphan-worktrees|tasks & worktrees|doctor_check_orphan_worktrees|
dangling-pids|tasks & worktrees|doctor_check_dangling_pids|
stateless-sessions|tasks & worktrees|doctor_check_stateless_sessions|
wake-queue-orphans|queues, holds & runs|doctor_check_wake_queue_orphans|doctor_fix_wake_queue
open-holds|queues, holds & runs|doctor_check_open_holds|
dispatch-queue-age|queues, holds & runs|doctor_check_dispatch_queue_age|
gate-runs|queues, holds & runs|doctor_check_gate_runs|
workflow-runs|queues, holds & runs|doctor_check_workflow_runs|
orphan-servers|queues, holds & runs|doctor_check_orphan_servers|
tools|tools & environment|doctor_check_tools|
primary-tangle|tools & environment|doctor_check_primary_tangle|
compat-symlinks|tools & environment|doctor_check_compat_symlinks|'

if [ -n "$SELECTED_CHECK" ]; then
  if ! printf '%s\n' "$CHECK_REGISTRY" | cut -d'|' -f1 | grep -Fx "$SELECTED_CHECK" >/dev/null; then
    die_usage "unknown check: $SELECTED_CHECK"
  fi
fi

while IFS='|' read -r check_name _category check_function fix_function; do
  [ -n "$check_name" ] || continue
  [ -z "$SELECTED_CHECK" ] || [ "$SELECTED_CHECK" = "$check_name" ] || continue
  if [ "$FIX_MODE" -eq 1 ] && [ -n "$fix_function" ]; then
    "$fix_function"
  fi
  "$check_function"
done <<EOF
$CHECK_REGISTRY
EOF

OK_COUNT=$(awk -F '\t' '$1 == "OK" {count++} END {print count+0}' "$RESULTS")
WARN_COUNT=$(awk -F '\t' '$1 == "WARN" {count++} END {print count+0}' "$RESULTS")
FAIL_COUNT=$(awk -F '\t' '$1 == "FAIL" {count++} END {print count+0}' "$RESULTS")
if [ "$FAIL_COUNT" -gt 0 ]; then
  EXIT_CODE=2
  WORST=FAIL
elif [ "$WARN_COUNT" -gt 0 ]; then
  EXIT_CODE=1
  WORST=WARN
else
  EXIT_CODE=0
  WORST=OK
fi

doctor_render_human() {
  local severity category name message suggestion fixable last_category=''
  while IFS=$'\t' read -r severity category name message suggestion fixable; do
    if [ "$category" != "$last_category" ]; then
      [ -z "$last_category" ] || printf '\n'
      printf '== %s ==\n' "$category"
      last_category=$category
    fi
    printf '%-5s %-24s %s\n' "$severity" "$name" "$message"
    [ -z "$suggestion" ] || printf '      %-24s -> suggest: %s\n' "" "$suggestion"
    : "$fixable"
  done <"$RESULTS"
  if [ -s "$FIXES" ]; then
    printf '\n== fixes applied ==\n'
    while IFS= read -r message; do
      printf 'FIXED %s\n' "$message"
    done <"$FIXES"
  fi
  printf '\nsummary: %s OK · %s WARN · %s FAIL          exit %s\n' \
    "$OK_COUNT" "$WARN_COUNT" "$FAIL_COUNT" "$EXIT_CODE"
}

doctor_render_json_node() {
  node - "$RESULTS" "$FIXES" "$WORST" "$EXIT_CODE" \
    "$OK_COUNT" "$WARN_COUNT" "$FAIL_COUNT" <<'NODE'
const fs = require("node:fs");
const [resultsPath, fixesPath, worst, exitCode, ok, warn, fail] = process.argv.slice(2);
const findings = fs.readFileSync(resultsPath, "utf8").split("\n").filter(Boolean).map(line => {
  const [severity, category, name, message, suggestion, fixable] = line.split("\t");
  return {severity, category, name, message, suggestion: suggestion || null, fixable: fixable === "true"};
});
const fixes = fs.readFileSync(fixesPath, "utf8").split("\n").filter(Boolean);
process.stdout.write(JSON.stringify({
  schema: "mx-doctor.v1",
  worst_severity: worst,
  exit_code: Number(exitCode),
  summary: {ok: Number(ok), warn: Number(warn), fail: Number(fail)},
  findings,
  fixes
}, null, 2) + "\n");
NODE
}

doctor_render_json_python() {
  python3 - "$RESULTS" "$FIXES" "$WORST" "$EXIT_CODE" \
    "$OK_COUNT" "$WARN_COUNT" "$FAIL_COUNT" <<'PY'
import json
import sys

results_path, fixes_path, worst, exit_code, ok, warn, fail = sys.argv[1:]
findings = []
with open(results_path, encoding="utf-8") as source:
    for line in source:
        if not line.rstrip("\n"):
            continue
        severity, category, name, message, suggestion, fixable = line.rstrip("\n").split("\t")
        findings.append({
            "severity": severity,
            "category": category,
            "name": name,
            "message": message,
            "suggestion": suggestion or None,
            "fixable": fixable == "true",
        })
with open(fixes_path, encoding="utf-8") as source:
    fixes = [line.rstrip("\n") for line in source if line.rstrip("\n")]
print(json.dumps({
    "schema": "mx-doctor.v1",
    "worst_severity": worst,
    "exit_code": int(exit_code),
    "summary": {"ok": int(ok), "warn": int(warn), "fail": int(fail)},
    "findings": findings,
    "fixes": fixes,
}, indent=2))
PY
}

if [ "$OUTPUT_JSON" -eq 1 ]; then
  if command -v node >/dev/null 2>&1; then
    doctor_render_json_node
  elif command -v python3 >/dev/null 2>&1; then
    doctor_render_json_python
  else
    printf 'mx-doctor: --json requires node or python3\n' >&2
    exit 2
  fi
else
  doctor_render_human
fi
exit "$EXIT_CODE"
