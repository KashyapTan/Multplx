#!/usr/bin/env bash
# Print canonical JSON bindings for a subsystem-owned maintainer exception.
# Usage:
#   mx-override-bindings.sh cleanup <task-id>
#   mx-override-bindings.sh validation <task-id> <commit-sha>
#   mx-override-bindings.sh workflow-skip <run-id> <stage-id>
#   mx-override-bindings.sh workflow-reorder <run-id> <stage-id> <before-stage-id>
#   mx-override-bindings.sh single-checkout <task-id> <project-dir>
#   mx-override-bindings.sh terminate-owner <harness-pid>
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# Portion 10 Rust-default adapter.
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_authority_implementation) || exit $?
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd -P); export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" authority mx-override-bindings.sh "$@"
fi
MX_ROOT=${MX_ROOT_OVERRIDE:-$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd -P)}
MX_HOME=${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}
STATE=${MX_STATE_OVERRIDE:-$MX_HOME/state}
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$SCRIPT_DIR/mx-maintainer-override-lib.sh"

die() { printf 'mx-override-bindings: %s\n' "$*" >&2; exit 1; }

file_digest() {
  local file=$1
  if [ -f "$file" ] && [ ! -L "$file" ]; then
    if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$file" | awk '{print $1}'; else sha256sum "$file" | awk '{print $1}'; fi
  else
    printf 'absent\n'
  fi
}

project_slug() {
  local value=$1
  value=$(printf '%s' "$value" | tr -c 'A-Za-z0-9._-' '_')
  [ -n "$value" ] || value=unknown
  printf '%s\n' "$value"
}

emit() {
  local boundary=$1 task=$2 project=$3 operation=$4 target=$5 state_json=$6 consequence=$7 digest
  digest=$(mx_override_sha256_text "$state_json") || exit 1
  jq -cn --arg boundary "$boundary" --arg task "$task" --arg project "$project" \
    --arg operation "$operation" --arg target "$target" --arg expected_state_digest "$digest" \
    --arg consequence "$consequence" --argjson state "$state_json" \
    '{boundary:$boundary,task:$task,project:$project,operation:$operation,target:$target,expected_state_digest:$expected_state_digest,consequence:$consequence,state:$state}'
}

