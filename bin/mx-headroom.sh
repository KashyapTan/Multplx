#!/usr/bin/env bash
# Composite dispatch capacity and durable parked-dispatch queue.
#
# `--json` combines local CPU/load and available-memory headroom with a
# conservative API budget. The tighter signal bounds dispatch. The API budget
# defaults to one concurrent request and may be set with config/api-capacity or
# MX_HEADROOM_API_CAPACITY. Per-harness config/api-capacity-<harness> files
# refine candidate detail while the global budget remains the upper bound.
# This is intentionally reported as `configured-budget`, not live provider
# quota. Unreadable local signals or malformed configured budgets are errors.
#
# Local spare slots are min(
#   floor((logical CPUs - one-minute load) / MX_HEADROOM_CPU_PER_ACTOR),
#   floor(available bytes / MX_HEADROOM_MEM_PER_ACTOR_BYTES)
# ).
# Defaults are two logical CPUs and 2 GiB per new actor.
#
# Queue records live at state/.dispatch-queue/<task-id>.request.
# Each is a private, one-line-field record with task id, project, requested
# harness/model/effort/backend/kind, and enqueue epoch. Queue add is idempotent,
# inspection is FIFO, cancel targets one exact id, and drain launches at most
# one oldest entry after a fresh capacity check. Records survive restarts.
#
# Usage:
#   mx-headroom.sh --json
#   mx-headroom.sh --queue
#   mx-headroom.sh --queue-add <id> <project> [profile flags]
#   mx-headroom.sh --queue-cancel <id>
#   mx-headroom.sh --queue-drain
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
CONFIG="${MX_CONFIG_OVERRIDE:-$MX_HOME/config}"
PROC_ROOT="${MX_HEADROOM_PROC_ROOT:-/proc}"
QUEUE_DIR="$STATE/.dispatch-queue"

# shellcheck source=bin/mx-backend.sh
. "$SCRIPT_DIR/mx-backend.sh"

fail() {
  printf 'mx-headroom: %s\n' "$*" >&2
  exit 1
}

valid_nonnegative_integer() {
  case "$1" in ''|*[!0-9]*) return 1 ;; esac
}

valid_positive_number() {
  printf '%s\n' "$1" | awk '
    /^[0-9]+([.][0-9]+)?$/ { if (($0 + 0) > 0) ok = 1 }
    END { exit ok ? 0 : 1 }
  '
}

valid_nonnegative_number() {
  printf '%s\n' "$1" | awk '
    /^[0-9]+([.][0-9]+)?$/ { ok = 1 }
    END { exit ok ? 0 : 1 }
  '
}

read_cpu_count() {
  if [ -n "${MX_HEADROOM_CPU_COUNT:-}" ]; then
    valid_positive_number "$MX_HEADROOM_CPU_COUNT" || return 1
    printf '%s\n' "$MX_HEADROOM_CPU_COUNT"
    return
  fi
  case "${MX_HEADROOM_PLATFORM:-$(uname -s)}" in
    Darwin) sysctl -n hw.logicalcpu 2>/dev/null ;;
    Linux)
      awk '/^processor[[:space:]]*:/ { count++ } END { if (count > 0) print count; else exit 1 }' \
        "$PROC_ROOT/cpuinfo" 2>/dev/null
      ;;
    *) return 1 ;;
  esac
}

read_load_one() {
  if [ -n "${MX_HEADROOM_LOAD1:-}" ]; then
    valid_nonnegative_number "$MX_HEADROOM_LOAD1" || return 1
    printf '%s\n' "$MX_HEADROOM_LOAD1"
    return
  fi
  case "${MX_HEADROOM_PLATFORM:-$(uname -s)}" in
    Darwin)
      sysctl -n vm.loadavg 2>/dev/null | awk '{
        for (i = 1; i <= NF; i++) {
          value = $i
          gsub(/[{}]/, "", value)
          if (value ~ /^[0-9]+([.][0-9]+)?$/) { print value; exit }
        }
        exit 1
      }'
      ;;
    Linux) awk '{ print $1 }' "$PROC_ROOT/loadavg" 2>/dev/null ;;
    *) return 1 ;;
  esac
}

