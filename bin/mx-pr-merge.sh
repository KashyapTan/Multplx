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
# Usage: mx-pr-merge.sh <task-id> <pr-url> [-- <extra gh pr merge args>]
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-pr-lib.sh
. "$SCRIPT_DIR/mx-pr-lib.sh"
# shellcheck source=bin/mx-deliver-lib.sh
. "$SCRIPT_DIR/mx-deliver-lib.sh"

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

"$SCRIPT_DIR/mx-pr-check.sh" "$ID" "$URL"
grep -qxF "pr=$URL" "$META" || {
  echo "error: PR metadata recording failed" >&2
  exit 1
}

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

gh pr merge "$PR_NUMBER" --repo "$PR_OWNER/$PR_REPO" \
  "${merge_args[@]+"${merge_args[@]}"}" \
  "${NORMALIZED_ARGS[@]+"${NORMALIZED_ARGS[@]}"}"
