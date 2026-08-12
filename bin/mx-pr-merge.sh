#!/usr/bin/env bash
# Merge a task's PR after recording pr= and any available pr_head= through
# bin/mx-pr-check.sh, so teardown can verify landed work after squash merges.
# The full canonical GitHub PR URL is parsed by bin/mx-pr-lib.sh and the derived
# owner/repository and PR number are passed to the official gh CLI.
# Plan 09 moves invocation of this script behind the credentialed delivery
# process; actors must never invoke it or receive remote write credentials.
#
# Merge method defaults to --squash when the caller passes none of --squash,
# --merge, or --rebase after the optional -- separator. Legacy --method
# spellings are normalized onto gh's native flags. Extra args
# must not include --repo or -R because the repository comes only from the URL.
# Usage: mx-pr-merge.sh <task-id> <pr-url> [--override <request-id>] [-- <extra gh pr merge args>]
#        mx-pr-merge.sh <task-id> <pr-url> --print-override-bindings [-- <extra args>]
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Portion 11 Rust-default orchestration boundary.
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_review_delivery_implementation) || exit $?
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" review mx-pr-merge.sh "$@"
fi
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-pr-lib.sh
. "$SCRIPT_DIR/mx-pr-lib.sh"
# shellcheck source=bin/mx-deliver-lib.sh
. "$SCRIPT_DIR/mx-deliver-lib.sh"
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$SCRIPT_DIR/mx-maintainer-override-lib.sh"

mx_delivery_refuse_agent_ambience || exit $?

if [ "$#" -lt 2 ]; then
  echo "error: invalid PR merge request" >&2
  exit 2
fi
ID=$1
RAW_URL=$2
# bin/mx-pr-lib.sh parses only canonical GitHub pull request URLs; the
# provider check is defense in depth so this path can never address anything
# but GitHub by owner/repository.
if ! mx_pr_task_id_valid "$ID" || ! mx_pr_url_parse "$RAW_URL" \
  || [ "$MX_PR_PROVIDER" != github ]; then
  echo "error: invalid PR merge request" >&2
  exit 2
fi
URL=$MX_PR_URL
PR_OWNER=$MX_PR_OWNER
PR_REPO=$MX_PR_REPO
PR_NUMBER=$MX_PR_NUMBER
shift 2
OVERRIDE_ID=
PRINT_BINDINGS=0
case "${1:-}" in
  --override)
    [ "$#" -ge 2 ] && [ -n "$2" ] || { echo "error: --override requires a request id" >&2; exit 2; }
    OVERRIDE_ID=$2
    shift 2
    ;;
  --print-override-bindings)
    PRINT_BINDINGS=1
    shift
    ;;
esac
[ "${1:-}" = "--" ] && shift

reject_repo_overrides() {
  local arg
  for arg in "$@"; do
    case "$arg" in
      --repo|--repo=*|-R|-R?*)
        echo "error: extra merge arguments must not override the repository" >&2
        return 1
        ;;
    esac
  done
}

reject_repo_overrides "$@" || exit 1

normalize_merge_args() {
  local arg method want_method=0
  NORMALIZED_ARGS=()
  for arg in "$@"; do
    if [ "$want_method" -eq 1 ]; then
      method=$arg
      want_method=0
      case "$method" in
        squash|merge|rebase) NORMALIZED_ARGS+=("--$method") ;;
        *) echo "error: unsupported merge method: $method" >&2; return 1 ;;
      esac
      continue
    fi
    case "$arg" in
      --method) want_method=1 ;;
      --method=*)
        method=${arg#--method=}
        case "$method" in
          squash|merge|rebase) NORMALIZED_ARGS+=("--$method") ;;
          *) echo "error: unsupported merge method: $method" >&2; return 1 ;;
        esac
        ;;
      *) NORMALIZED_ARGS+=("$arg") ;;
    esac
  done
  [ "$want_method" -eq 0 ] || {
    echo "error: --method requires squash, merge, or rebase" >&2
    return 1
  }
}

normalize_merge_args "$@" || exit 1

# Task-derived paths are constructed only after the canonical ID validation.
META="$STATE/$ID.meta"
if [ ! -f "$META" ] || [ -L "$META" ]; then
  echo "error: task metadata is unavailable" >&2
  exit 1
fi

merge_args=()
has_method=0
for arg in "${NORMALIZED_ARGS[@]+"${NORMALIZED_ARGS[@]}"}"; do
  case "$arg" in
    --squash|--merge|--rebase)
      has_method=1
      ;;
  esac
done
[ "$has_method" -eq 1 ] || merge_args=(--squash)