read_memory_available() {
  if [ -n "${MX_HEADROOM_MEM_AVAILABLE_BYTES:-}" ]; then
    valid_nonnegative_integer "$MX_HEADROOM_MEM_AVAILABLE_BYTES" || return 1
    printf '%s\n' "$MX_HEADROOM_MEM_AVAILABLE_BYTES"
    return
  fi
  case "${MX_HEADROOM_PLATFORM:-$(uname -s)}" in
    Darwin)
      vm_stat 2>/dev/null | awk '
        NR == 1 {
          if (match($0, /page size of [0-9]+ bytes/)) {
            value = substr($0, RSTART, RLENGTH)
            gsub(/[^0-9]/, "", value)
            page = value + 0
          }
          next
        }
        /Pages free:|Pages inactive:|Pages speculative:|Pages purgeable:/ {
          value = $NF
          gsub(/[^0-9]/, "", value)
          pages += value + 0
        }
        END {
          if (page > 0) printf "%.0f\n", page * pages
          else exit 1
        }
      '
      ;;
    Linux)
      awk '/^MemAvailable:[[:space:]]+/ { printf "%.0f\n", $2 * 1024; found = 1; exit }
        END { if (!found) exit 1 }' "$PROC_ROOT/meminfo" 2>/dev/null
      ;;
    *) return 1 ;;
  esac
}

