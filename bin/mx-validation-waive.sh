#!/usr/bin/env bash
# Create a truthful exact-SHA waived delivery handoff without marking a gate passed.
# Usage: mx-validation-waive.sh <task-id> <sha> <override-request-id> [--title <title>]
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
MX_ROOT=${MX_ROOT_OVERRIDE:-$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd -P)}
MX_HOME=${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}
STATE=${MX_STATE_OVERRIDE:-$MX_HOME/state}
# shellcheck source=bin/mx-pr-lib.sh
. "$SCRIPT_DIR/mx-pr-lib.sh"
# shellcheck source=bin/mx-deliver-lib.sh
. "$SCRIPT_DIR/mx-deliver-lib.sh"
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$SCRIPT_DIR/mx-maintainer-override-lib.sh"

[ "$#" -ge 3 ] || { echo "usage: mx-validation-waive.sh <task-id> <sha> <override-request-id> [--title <title>]" >&2; exit 2; }
id=$1
sha=$2
request=$3
shift 3
title=
if [ "${1:-}" = --title ] && [ "$#" -eq 2 ]; then title=$2; elif [ "$#" -ne 0 ]; then exit 2; fi
if ! mx_pr_task_id_valid "$id" || ! mx_pr_head_valid "$sha"; then
  echo "validation-waive: invalid task or SHA" >&2
  exit 2
fi
meta=$STATE/$id.meta
run=$STATE/$id.gate/run.json
[ -f "$meta" ] && [ ! -L "$meta" ] && [ -f "$run" ] && [ ! -L "$run" ] || { echo "validation-waive: task or gate state is unavailable" >&2; exit 1; }
[ "$(jq -r '.status' "$run")" != passed ] || { echo "validation-waive: gate already passed; use the ordinary handoff" >&2; exit 1; }
worktree=$(sed -n 's/^worktree=//p' "$meta" | head -1)
branch=$(git -C "$worktree" symbolic-ref --quiet --short HEAD 2>/dev/null) || exit 1
head=$(git -C "$worktree" rev-parse --verify HEAD 2>/dev/null) || exit 1
[ "$branch" = "mx/$id" ] && [ "$head" = "$sha" ] || { echo "validation-waive: worktree no longer matches the exact SHA" >&2; exit 1; }
base=$(jq -r '.default_branch // empty' "$run")
if [ -z "$base" ]; then
  base=$(git -C "$worktree" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)
  base=${base#origin/}
fi
[ -n "$base" ] || base=main
mx_delivery_ref_valid "$base" || { echo "validation-waive: invalid base branch" >&2; exit 1; }
[ -n "$title" ] || title=$(git -C "$worktree" log -1 --format=%s)
mx_delivery_title_valid "$title" || { echo "validation-waive: invalid delivery title" >&2; exit 1; }
bindings=$(MX_HOME="$MX_HOME" MX_STATE_OVERRIDE="$STATE" "$SCRIPT_DIR/mx-override-bindings.sh" validation "$id" "$sha") || exit 1
mx_override_consume "$request" "$(printf '%s' "$bindings" | jq -r '.boundary')" \
  "$(printf '%s' "$bindings" | jq -r '.task')" "$(printf '%s' "$bindings" | jq -r '.project')" \
  "$(printf '%s' "$bindings" | jq -r '.operation')" "$(printf '%s' "$bindings" | jq -r '.target')" \
  "$(printf '%s' "$bindings" | jq -r '.expected_state_digest')" >/dev/null || exit 1
tmp=$(mktemp "$STATE/.ready-to-push-waived.XXXXXX") || exit 1
{
  printf 'version=2\n'
  printf 'task=%s\n' "$id"
  printf 'worktree=%s\n' "$worktree"
  printf 'branch=%s\n' "$branch"
  printf 'approved_sha=%s\n' "$sha"
  printf 'base=%s\n' "$base"
  printf 'gate_run=%s\n' "$STATE/$id.gate"
  printf 'approval=pending\n'
  printf 'title=%s\n' "$title"
  printf 'validation=waived\n'
  printf 'override_request=%s\n' "$request"
} >"$tmp"
chmod 600 "$tmp"
mv "$tmp" "$STATE/$id.ready-to-push"
mx_override_result "$request" succeeded "maintainer-waived delivery handoff created for exact SHA $sha" || true
printf 'validation-waive: waived, not passed, for %s at %s; delivery approval is pending\n' "$id" "$sha"
