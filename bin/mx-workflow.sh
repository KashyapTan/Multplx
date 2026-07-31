#!/usr/bin/env bash
# Validate, launch, inspect, resume, abort, or dry-run a linear workflow.
#
# Usage:
#   mx-workflow.sh validate <definition.workflow.md>
#   mx-workflow.sh run <name|definition.workflow.md> --input <text>
#                  [--id <run-id>] [--repo <project-root>]
#   mx-workflow.sh status <run-id>
#   mx-workflow.sh resume <run-id>
#   mx-workflow.sh abort <run-id>
#   mx-workflow.sh dry-run <name|definition.workflow.md> [--input <text>]
#
# Definitions are validated as drafts from any path, but `run` accepts only a
# repo-tracked file under workflows/. Every run receives a private launch-time
# snapshot under state/<run-id>.workflow; resume never rereads the tracked file.
# Exact schema and approval-routing details live in docs/workflows.md.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
WF_SCRIPT_DIR=$SCRIPT_DIR
WF_MX_ROOT=$MX_ROOT
export WF_SCRIPT_DIR WF_MX_ROOT

# shellcheck source=bin/mx-journal-lib.sh
. "$SCRIPT_DIR/mx-journal-lib.sh"

wf_journal_stage_entered() { # <run-dir> <stage-id>
  local run_dir=$1 stage=$2 run detail
  run=$(jq -r '.run' "$run_dir/run.json" 2>/dev/null || true)
  if detail=$(jq -cn --arg run "$run" --arg stage "$stage" \
      '{run:$run,stage:$stage}' 2>/dev/null); then
    MX_STATE_OVERRIDE="$STATE" MX_JOURNAL_SOURCE=mx-workflow \
      mx_journal_try "$run" workflow.stage.entered "$detail"
  else
    mx_journal_warn_once "could not compose workflow.stage.entered for $run"
  fi
  return 0
}

wf_journal_stage_gated() { # <run-dir> <stage-id> <gate> <outcome>
  local run_dir=$1 stage=$2 gate=$3 outcome=$4 run detail
  run=$(jq -r '.run' "$run_dir/run.json" 2>/dev/null || true)
  if detail=$(jq -cn --arg run "$run" --arg stage "$stage" \
      --arg gate "$gate" --arg outcome "$outcome" \
      '{run:$run,stage:$stage,gate:$gate,outcome:$outcome}' 2>/dev/null); then
    MX_STATE_OVERRIDE="$STATE" MX_JOURNAL_SOURCE=mx-workflow \
      mx_journal_try "$run" workflow.stage.gated "$detail"
  else
    mx_journal_warn_once "could not compose workflow.stage.gated for $run"
  fi
  return 0
}

# shellcheck source=bin/mx-workflow-lib.sh
. "$SCRIPT_DIR/mx-workflow-lib.sh"
# shellcheck source=bin/mx-backlog-lib.sh
. "$SCRIPT_DIR/mx-backlog-lib.sh"

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  wf_error "$*"
  exit 1
}