meta_is_live() {
  local meta=$1 backend target label
  [ -f "$meta" ] && [ ! -L "$meta" ] || return 1
  target=$(mx_backend_target_of_meta "$meta" 2>/dev/null || true)
  [ -n "$target" ] || return 1
  backend=$(mx_backend_of_meta "$meta" 2>/dev/null || true)
  [ -n "$backend" ] || return 1
  label=${target##*:}
  mx_backend_target_exists "$backend" "$target" "$label" >/dev/null 2>&1
}

in_use_total() {
  local meta count=0
  if [ -n "${MX_HEADROOM_IN_USE:-}" ]; then
    valid_nonnegative_integer "$MX_HEADROOM_IN_USE" || return 1
    printf '%s\n' "$MX_HEADROOM_IN_USE"
    return
  fi
  for meta in "$STATE"/*.meta; do
    [ -e "$meta" ] || continue
    [ "$(sed -n 's/^kind=//p' "$meta" | tail -1)" != daemon ] || continue
    if meta_is_live "$meta"; then count=$((count + 1)); fi
  done
  printf '%s\n' "$count"
}

in_use_harness() {
  local wanted=$1 meta harness count=0
  if [ -n "${MX_HEADROOM_IN_USE:-}" ]; then
    printf '%s\n' "$MX_HEADROOM_IN_USE"
    return
  fi
  for meta in "$STATE"/*.meta; do
    [ -e "$meta" ] || continue
    [ "$(sed -n 's/^kind=//p' "$meta" | tail -1)" != daemon ] || continue
    harness=$(sed -n 's/^harness=//p' "$meta" | tail -1)
    [ "${harness:-default}" = "$wanted" ] || continue
    if meta_is_live "$meta"; then count=$((count + 1)); fi
  done
  printf '%s\n' "$count"
}

configured_api_capacity() {
  local candidate=${1:-} file value
  if [ -n "$candidate" ] && [ -f "$CONFIG/api-capacity-$candidate" ]; then
    file="$CONFIG/api-capacity-$candidate"
    value=$(tr -d '[:space:]' < "$file")
  elif [ -n "${MX_HEADROOM_API_CAPACITY:-}" ]; then
    value=$MX_HEADROOM_API_CAPACITY
  elif [ -f "$CONFIG/api-capacity" ]; then
    value=$(tr -d '[:space:]' < "$CONFIG/api-capacity")
  else
    value=1
  fi
  valid_nonnegative_integer "$value" || return 1
  printf '%s\n' "$value"
}

configured_candidates() {
  local harness
  if [ "${MX_HEADROOM_IGNORE_DISPATCH_CONFIG:-0}" = 1 ]; then
    printf '%s\n' default
    return
  fi
  if [ -f "$CONFIG/actor-dispatch.json" ]; then
    command -v jq >/dev/null 2>&1 || return 1
    jq -r '
      def profiles($value):
        if ($value | type) == "array" then $value[]
        elif ($value | type) == "object" then $value
        else empty
        end;
      ([.rules[]? | profiles(.use) | .harness]
       + [if has("default") then profiles(.default) | .harness else empty end])
      | unique[]
    ' "$CONFIG/actor-dispatch.json" 2>/dev/null
    return
  fi
  if [ -f "$CONFIG/actor-harness" ]; then
    harness=$(awk 'NF { print $1; exit }' "$CONFIG/actor-harness")
    [ -n "$harness" ] || return 1
    printf '%s\n' "$harness"
  else
    printf '%s\n' default
  fi
}

collect_headroom() {
  local cpu_count load_one memory_available in_use cpu_per_actor memory_per_actor
  local cpu_slots memory_slots local_available global_capacity global_available
  local candidate candidates candidate_capacity candidate_in_use candidate_available
  local candidate_max=-1 candidate_json='' comma='' overall_available

  cpu_count=$(read_cpu_count) || fail "CPU capacity signal is unreadable"
  load_one=$(read_load_one) || fail "one-minute load signal is unreadable"
  memory_available=$(read_memory_available) || fail "available-memory signal is unreadable"
  in_use=$(in_use_total) || fail "live actor count is unreadable"
  cpu_per_actor=${MX_HEADROOM_CPU_PER_ACTOR:-2}
  memory_per_actor=${MX_HEADROOM_MEM_PER_ACTOR_BYTES:-2147483648}
  valid_positive_number "$cpu_per_actor" || fail "MX_HEADROOM_CPU_PER_ACTOR must be positive"
  valid_nonnegative_integer "$memory_per_actor" || fail "MX_HEADROOM_MEM_PER_ACTOR_BYTES must be an integer"
  [ "$memory_per_actor" -gt 0 ] || fail "MX_HEADROOM_MEM_PER_ACTOR_BYTES must be positive"

  cpu_slots=$(awk -v cpus="$cpu_count" -v current_load="$load_one" -v unit="$cpu_per_actor" \
    'BEGIN { slots = int((cpus - current_load) / unit); if (slots < 0) slots = 0; print slots }')
  memory_slots=$(awk -v bytes="$memory_available" -v unit="$memory_per_actor" \
    'BEGIN { slots = int(bytes / unit); if (slots < 0) slots = 0; print slots }')
  if [ "$cpu_slots" -lt "$memory_slots" ]; then local_available=$cpu_slots; else local_available=$memory_slots; fi

  global_capacity=$(configured_api_capacity) || fail "configured API capacity is invalid"
  global_available=$((global_capacity - in_use))
  [ "$global_available" -ge 0 ] || global_available=0
  candidates=$(configured_candidates) || fail "configured dispatch candidates are unreadable"
  [ -n "$candidates" ] || fail "configured dispatch candidates are empty"
  while IFS= read -r candidate; do
    [ -n "$candidate" ] || continue
    case "$candidate" in *[!A-Za-z0-9._-]*) fail "invalid configured candidate: $candidate" ;; esac
    candidate_capacity=$(configured_api_capacity "$candidate") \
      || fail "configured API capacity for $candidate is invalid"
    candidate_in_use=$(in_use_harness "$candidate") || fail "usage count for $candidate is unreadable"
    candidate_available=$((candidate_capacity - candidate_in_use))
    [ "$candidate_available" -ge 0 ] || candidate_available=0
    [ "$candidate_available" -le "$global_available" ] || candidate_available=$global_available
    [ "$candidate_available" -le "$candidate_max" ] || candidate_max=$candidate_available
    candidate_json="${candidate_json}${comma}\"$candidate\":{\"capacity\":$candidate_capacity,\"in_use\":$candidate_in_use,\"available\":$candidate_available,\"window\":\"configured-budget\"}"
    comma=,
  done <<EOF
$candidates
EOF
  [ "$candidate_max" -ge 0 ] || fail "configured dispatch candidates are empty"
  if [ "$local_available" -lt "$candidate_max" ]; then
    overall_available=$local_available
  else
    overall_available=$candidate_max
  fi

  HEADROOM_CPU_COUNT=$cpu_count
  HEADROOM_LOAD_ONE=$load_one
  HEADROOM_MEMORY_AVAILABLE=$memory_available
  HEADROOM_IN_USE=$in_use
  HEADROOM_LOCAL_AVAILABLE=$local_available
  HEADROOM_API_CAPACITY=$global_capacity
  HEADROOM_API_AVAILABLE=$global_available
  HEADROOM_AVAILABLE=$overall_available
  HEADROOM_CAPACITY=$((in_use + overall_available))
  HEADROOM_CANDIDATES=$candidate_json
}

print_json() {
  collect_headroom
  if [ "$HEADROOM_AVAILABLE" -eq 0 ]; then at_limit=true; else at_limit=false; fi
  printf '{"model":"local+api","capacity":%s,"in_use":%s,"available":%s,"at_limit":%s,' \
    "$HEADROOM_CAPACITY" "$HEADROOM_IN_USE" "$HEADROOM_AVAILABLE" "$at_limit"
  printf '"local":{"cpu_count":%s,"load_one":%s,"memory_available_bytes":%s,"available":%s},' \
    "$HEADROOM_CPU_COUNT" "$HEADROOM_LOAD_ONE" "$HEADROOM_MEMORY_AVAILABLE" "$HEADROOM_LOCAL_AVAILABLE"
  printf '"api":{"source":"configured-budget","capacity":%s,"available":%s},' \
    "$HEADROOM_API_CAPACITY" "$HEADROOM_API_AVAILABLE"
  printf '"candidates":{%s}}\n' "$HEADROOM_CANDIDATES"
}

validate_queue_id() {
  case "$1" in ''|*[!A-Za-z0-9._-]*) fail "invalid queue task id: $1" ;; esac
}

validate_one_line() {
  local label=$1 value=$2
  [ -n "$value" ] || fail "$label must not be empty"
  case "$value" in *$'\n'*|*$'\r'*) fail "$label must be one line" ;; esac
}

queue_record_value() {
  sed -n "s/^$2=//p" "$1" | tail -1
}

queue_record_mode() {
  if [ "$(uname -s 2>/dev/null)" = Darwin ]; then
    stat -f '%Lp' "$1" 2>/dev/null
  else
    stat -c '%a' "$1" 2>/dev/null
  fi
}

validate_queue_record() {
  local record=$1 expected_id=${2:-} id project harness model effort backend kind enqueued version mode value
  [ -f "$record" ] && [ ! -L "$record" ] || fail "queue record is not a regular private file: $record"
  mode=$(queue_record_mode "$record") || fail "queue record mode is unreadable: $record"
  [ "$mode" = 600 ] || fail "queue record mode must be 0600: $record"
  version=$(queue_record_value "$record" version)
  id=$(queue_record_value "$record" task_id)
  project=$(queue_record_value "$record" project)
  harness=$(queue_record_value "$record" harness)
  model=$(queue_record_value "$record" model)
  effort=$(queue_record_value "$record" effort)
  backend=$(queue_record_value "$record" backend)
  kind=$(queue_record_value "$record" kind)
  enqueued=$(queue_record_value "$record" enqueued_at)
  [ "$version" = 1 ] || fail "queue record has an unsupported version: $record"
  validate_queue_id "$id"
  [ -z "$expected_id" ] || [ "$id" = "$expected_id" ] \
    || fail "queue record identity does not match its path: $record"
  validate_one_line project "$project"
  for value in "$harness" "$model" "$effort" "$backend"; do
    [ -z "$value" ] || validate_one_line "queue profile value" "$value"
  done
  case "$kind" in delivery|scout) ;; *) fail "queue record has invalid kind: $record" ;; esac
  valid_nonnegative_integer "$enqueued" || fail "queue record has invalid enqueue time: $record"
}

queue_add() {
  local id=$1 project=$2
  shift 2
  local harness='' model='' effort='' backend='' kind=delivery want='' argument record temporary now
  validate_queue_id "$id"
  validate_one_line project "$project"
  for argument in "$@"; do
    if [ -n "$want" ]; then
      case "$want" in
        harness) harness=$argument ;;
        model) model=$argument ;;
        effort) effort=$argument ;;
        backend) backend=$argument ;;
      esac
      want=
      continue
    fi
    case "$argument" in
      --harness) want=harness ;;
      --model) want=model ;;
      --effort) want=effort ;;
      --backend) want=backend ;;
      --scout) kind=scout ;;
      *) fail "unknown queue profile argument: $argument" ;;
    esac
  done
  [ -z "$want" ] || fail "--$want requires a value"
  for argument in "$harness" "$model" "$effort" "$backend"; do
    [ -z "$argument" ] || validate_one_line "profile value" "$argument"
  done
  mkdir -p "$QUEUE_DIR"
  chmod 0700 "$QUEUE_DIR" 2>/dev/null || true
  record="$QUEUE_DIR/$id.request"
  if [ -f "$record" ]; then
    validate_queue_record "$record" "$id"
    [ "$(queue_record_value "$record" project)" = "$project" ] \
      && [ "$(queue_record_value "$record" harness)" = "$harness" ] \
      && [ "$(queue_record_value "$record" model)" = "$model" ] \
      && [ "$(queue_record_value "$record" effort)" = "$effort" ] \
      && [ "$(queue_record_value "$record" backend)" = "$backend" ] \
      && [ "$(queue_record_value "$record" kind)" = "$kind" ] \
      || fail "queued dispatch $id already exists with a different request"
    printf 'queued: %s already parked\n' "$id"
    return
  fi
  [ ! -e "$record" ] && [ ! -L "$record" ] || fail "queue record path is unsafe: $record"
  now=$(date +%s)
  temporary="$QUEUE_DIR/.$id.tmp.$$"
  umask 077
  {
    printf 'version=1\n'
    printf 'task_id=%s\n' "$id"
    printf 'project=%s\n' "$project"
    printf 'harness=%s\n' "$harness"
    printf 'model=%s\n' "$model"
    printf 'effort=%s\n' "$effort"
    printf 'backend=%s\n' "$backend"
    printf 'kind=%s\n' "$kind"
    printf 'enqueued_at=%s\n' "$now"
  } > "$temporary"
  mv "$temporary" "$record"
  printf 'queued: %s parked until dispatch capacity is available\n' "$id"
}

queue_list() {
  local record id project harness model effort backend kind enqueued
  [ -d "$QUEUE_DIR" ] || return 0
  for record in "$QUEUE_DIR"/*.request; do
    [ -e "$record" ] || continue
    id=${record##*/}
    id=${id%.request}
    validate_queue_record "$record" "$id"
    id=$(queue_record_value "$record" task_id)
    project=$(queue_record_value "$record" project)
    harness=$(queue_record_value "$record" harness)
    model=$(queue_record_value "$record" model)
    effort=$(queue_record_value "$record" effort)
    backend=$(queue_record_value "$record" backend)
    kind=$(queue_record_value "$record" kind)
    enqueued=$(queue_record_value "$record" enqueued_at)
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$enqueued" "$id" "$project" "${harness:--}" "${model:--}" "${effort:--}" "${backend:--}" "$kind"
  done | LC_ALL=C sort -n -k1,1 -k2,2
}

