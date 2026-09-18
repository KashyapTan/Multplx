#!/usr/bin/env bash
# Phase 11 configured discovery, local reuse and durable task intake.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mx_test_tmproot workspace-discovery)
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
HOME_FIXTURE="$TMP_ROOT/home"
DEV_ROOT="$TMP_ROOT/dev root"
REPO="$DEV_ROOT/team/app"
PLAIN="$DEV_ROOT/plain"
mkdir -p "$HOME_FIXTURE/state" "$HOME_FIXTURE/data" "$HOME_FIXTURE/config" "$PLAIN"
printf '{}\n' >"$PLAIN/package.json"
mx_git_init_commit "$REPO"
printf 'local and untouched\n' >"$REPO/working-file"

mx_cli() {
  MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$HOME_FIXTURE" \
    MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
    MX_CONFIG_OVERRIDE="$HOME_FIXTURE/config" \
    "${MX_RUST_BIN:-$ROOT/target/release/mx}" "$@"
}

mx_cli project roots add "$DEV_ROOT" --depth 5 >"$TMP_ROOT/root.json" || fail 'root add failed'
jq -e --arg root "$DEV_ROOT" '.roots == [{path:$root,max_depth:5}] and .follow_symlinks == false' \
  "$TMP_ROOT/root.json" >/dev/null || fail 'configured root contract differs'

mx_cli project discover --refresh >"$TMP_ROOT/discovery.json" || fail 'refresh failed'
jq -e --arg repo "$REPO" --arg plain "$PLAIN" '
  .status == "complete" and .completed_roots == 1 and
  any(.candidates[]; .canonical_path == $repo and .kind == "git" and .task_ready == true) and
  any(.candidates[]; .canonical_path == $plain and .kind == "unversioned" and .task_ready == false)
' "$TMP_ROOT/discovery.json" >/dev/null || fail 'nested Git and unversioned discovery differs'

mx_cli task --project "$REPO" --request-id login-request --batch-id release-batch \
  --task-id fix-login 'Fix login' >"$TMP_ROOT/receipt.json" || fail 'task intake failed'
jq -e '
  .schema == "mx-task-intake-receipt.v1" and .newly_accepted == true and
  .implementation_started == false and .working_changes == "present-and-excluded" and
  .route.kind == "main-orchestrator" and .request.task_id == "fix-login" and
  .request.batch_id == "release-batch" and .request.notification_event_id != null
' "$TMP_ROOT/receipt.json" >/dev/null || fail 'durable task receipt differs'
[ "$(cat "$REPO/working-file")" = 'local and untouched' ] || fail 'task intake changed working files'

printf 'next\n' >>"$REPO/file"
git -C "$REPO" add file
git -C "$REPO" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm next
mx_cli task --project "$REPO" --request-id login-request --batch-id release-batch \
  --task-id fix-login 'Fix login' >"$TMP_ROOT/retry.json" || fail 'identical retry failed after HEAD moved'
jq -e --arg start "$(jq -r '.request.starting_revision' "$TMP_ROOT/receipt.json")" '
  .newly_accepted == false and .request.starting_revision == $start
' "$TMP_ROOT/retry.json" >/dev/null || fail 'retry did not retain first binding'

if mx_cli task --project "$REPO" --request-id login-request --batch-id release-batch \
  --task-id fix-login 'Different request' >"$TMP_ROOT/conflict.out" 2>"$TMP_ROOT/conflict.err"; then
  fail 'changed retry was accepted'
fi
grep -F 'conflicts with durable submission intent' "$TMP_ROOT/conflict.err" >/dev/null \
  || fail 'changed retry did not explain conflict'

MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$HOME_FIXTURE" \
  MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
  MX_CONFIG_OVERRIDE="$HOME_FIXTURE/config" \
  "$ROOT/bin/mx-system-snapshot.sh" --json >"$TMP_ROOT/snapshot.json" \
  || fail 'canonical snapshot failed'
jq -e '
  any(.portfolio.tasks[]; .id == "fix-login" and .state == "queued-request" and
    .intake.request_id == "login-request" and .intake.implementation_started == false)
' "$TMP_ROOT/snapshot.json" >/dev/null || fail 'accepted intake missing from canonical portfolio'

if mx_cli task --project "$PLAIN" 'Cannot dispatch without Git' >"$TMP_ROOT/plain.out" 2>"$TMP_ROOT/plain.err"; then
  fail 'unversioned folder produced a task-ready binding'
fi
grep -E 'Git rev-parse|not a git repository' "$TMP_ROOT/plain.err" >/dev/null \
  || fail 'unversioned refusal did not retain its capability limit'

pass 'configured nested discovery, local reuse, immutable retry and canonical intake projection'