mode=${1:-}
case "$mode" in
  cleanup)
    [ "$#" -eq 2 ] || die "cleanup requires one task id"
    id=$2
    mx_override_slug_valid "$id" || die "invalid task id"
    meta=$STATE/$id.meta
    [ -f "$meta" ] && [ ! -L "$meta" ] || die "task metadata is unavailable"
    worktree=$(sed -n 's/^worktree=//p' "$meta" | head -1)
    project_path=$(sed -n 's/^project=//p' "$meta" | head -1)
    home_path=$(sed -n 's/^home=//p' "$meta" | head -1)
    kind=$(sed -n 's/^kind=//p' "$meta" | head -1)
    [ -n "$kind" ] || kind=delivery
    target=$worktree
    [ "$kind" != daemon ] || target=${home_path:-$worktree}
    [ -n "$target" ] || die "task cleanup target is unavailable"
    head=absent
    status_digest=absent
    if [ -d "$worktree" ]; then
      head=$(git -C "$worktree" rev-parse --verify HEAD 2>/dev/null || printf unreadable)
      status=$(git -C "$worktree" status --porcelain=v1 --untracked-files=all 2>/dev/null || printf unreadable)
      status_digest=$(mx_override_sha256_text "$status")
    fi
    child_inventory=absent
    if [ "$kind" = daemon ] && [ -d "$target/state" ]; then
      child_inventory=$(for file in "$target/state"/*.meta; do [ -f "$file" ] || continue; printf '%s:%s\n' "${file##*/}" "$(file_digest "$file")"; done | LC_ALL=C sort)
    fi
    state_json=$(jq -cn --arg meta "$(file_digest "$meta")" --arg head "$head" \
      --arg status "$status_digest" --arg ready "$(file_digest "$STATE/$id.ready-to-push")" \
      --arg report "$(file_digest "$MX_HOME/data/$id/report.md")" --arg children "$child_inventory" \
      '{meta_digest:$meta,head:$head,status_digest:$status,ready_to_push_digest:$ready,report_digest:$report,child_inventory:$children}')
    emit cleanup.discard-unlanded "$id" "$(project_slug "${project_path##*/}")" \
      "discard task resources for $id" "$target" "$state_json" \
      "Discard exactly the inventoried unlanded task material and retire only this task's resources."
    ;;
  validation)
    [ "$#" -eq 3 ] || die "validation requires task id and commit SHA"
    id=$2
    sha=$3
    mx_override_slug_valid "$id" || die "invalid task id"
    case "$sha" in [0-9a-f][0-9a-f]*) ;; *) die "invalid commit SHA" ;; esac
    gate=$STATE/$id.gate
    run=$gate/run.json
    [ -f "$run" ] && [ ! -L "$run" ] || die "gate run is unavailable"
    repo=$(jq -r '.worktree // empty' "$run")
    [ -n "$repo" ] || repo=$(sed -n 's/^worktree=//p' "$STATE/$id.meta" | head -1)
    state_json=$(jq -cn --arg run "$(file_digest "$run")" --arg sha "$sha" \
      --arg actual "$(git -C "$repo" rev-parse --verify HEAD 2>/dev/null || printf unreadable)" \
      '{gate_run_digest:$run,requested_sha:$sha,worktree_head:$actual}')
    emit validation.waive-gate "$id" "$(project_slug "${repo##*/}")" \
      "waive validation gate for $id at $sha" "$gate@$sha" "$state_json" \
      "Create a maintainer-waived delivery handoff for this exact SHA without recording validation as passed."
    ;;
  workflow-skip|workflow-reorder)
    if [ "$mode" = workflow-skip ]; then [ "$#" -eq 3 ] || die "workflow-skip requires run and stage"; else [ "$#" -eq 4 ] || die "workflow-reorder requires run, stage, and before-stage"; fi
    run_id=$2
    stage=$3
    before=${4:-}
    if ! mx_override_slug_valid "$run_id" || ! mx_override_slug_valid "$stage"; then
      die "invalid workflow identity"
    fi
    run_dir=$STATE/$run_id.workflow
    run=$run_dir/run.json
    snapshot=$run_dir/definition.json
    [ -f "$run" ] && [ ! -L "$run" ] && [ -f "$snapshot" ] && [ ! -L "$snapshot" ] || die "workflow run is unavailable"
    repo=$(jq -r '.repo // empty' "$run")
    state_json=$(jq -cn --arg run "$(file_digest "$run")" --arg definition "$(file_digest "$snapshot")" \
      --arg order "$(file_digest "$run_dir/stage-order.json")" --arg stage_record "$(file_digest "$run_dir/stages/$stage.json")" \
      --arg before_record "$(file_digest "$run_dir/stages/$before.json")" --arg stage "$stage" --arg before "$before" \
      '{run_digest:$run,definition_digest:$definition,order_digest:$order,stage_record_digest:$stage_record,before_record_digest:$before_record,stage:$stage,before_stage:$before}')
    if [ "$mode" = workflow-skip ]; then
      emit workflow.skip-stage "$run_id" "$(project_slug "${repo##*/}")" "skip workflow stage $stage in run $run_id" "$run_dir#$stage" "$state_json" "Skip only the named stage and preserve every other snapshotted stage."
    else
      emit workflow.reorder-stage "$run_id" "$(project_slug "${repo##*/}")" "move workflow stage $stage before $before in run $run_id" "$run_dir#$stage-before-$before" "$state_json" "Move only the named stage before the named target and preserve every other snapshotted stage."
    fi
    ;;
  single-checkout)
    [ "$#" -eq 3 ] || die "single-checkout requires task id and project directory"
    id=$2
    project_path=$3
    mx_override_slug_valid "$id" || die "invalid task id"
    project_path=$(cd "$project_path" 2>/dev/null && pwd -P) || die "project directory is unavailable"
    top=$(git -C "$project_path" rev-parse --show-toplevel 2>/dev/null || true)
    top=$(cd "$top" 2>/dev/null && pwd -P) || die "project is not a git checkout root"
    [ "$top" = "$project_path" ] || die "project is not a git checkout root"
    head=$(git -C "$project_path" rev-parse --verify HEAD 2>/dev/null || printf unreadable)
    branch=$(git -C "$project_path" symbolic-ref --quiet --short HEAD 2>/dev/null || printf detached)
    status=$(git -C "$project_path" status --porcelain=v1 --untracked-files=all 2>/dev/null || printf unreadable)
    active=$(for file in "$STATE"/*.meta; do
      [ -f "$file" ] && [ ! -L "$file" ] || continue
      [ "$(sed -n 's/^project=//p' "$file" | head -1)" = "$project_path" ] || continue
      printf '%s:%s\n' "${file##*/}" "$(file_digest "$file")"
    done | LC_ALL=C sort)
    reservation=$(file_digest "$STATE/.single-checkout-$(mx_override_sha256_text "$project_path").json")
    state_json=$(jq -cn --arg head "$head" --arg branch "$branch" \
      --arg status "$(mx_override_sha256_text "$status")" --arg active "$active" \
      --arg reservation "$reservation" \
      '{head:$head,branch:$branch,status_digest:$status,active_task_inventory:$active,reservation_digest:$reservation}')
    emit isolation.single-checkout "$id" "$(project_slug "${project_path##*/}")" \
      "launch task $id in serialized single-checkout mode" "$project_path" "$state_json" \
      "Run only this task in the named checkout, record the loss of isolation, and exclude another single-checkout task until teardown."
    ;;
  terminate-owner)
    [ "$#" -eq 2 ] || die "terminate-owner requires one harness pid"
    pid=$2
    case "$pid" in ''|*[!0-9]*) die "invalid harness pid" ;; esac
    lock=$STATE/.lock
    [ -f "$lock" ] && [ ! -L "$lock" ] || die "session lock is unavailable"
    [ "$(cat "$lock" 2>/dev/null)" = "$pid" ] || die "session lock owner changed"
    # shellcheck source=bin/mx-session-lock-lib.sh
    . "$SCRIPT_DIR/mx-session-lock-lib.sh"
    mx_harness_pid_alive "$pid" || die "session lock owner is not a live verified harness"
    command_line=$(ps -o args= -p "$pid" 2>/dev/null | sed 's/^[[:space:]]*//')
    state_json=$(jq -cn --arg lock "$(file_digest "$lock")" --arg pid "$pid" \
      --arg command "$command_line" '{lock_digest:$lock,pid:$pid,verified_harness_command:$command}')
    emit session.terminate-owner broker-session multplx \
      "terminate live broker harness pid $pid and reacquire session lock" "harness-pid:$pid" "$state_json" \
      "Send TERM only to the verified competing harness, prove it exited, then acquire the ordinary lock without bypassing it."
    ;;
  *) die "unknown binding mode: $mode" ;;
esac
