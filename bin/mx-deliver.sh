#!/usr/bin/env bash
# Deliver one or all approved local branches from outside every agent session.
#
# Usage: mx-deliver.sh [<task-id>]
#
# With an id, consume state/<id>.ready-to-push. Without one, scan every such
# record. Each record is parsed and re-verified through mx-deliver-lib.sh before
# any network write. The service pushes the exact approved object id to the
# recorded branch, creates a PR with a deterministic gate-derived body, records
# the canonical URL through mx-pr-check.sh, then archives the queue record as
# state/<id>.delivered. A stale record moves to <record>.stale.
#
# Ambient GH_TOKEN/GITHUB_TOKEN values are never forwarded. A scheduler that
# uses a token passes MX_DELIVERY_GH_TOKEN; a scheduler that uses an isolated gh
# config passes MX_DELIVERY_GH_CONFIG_DIR. Maintainer-shell invocation may use
# the maintainer's normal gh/keychain configuration. Credentials are never read
# from a repository or state file.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-pr-lib.sh
. "$SCRIPT_DIR/mx-pr-lib.sh"
# shellcheck source=bin/mx-deliver-lib.sh
. "$SCRIPT_DIR/mx-deliver-lib.sh"

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
esac
if [ "$#" -gt 1 ]; then
  echo "error: invalid delivery request" >&2
  exit 2
fi
if [ "$#" -eq 1 ] && ! mx_pr_task_id_valid "$1"; then
  echo "error: invalid delivery request" >&2
  exit 2
fi
[ -d "$STATE" ] && [ ! -L "$STATE" ] || {
  echo "error: delivery state directory is unavailable" >&2
  exit 1
}
mx_delivery_refuse_agent_ambience || exit $?

if [ -n "${MX_DELIVERY_GH_TOKEN:-}" ] && [ -n "${MX_DELIVERY_GH_CONFIG_DIR:-}" ]; then
  echo "error: choose one delivery credential source, not both" >&2
  exit 1
fi

DELIVERY_ENV=(env
  -u GH_TOKEN
  -u GITHUB_TOKEN
  -u GH_ENTERPRISE_TOKEN
  -u GITHUB_ENTERPRISE_TOKEN
  -u GH_CONFIG_DIR
  -u MX_AGENT_GH_TOKEN
  -u MX_DELIVERY_GH_TOKEN
  -u MX_DELIVERY_GH_CONFIG_DIR
  -u CLAUDECODE
  -u CODEX_THREAD_ID
  -u PI_CODING_AGENT
  -u DEEP_REVIEW_GATE
)
if [ -n "${MX_DELIVERY_GH_TOKEN:-}" ]; then
  DELIVERY_ENV+=("GH_TOKEN=$MX_DELIVERY_GH_TOKEN")
elif [ -n "${MX_DELIVERY_GH_CONFIG_DIR:-}" ]; then
  case "$MX_DELIVERY_GH_CONFIG_DIR" in /*) ;; *)
    echo "error: MX_DELIVERY_GH_CONFIG_DIR must be absolute" >&2
    exit 1
    ;;
  esac
  [ -d "$MX_DELIVERY_GH_CONFIG_DIR" ] || {
    echo "error: MX_DELIVERY_GH_CONFIG_DIR is unavailable" >&2
    exit 1
  }
  DELIVERY_ENV+=("GH_CONFIG_DIR=$MX_DELIVERY_GH_CONFIG_DIR")
fi

delivery_exec() {
  "${DELIVERY_ENV[@]}" "$@"
}

archive_delivered() {
  local record=$1 destination=$2
  mx_delivery_record_unchanged "$record" || return 1
  mx_pr_regular_destination_or_absent "$destination" || return 1
  [ ! -e "$destination" ] || return 1
  mv -- "$record" "$destination"
}

deliver_one() {
  local id=$1 record destination eligibility pr_url
  record="$STATE/$id.ready-to-push"
  destination="$STATE/$id.delivered"
  [ -e "$record" ] || [ -L "$record" ] || {
    echo "delivery: no ready record for $id" >&2
    return 1
  }
  if ! mx_delivery_record_parse "$record" "$id" "$STATE"; then
    echo "delivery: refused malformed or unsafe record for $id" >&2
    return 1
  fi
  mx_delivery_eligible "$STATE"
  eligibility=$?
  case "$eligibility" in
    0) ;;
    2)
      echo "delivery: $id is pending explicit approval" >&2
      return 1
      ;;
    3)
      if mx_delivery_mark_stale "$record"; then
        echo "delivery: stale $id - $MX_DELIVERY_STALE_REASON; archived as $(basename "$record").stale" >&2
      else
        echo "delivery: stale $id - $MX_DELIVERY_STALE_REASON; record changed while marking stale" >&2
      fi
      return 1
      ;;
    *)
      echo "delivery: refused unsafe eligibility state for $id" >&2
      return 1
      ;;
  esac

  mx_delivery_record_unchanged "$record" || {
    echo "delivery: refused $id because its ready record changed during verification" >&2
    return 1
  }
  delivery_exec git -C "$MX_DELIVERY_WORKTREE" push origin \
    "$MX_DELIVERY_APPROVED_SHA:refs/heads/$MX_DELIVERY_BRANCH" || {
    echo "delivery: push failed for $id" >&2
    return 1
  }
  if pr_url=$(cd "$MX_DELIVERY_WORKTREE" && delivery_exec gh pr create \
      --base "$MX_DELIVERY_BASE" \
      --head "$MX_DELIVERY_BRANCH" \
      --title "$MX_DELIVERY_TITLE" \
      --body "$MX_DELIVERY_BODY"); then
    :
  elif pr_url=$(cd "$MX_DELIVERY_WORKTREE" && delivery_exec gh pr view \
      "$MX_DELIVERY_BRANCH" --json url -q .url); then
    :
  else
    echo "delivery: PR creation failed for $id" >&2
    return 1
  fi
  pr_url=$(printf '%s\n' "$pr_url" | tail -1)
  if ! mx_pr_url_parse "$pr_url"; then
    echo "delivery: gh returned a non-canonical PR URL for $id" >&2
    return 1
  fi
  MX_ROOT_OVERRIDE="$MX_ROOT" MX_HOME="$MX_HOME" MX_STATE_OVERRIDE="$STATE" \
    "${DELIVERY_ENV[@]}" "$SCRIPT_DIR/mx-pr-check.sh" "$id" "$pr_url" || {
    echo "delivery: PR state recording failed for $id" >&2
    return 1
  }
  archive_delivered "$record" "$destination" || {
    echo "delivery: PR was recorded but the ready record could not be archived for $id" >&2
    return 1
  }
  printf 'delivered: %s %s\n' "$id" "$pr_url"
}

records=()
if [ "$#" -eq 1 ]; then
  records=("$1")
else
  shopt -s nullglob
  for record in "$STATE"/*.ready-to-push; do
    id=${record##*/}
    id=${id%.ready-to-push}
    records+=("$id")
  done
  shopt -u nullglob
fi

[ "${#records[@]}" -gt 0 ] || exit 0
rc=0
for id in "${records[@]}"; do
  deliver_one "$id" || rc=1
done
exit "$rc"