definition_path() { # <name-or-path>
  local requested=$1 candidate
  case "$requested" in
    */*|*.workflow.md) candidate=$requested ;;
    *) candidate="$MX_ROOT/workflows/$requested.workflow.md" ;;
  esac
  [ -f "$candidate" ] || fail "definition not found: $candidate"
  printf '%s\n' "$(cd "$(dirname "$candidate")" && pwd -P)/$(basename "$candidate")"
}

run_dir() { # <run-id>
  wf_slug_valid "$1" || fail "invalid run id: $1"
  [ -d "$STATE/$1.workflow" ] && [ ! -L "$STATE/$1.workflow" ] \
    || fail "workflow run not found or unsafe: $1"
  printf '%s\n' "$STATE/$1.workflow"
}

workflow_lock_acquire() { # <run-dir>
  local directory=$1
  # shellcheck source=bin/mx-wake-lib.sh
  if ! command -v mx_lock_try_acquire >/dev/null 2>&1; then
    . "$SCRIPT_DIR/mx-wake-lib.sh"
  fi
  mx_lock_try_acquire "$directory/.reconcile.lock" || {
    wf_error "workflow run is already being reconciled"
    return 1
  }
}

workflow_lock_release() { # <run-dir>
  mx_lock_release "$1/.reconcile.lock"
}

workflow_reconcile_locked() { # <run-dir>
  local directory=$1 rc
  workflow_lock_acquire "$directory" || return 1
  wf_reconcile_run "$directory"
  rc=$?
  workflow_lock_release "$directory"
  return "$rc"
}

generated_run_id() { # <workflow-name>
  local suffix
  if [ -n "${MX_WORKFLOW_RUN_ID:-}" ]; then
    printf '%s\n' "$MX_WORKFLOW_RUN_ID"
    return
  fi
  suffix=$(printf '%04x' "$(( (RANDOM + $$) % 65536 ))")
  printf '%s-%s-%s\n' "$1" "$(date -u +%Y%m%d%H%M%S)" "$suffix"
}

command_validate() {
  [ "$#" -eq 1 ] || { usage >&2; exit 2; }
  local definition json
  definition=$(definition_path "$1")
  json=$(mktemp "${TMPDIR:-/tmp}/mx-workflow-validate.XXXXXX")
  trap "rm -f '$json'" EXIT
  wf_definition_json "$definition" >"$json" || exit 1
  printf 'valid: %s (%s stages)\n' \
    "$(jq -r '.name' "$json")" "$(jq -r '.stages | length' "$json")"
}

command_run() {
  [ "$#" -ge 1 ] || { usage >&2; exit 2; }
  local requested=$1 definition input='' id='' repo run_name created
  shift
  repo=$(pwd -P)
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --input) shift; input=${1-} ;;
      --id) shift; id=${1-} ;;
      --repo) shift; repo=${1-} ;;
      *) usage >&2; exit 2 ;;
    esac
    shift
  done
  [ -n "$input" ] || fail "--input is required and must not be empty"
  definition=$(definition_path "$requested")
  wf_definition_json "$definition" >/dev/null || exit 1
  wf_definition_tracked "$MX_ROOT" "$definition" || exit 1
  repo=$(cd "$repo" && pwd -P) || fail "repo is unavailable: $repo"
  git -C "$repo" rev-parse --show-toplevel >/dev/null 2>&1 \
    || fail "repo is not a git worktree: $repo"
  run_name=$(wf_definition_json "$definition" | jq -r '.name')
  [ -n "$id" ] || id=$(generated_run_id "$run_name")
  mkdir -p "$STATE" "$DATA"
  created=$(wf_create_run "$definition" "$id" "$input" "$repo" "$MX_HOME" "$STATE") \
    || exit 1
  printf 'launched: %s\n' "$id"
  if ! workflow_reconcile_locked "$created"; then
    wf_status_render "$created"
    exit 1
  fi
  wf_status_render "$created"
}

command_status() {
  [ "$#" -eq 1 ] || { usage >&2; exit 2; }
  wf_status_render "$(run_dir "$1")"
}

command_resume() {
  [ "$#" -eq 1 ] || { usage >&2; exit 2; }
  local directory
  directory=$(run_dir "$1")
  if ! workflow_reconcile_locked "$directory"; then
    wf_status_render "$directory"
    exit 1
  fi
  wf_status_render "$directory"
}

command_abort() {
  [ "$#" -eq 1 ] || { usage >&2; exit 2; }
  local directory status
  directory=$(run_dir "$1")
  workflow_lock_acquire "$directory" || exit 1
  status=$(jq -r '.status' "$directory/run.json")
  if [ "$status" = completed ]; then
    workflow_lock_release "$directory"
    fail "completed run cannot be aborted"
  fi
  if [ "$status" = aborted ]; then
    workflow_lock_release "$directory"
    fail "run is already aborted"
  fi
  wf_run_set_state "$directory" aborted "$(jq -r '.current_stage // ""' "$directory/run.json")" \
    "workflow permanently aborted" || {
      workflow_lock_release "$directory"
      exit 1
    }
  if [ -f "$DATA/backlog.md" ]; then
    mx_backlog_hold "$DATA/backlog.md" "$1" --reason "workflow aborted" \
      --kind workflow >/dev/null || {
        workflow_lock_release "$directory"
        fail "could not park aborted workflow in backlog"
      }
  fi
  workflow_lock_release "$directory"
  printf 'aborted: %s\n' "$1"
}

command_dry_run() {
  [ "$#" -ge 1 ] || { usage >&2; exit 2; }
  local requested=$1 definition input='example input' json stage output
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --input) shift; input=${1-} ;;
      *) usage >&2; exit 2 ;;
    esac
    shift
  done
  definition=$(definition_path "$requested")
  json=$(mktemp "${TMPDIR:-/tmp}/mx-workflow-dry-run.XXXXXX")
  trap "rm -f '$json'" EXIT
  wf_definition_json "$definition" >"$json" || exit 1
  printf 'workflow: %s\n' "$(jq -r '.name' "$json")"
  printf 'input: %s\n' "$input"
  jq -c '.stages[]' "$json" | while IFS= read -r stage; do
    output=$(printf '%s\n' "$stage" | jq -r '.output // empty')
    [ -z "$output" ] || output=$(wf_substitute "$output" dry-run "$input" "")
    printf '%s | type=%s | gate=%s | executor=%s | output=%s\n' \
      "$(printf '%s\n' "$stage" | jq -r '.id')" \
      "$(printf '%s\n' "$stage" | jq -r '.type')" \
      "$(printf '%s\n' "$stage" | jq -r '.gate')" \
      "$(printf '%s\n' "$stage" | jq -r '.executor // "-"')" \
      "${output:--}"
  done
}

case "${1:-}" in
  validate) shift; command_validate "$@" ;;
  run) shift; command_run "$@" ;;
  status) shift; command_status "$@" ;;
  resume) shift; command_resume "$@" ;;
  abort) shift; command_abort "$@" ;;
  dry-run) shift; command_dry_run "$@" ;;
  -h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
