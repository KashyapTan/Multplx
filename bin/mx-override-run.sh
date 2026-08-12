#!/usr/bin/env bash
# Bind, consume, run, and truthfully record one exact command exception.
# Usage:
#   mx-override-run.sh --print-bindings --boundary <id> --task <id>
#     --project <slug> --target <identity> [--verify-command <name>] -- <argv...>
#   mx-override-run.sh <request-id> --boundary <id> --task <id>
#     --project <slug> --target <identity> [--verify-command <name>] -- <argv...>
#
# project.direct-write runs with cwd fixed to the named git checkout and records
# its before/after repository state. dependency.install requires a named command
# capability and succeeds only when that command is discoverable afterward.
# security.one-action-elevation runs only the exact argv while all other guards
# remain unchanged.
set -u

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# Portion 10 Rust-default adapter. Selection occurs before fresh binding
# observation, grant consumption, or exact command execution.
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_authority_implementation) || exit $?
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd -P); export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" authority mx-override-run.sh "$@"
fi
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$SCRIPT_DIR/mx-maintainer-override-lib.sh"

fail_usage() { printf 'mx-override-run: %s\n' "$*" >&2; exit 2; }

print_only=0
request=
case "${1:-}" in
  --print-bindings) print_only=1; shift ;;
  '') fail_usage "request id or --print-bindings is required" ;;
  *) request=$1; shift ;;
esac

boundary=''
task=''
project=''
target=''
verify_command=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --boundary) [ "$#" -ge 2 ] || fail_usage "--boundary requires a value"; boundary=$2; shift 2 ;;
    --task) [ "$#" -ge 2 ] || fail_usage "--task requires a value"; task=$2; shift 2 ;;
    --project) [ "$#" -ge 2 ] || fail_usage "--project requires a value"; project=$2; shift 2 ;;
    --target) [ "$#" -ge 2 ] || fail_usage "--target requires a value"; target=$2; shift 2 ;;
    --verify-command) [ "$#" -ge 2 ] || fail_usage "--verify-command requires a value"; verify_command=$2; shift 2 ;;
    --) shift; break ;;
    *) fail_usage "unknown argument: $1" ;;
  esac
done
[ -n "$boundary" ] && [ -n "$task" ] && [ -n "$project" ] \
  && [ -n "$target" ] && [ "$#" -gt 0 ] \
  || fail_usage "every binding and one command are required"
if ! mx_override_slug_valid "$task" || ! mx_override_slug_valid "$project"; then
  fail_usage "task and project must be safe slugs"
fi
case "$boundary" in
  project.direct-write|security.one-action-elevation|dependency.install) ;;
  *) fail_usage "boundary does not use the exact-command runner: $boundary" ;;
esac
operation=$(jq -cn '$ARGS.positional' --args -- "$@") || exit 1
action_executable=$1

state_for_action() {
  local canonical canonical_top raw_top status head branch command_path
  case "$boundary" in
    project.direct-write)
      canonical=$(cd "$target" 2>/dev/null && pwd -P) || return 1
      raw_top=$(git -C "$canonical" rev-parse --show-toplevel 2>/dev/null) || return 1
      canonical_top=$(cd "$raw_top" 2>/dev/null && pwd -P) || return 1
      [ "$canonical_top" = "$canonical" ] || return 1
      target=$canonical
      head=$(git -C "$target" rev-parse --verify HEAD 2>/dev/null) || return 1
      branch=$(git -C "$target" symbolic-ref --quiet --short HEAD 2>/dev/null || printf detached)
      status=$(git -C "$target" status --porcelain=v1 --untracked-files=all 2>/dev/null) || return 1
      jq -cn --arg head "$head" --arg branch "$branch" --arg status "$(mx_override_sha256_text "$status")" \
        '{checkout_head:$head,checkout_branch:$branch,checkout_status_digest:$status}'
      ;;
    dependency.install)
      mx_override_slug_valid "$verify_command" || return 1
      [ "$target" = "command:$verify_command" ] || return 1
      command_path=$(command -v "$verify_command" 2>/dev/null || printf absent)
      jq -cn --arg command "$verify_command" --arg path "$command_path" --arg os "$(uname -s)-$(uname -m)" \
        '{required_command:$command,current_path:$path,host:$os}'
      ;;
    security.one-action-elevation)
      jq -cn --arg cwd "$(pwd -P)" --arg uid "$(id -u)" --arg executable "$(command -v "$action_executable" 2>/dev/null || printf unavailable)" \
        '{cwd:$cwd,uid:$uid,executable:$executable}'
      ;;
  esac
}

state=$(state_for_action) || fail_usage "target or capability binding is not valid for $boundary"
state_digest=$(mx_override_sha256_text "$state") || exit 1
case "$boundary" in
  project.direct-write) consequence="Run only the exact argv from the named checkout and report its resulting git-state digest for ordinary validation and delivery." ;;
  dependency.install) consequence="Run only the exact installer argv and report success only if command $verify_command is discoverable afterward." ;;
  security.one-action-elevation) consequence="Run only the exact elevated argv once while leaving every other sandbox and command guard unchanged." ;;
esac

if [ "$print_only" -eq 1 ]; then
  jq -cn --arg boundary "$boundary" --arg task "$task" --arg project "$project" \
    --arg operation "$operation" --arg target "$target" --arg expected_state_digest "$state_digest" \
    --arg consequence "$consequence" --argjson state "$state" \
    '{boundary:$boundary,task:$task,project:$project,operation:$operation,target:$target,expected_state_digest:$expected_state_digest,consequence:$consequence,state:$state}'
  exit 0
fi

mx_override_consume "$request" "$boundary" "$task" "$project" \
  "$operation" "$target" "$state_digest" >/dev/null || exit 1
if [ "$boundary" = project.direct-write ]; then
  (cd "$target" && "$@")
  status=$?
else
  "$@"
  status=$?
fi
if [ "$status" -eq 0 ] && [ "$boundary" = dependency.install ] \
   && ! command -v "$verify_command" >/dev/null 2>&1; then
  status=1
fi
if [ "$status" -eq 0 ]; then
  after=$(state_for_action 2>/dev/null || printf unavailable)
  mx_override_result "$request" succeeded \
    "exact $boundary action completed; resulting state digest $(mx_override_sha256_text "$after")" || true
else
  mx_override_result "$request" failed "exact $boundary action failed or capability verification failed with status $status" || true
fi
exit "$status"
