#!/usr/bin/env bash
# Record a PR-ready task: store one validated canonical pr=<url> and the forge's
# exact pr_head=<sha> when available, then atomically arm a static merge poll.
# The watcher check source is byte-for-byte bin/mx-pr-poll.sh; task and PR data
# live only in a private sidecar and are never interpolated into shell source.
# Only a canonical GitHub pull request URL is accepted; any other host is a
# hard validation error.
# Usage: mx-pr-check.sh <task-id> <pr-url>
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Portion 11 Rust-default orchestration boundary.
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_review_delivery_implementation) || exit $?
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" review mx-pr-check.sh "$@"
fi
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-pr-lib.sh
. "$SCRIPT_DIR/mx-pr-lib.sh"

if [ "$#" -ne 2 ]; then
  echo "error: invalid PR check request" >&2
  exit 2
fi
ID=$1
RAW_URL=$2
if ! mx_pr_task_id_valid "$ID" || ! mx_pr_url_parse "$RAW_URL"; then
  echo "error: invalid PR check request" >&2
  exit 2
fi
URL=$MX_PR_URL
PROVIDER=$MX_PR_PROVIDER
HOST=$MX_PR_HOST
PROJECT_PATH=$MX_PR_PATH
NUMBER=$MX_PR_NUMBER

# Task-derived paths are constructed only after the canonical ID validation.
META="$STATE/$ID.meta"
if [ ! -f "$META" ] || [ -L "$META" ] || [ "$(mx_pr_file_link_count "$META")" != 1 ]; then
  echo "error: task metadata is unavailable" >&2
  exit 1
fi

# A prior exact merged result may have queued its durable wake immediately
# before interruption.
# Finish only its identity-bound receipt before publishing a replacement poll.
mx_pr_poll_retirement_recover_one "$STATE" "$ID" "$SCRIPT_DIR/mx-pr-poll.sh" || {
  echo "error: pending PR poll retirement could not be validated" >&2
  exit 1
}

# Neutralize any pre-fix poll before recording or arming this task. The
# migration never executes legacy artifacts and holds watcher exclusion while
# it quarantines or rebuilds them.
"$SCRIPT_DIR/mx-pr-check-migrate.sh" --checks-safe || exit 1
"$MX_ROOT/bin/mx-guard.sh" || true

# pr_head is recorded only when gh can supply it: it exposes the head commit
# as a selectable field. Both consumers already treat it as optional:
# bin/mx-teardown.sh reads the head from the forge at teardown rather than from
# metadata and falls back to its provider-agnostic content check, and
# bin/mx-review-diff.sh resolves the head from the remote when none is recorded.
WT=$(grep '^worktree=' "$META" | tail -1 | cut -d= -f2- || true)
PR_HEAD=
if [ "$PROVIDER" = github ] && [ -n "$WT" ] && [ -d "$WT" ] && command -v gh >/dev/null 2>&1; then
  if REMOTE_HEAD=$(cd "$WT" && gh pr view "$URL" --json headRefOid -q .headRefOid 2>/dev/null) \
    && mx_pr_head_valid "$REMOTE_HEAD"; then
    PR_HEAD=$REMOTE_HEAD
  fi
fi

META_TMP=
pr_check_cleanup() {
  mx_pr_poll_cleanup
  [ -z "$META_TMP" ] || rm -f -- "$META_TMP"
}
trap pr_check_cleanup EXIT
trap 'exit 1' HUP INT TERM
mx_pr_poll_prepare "$STATE" "$ID" "$PROVIDER" "$URL" "$HOST" "$PROJECT_PATH" "$NUMBER" "$SCRIPT_DIR/mx-pr-poll.sh" \
  || { echo "error: could not prepare PR poll" >&2; exit 1; }

META_DEVICE=$(mx_pr_file_device "$META") || exit 1
STATE_DEVICE=$(mx_pr_file_device "$STATE") || exit 1
[ "$META_DEVICE" = "$STATE_DEVICE" ] || { echo "error: task metadata is unavailable" >&2; exit 1; }
META_TMP=$(mktemp "$STATE/.mx-pr-meta.XXXXXX") || exit 1
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    pr=*|pr_head=*) ;;
    *) printf '%s\n' "$line" >> "$META_TMP" || exit 1 ;;
  esac
done < "$META"
printf 'pr=%s\n' "$URL" >> "$META_TMP" || exit 1
[ -z "$PR_HEAD" ] || printf 'pr_head=%s\n' "$PR_HEAD" >> "$META_TMP" || exit 1
chmod 0600 "$META_TMP" || exit 1
mx_pr_private_file_valid "$META_TMP" 600 "$STATE_DEVICE" || exit 1
mx_pr_metadata_identity_parse "$META_TMP" || exit 1
[ "$MX_PR_META_PROVIDER" = "$PROVIDER" ] && [ "$MX_PR_META_URL" = "$URL" ] \
  && [ "$MX_PR_META_HOST" = "$HOST" ] && [ "$MX_PR_META_PATH" = "$PROJECT_PATH" ] \
  && [ "$MX_PR_META_NUMBER" = "$NUMBER" ] || exit 1
mx_pr_regular_destination_on_device_or_absent "$META" "$STATE_DEVICE" || exit 1
mv -f -- "$META_TMP" "$META" || exit 1
META_TMP=
mx_pr_private_file_valid "$META" 600 "$STATE_DEVICE" || exit 1
mx_pr_metadata_identity_parse "$META" || exit 1
[ "$MX_PR_META_PROVIDER" = "$PROVIDER" ] && [ "$MX_PR_META_URL" = "$URL" ] \
  && [ "$MX_PR_META_HOST" = "$HOST" ] && [ "$MX_PR_META_PATH" = "$PROJECT_PATH" ] \
  && [ "$MX_PR_META_NUMBER" = "$NUMBER" ] || exit 1

mx_pr_poll_publish_prepared || {
  echo "error: could not publish PR poll" >&2
  exit 1
}
printf 'armed: state/%s.check.sh\n' "$ID"