merge_state() {
  local raw
  raw=$(gh pr view "$PR_NUMBER" --repo "$PR_OWNER/$PR_REPO" --json headRefOid,statusCheckRollup) || return 1
  printf '%s' "$raw" | jq -ce '
    (.headRefOid // "") as $sha |
    select($sha | test("^[0-9a-f]{40}([0-9a-f]{24})?$")) |
    {sha:$sha,failed_checks:([
      .statusCheckRollup[]? |
      select(((.conclusion // .state // .status // "") | ascii_upcase) as $state |
        ($state == "FAILURE" or $state == "FAILED" or $state == "ERROR" or $state == "CANCELLED" or $state == "TIMED_OUT")) |
      {name:(.name // .context // .workflowName // "unknown"),state:(.conclusion // .state // .status // "unknown")}
    ] | sort_by(.name,.state))}
  '
}

merge_operation() {
  local args
  args=(gh pr merge "$PR_NUMBER" --repo "$PR_OWNER/$PR_REPO")
  args+=("${merge_args[@]+"${merge_args[@]}"}")
  args+=("${NORMALIZED_ARGS[@]+"${NORMALIZED_ARGS[@]}"}")
  case " ${args[*]} " in *' --admin '*) ;; *) args+=(--admin) ;; esac
  jq -cn '$ARGS.positional' --args -- "${args[@]}"
}

override_bindings() {
  local state_json=$1 operation=$2 sha digest project
  sha=$(printf '%s' "$state_json" | jq -r '.sha')
  [ "$(printf '%s' "$state_json" | jq '.failed_checks | length')" -gt 0 ] || {
    echo "error: PR check failure is not a red-check set; use the concrete capability or integrity recovery path" >&2
    return 1
  }
  digest=$(mx_override_sha256_text "$state_json") || return 1
  project=$(printf '%s' "$PR_REPO" | tr -c 'A-Za-z0-9._-' '_')
  jq -cn --arg boundary delivery.merge-red --arg task "$ID" --arg project "$project" \
    --arg operation "$operation" --arg target "$URL@$sha" --arg expected_state_digest "$digest" \
    --arg consequence "Merge the exact PR head despite the recorded failed check set; record the merge as maintainer-directed." \
    --argjson state "$state_json" \
    '{boundary:$boundary,task:$task,project:$project,operation:$operation,target:$target,expected_state_digest:$expected_state_digest,consequence:$consequence,state:$state}'
}

if [ "$PRINT_BINDINGS" -eq 1 ]; then
  state_json=$(merge_state) || { echo "error: could not inspect exact PR head and check set" >&2; exit 1; }
  override_bindings "$state_json" "$(merge_operation)"
  exit
fi

if ! "$SCRIPT_DIR/mx-pr-check.sh" "$ID" "$URL"; then
  echo "error: PR metadata and poll could not be established; no merge authority can change that capability result" >&2
  exit 1
fi
grep -qxF "pr=$URL" "$META" || { echo "error: PR metadata recording failed" >&2; exit 1; }

if [ -z "$OVERRIDE_ID" ]; then
  gh pr merge "$PR_NUMBER" --repo "$PR_OWNER/$PR_REPO" \
    "${merge_args[@]+"${merge_args[@]}"}" \
    "${NORMALIZED_ARGS[@]+"${NORMALIZED_ARGS[@]}"}"
  exit
fi
state_json=$(merge_state) || { echo "error: could not inspect exact PR head and failed-check set" >&2; exit 1; }
operation=$(merge_operation)
bindings=$(override_bindings "$state_json" "$operation") || exit 1
MX_STATE_OVERRIDE="$STATE" mx_override_consume "$OVERRIDE_ID" delivery.merge-red "$ID" \
  "$(printf '%s' "$bindings" | jq -r '.project')" "$operation" \
  "$(printf '%s' "$bindings" | jq -r '.target')" "$(printf '%s' "$bindings" | jq -r '.expected_state_digest')" >/dev/null || exit 1
merge_command=(gh pr merge "$PR_NUMBER" --repo "$PR_OWNER/$PR_REPO")
merge_command+=("${merge_args[@]+"${merge_args[@]}"}")
merge_command+=("${NORMALIZED_ARGS[@]+"${NORMALIZED_ARGS[@]}"}")
case " ${merge_command[*]} " in *' --admin '*) ;; *) merge_command+=(--admin) ;; esac
if "${merge_command[@]}"; then
  MX_STATE_OVERRIDE="$STATE" mx_override_result "$OVERRIDE_ID" succeeded \
    "maintainer-directed merge completed for $(printf '%s' "$state_json" | jq -r '.sha') with recorded failed checks" || true
else
  rc=$?
  MX_STATE_OVERRIDE="$STATE" mx_override_result "$OVERRIDE_ID" failed \
    "maintainer-directed merge command failed with status $rc" || true
  exit "$rc"
fi