queue_cancel() {
  local id=$1 record
  validate_queue_id "$id"
  record="$QUEUE_DIR/$id.request"
  [ -e "$record" ] || [ -L "$record" ] || fail "queued dispatch not found: $id"
  validate_queue_record "$record" "$id"
  rm -f "$record"
  printf 'cancelled: %s\n' "$id"
}

queue_drain() {
  local oldest record id project harness model effort backend kind
  local -a spawn_args
  [ -d "$QUEUE_DIR" ] || return 0
  collect_headroom
  [ "$HEADROOM_AVAILABLE" -gt 0 ] || return 0
  oldest=$(queue_list | head -1)
  [ -n "$oldest" ] || return 0
  id=$(printf '%s\n' "$oldest" | cut -f2)
  validate_queue_id "$id"
  record="$QUEUE_DIR/$id.request"
  validate_queue_record "$record" "$id"
  project=$(queue_record_value "$record" project)
  harness=$(queue_record_value "$record" harness)
  model=$(queue_record_value "$record" model)
  effort=$(queue_record_value "$record" effort)
  backend=$(queue_record_value "$record" backend)
  kind=$(queue_record_value "$record" kind)
  validate_one_line project "$project"
  spawn_args=("$id" "$project")
  [ -z "$harness" ] || spawn_args+=(--harness "$harness")
  [ -z "$model" ] || spawn_args+=(--model "$model")
  [ -z "$effort" ] || spawn_args+=(--effort "$effort")
  [ -z "$backend" ] || spawn_args+=(--backend "$backend")
  case "$kind" in
    delivery) ;;
    scout) spawn_args+=(--scout) ;;
    *) fail "queued dispatch $id has invalid kind: $kind" ;;
  esac
  if MX_HEADROOM_SKIP_QUEUE=1 "${MX_HEADROOM_SPAWN_BIN:-$SCRIPT_DIR/mx-spawn.sh}" "${spawn_args[@]}"; then
    rm -f "$record"
    printf 'dispatch-queue: launched %s\n' "$id"
  else
    fail "queued dispatch $id could not be launched; record retained"
  fi
}

case "${1:-}" in
  --json)
    [ "$#" -eq 1 ] || fail "--json takes no arguments"
    print_json
    ;;
  --queue)
    [ "$#" -eq 1 ] || fail "--queue takes no arguments"
    queue_list
    ;;
  --queue-add)
    [ "$#" -ge 3 ] || fail "--queue-add requires task id and project"
    shift
    queue_add "$@"
    ;;
  --queue-cancel)
    [ "$#" -eq 2 ] || fail "--queue-cancel requires exactly one task id"
    queue_cancel "$2"
    ;;
  --queue-drain)
    [ "$#" -eq 1 ] || fail "--queue-drain takes no arguments"
    queue_drain
    ;;
  -h|--help)
    sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'
    ;;
  *) fail "usage: mx-headroom.sh --json|--queue|--queue-add|--queue-cancel|--queue-drain" ;;
esac
