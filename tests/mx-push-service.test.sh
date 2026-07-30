#!/usr/bin/env bash
# End-to-end tests for the non-agent least-privilege delivery service.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_git_identity mx-test mx-test@example.invalid

DELIVER="$ROOT/bin/mx-deliver.sh"
TMP_ROOT=$(mx_test_tmproot mx-push-service)
REAL_GIT=$(command -v git)
export REAL_GIT
REAL_GH=$(command -v gh)

make_case() {
  local name=$1 case_dir fakebin head
  case_dir="$TMP_ROOT/$name"
  fakebin="$case_dir/fakebin"
  mkdir -p "$case_dir/state" "$case_dir/config" "$fakebin"
  case_dir=$(cd "$case_dir" && pwd -P)
  fakebin="$case_dir/fakebin"

  "$REAL_GIT" init -q --bare "$case_dir/origin.git"
  "$REAL_GIT" -C "$case_dir/origin.git" symbolic-ref HEAD refs/heads/main
  "$REAL_GIT" clone -q "$case_dir/origin.git" "$case_dir/seed" 2>/dev/null
  "$REAL_GIT" -C "$case_dir/seed" commit -q --allow-empty -m baseline
  "$REAL_GIT" -C "$case_dir/seed" push -q origin HEAD:main
  "$REAL_GIT" clone -q "$case_dir/origin.git" "$case_dir/project"
  "$REAL_GIT" -C "$case_dir/project" worktree add -q -b mx/task-x1 "$case_dir/wt" main
  printf 'validated\n' > "$case_dir/wt/change.txt"
  "$REAL_GIT" -C "$case_dir/wt" add change.txt
  "$REAL_GIT" -C "$case_dir/wt" commit -q -m "validated change"
  head=$("$REAL_GIT" -C "$case_dir/wt" rev-parse HEAD)

  mx_write_meta "$case_dir/state/task-x1.meta" \
    "window=mx-task-x1" \
    "worktree=$case_dir/wt" \
    "project=$case_dir/project" \
    "kind=delivery" \
    "mode=no-mistakes"
  chmod 600 "$case_dir/state/task-x1.meta"
  mkdir -p "$case_dir/state/task-x1.gate"
  jq -n \
    --arg head "$head" \
    '{
      status:"passed",
      approved_head:$head,
      summary:"Validated change",
      risk_level:"low",
      risk_rationale:"Focused local change with passing validation."
    }' > "$case_dir/state/task-x1.gate/run.json"
  chmod 600 "$case_dir/state/task-x1.gate/run.json"
  touch "$case_dir/state/.last-watcher-beat"

  cat > "$fakebin/git" <<'SH'
#!/usr/bin/env bash
if printf '%s\n' "$*" | grep -q ' push '; then
  printf 'GH_TOKEN=%s GITHUB_TOKEN=%s MX_AGENT_GH_TOKEN=%s CODEX_THREAD_ID=%s\n' \
    "${GH_TOKEN:-}" "${GITHUB_TOKEN:-}" "${MX_AGENT_GH_TOKEN:-}" "${CODEX_THREAD_ID:-}" \
    >> "$MX_TEST_PUSH_ENV_LOG"
  printf '%s\n' "$*" >> "$MX_TEST_PUSH_LOG"
fi
exec "$REAL_GIT" "$@"
SH
  cat > "$fakebin/gh" <<'SH'
#!/usr/bin/env bash
printf 'GH_TOKEN=%s GITHUB_TOKEN=%s MX_AGENT_GH_TOKEN=%s CODEX_THREAD_ID=%s\n' \
  "${GH_TOKEN:-}" "${GITHUB_TOKEN:-}" "${MX_AGENT_GH_TOKEN:-}" "${CODEX_THREAD_ID:-}" \
  >> "$MX_TEST_GH_ENV_LOG"
case "${1:-} ${2:-}" in
  "pr create")
    printf '%s\n' "$*" >> "$MX_TEST_GH_LOG"
    printf '%s\n' 'https://github.com/example/repo/pull/42'
    ;;
  "pr view")
    "$REAL_GIT" -C "$MX_TEST_WORKTREE" rev-parse HEAD
    ;;
  *) exit 0 ;;
esac
SH
  chmod +x "$fakebin/git" "$fakebin/gh"
  : > "$case_dir/push.log"
  : > "$case_dir/push-env.log"
  : > "$case_dir/gh.log"
  : > "$case_dir/gh-env.log"
  printf '%s\n' "$case_dir"
}

write_record() {
  local case_dir=$1 approval=$2 head
  head=$("$REAL_GIT" -C "$case_dir/wt" rev-parse HEAD)
  {
    printf 'version=1\n'
    printf 'task=task-x1\n'
    printf 'worktree=%s\n' "$case_dir/wt"
    printf 'branch=mx/task-x1\n'
    printf 'approved_sha=%s\n' "$head"
    printf 'base=main\n'
    printf 'gate_run=%s\n' "$case_dir/state/task-x1.gate"
    printf 'approval=%s\n' "$approval"
    printf 'title=Validated delivery\n'
  } > "$case_dir/state/task-x1.ready-to-push"
  chmod 600 "$case_dir/state/task-x1.ready-to-push"
}

run_delivery() {
  local case_dir=$1
  shift
  env -u CODEX_THREAD_ID -u CLAUDECODE -u PI_CODING_AGENT \
    MX_ROOT_OVERRIDE="$ROOT" \
    MX_HOME="$case_dir" \
    MX_STATE_OVERRIDE="$case_dir/state" \
    MX_TEST_PUSH_LOG="$case_dir/push.log" \
    MX_TEST_PUSH_ENV_LOG="$case_dir/push-env.log" \
    MX_TEST_GH_LOG="$case_dir/gh.log" \
    MX_TEST_GH_ENV_LOG="$case_dir/gh-env.log" \
    MX_TEST_WORKTREE="$case_dir/wt" \
    PATH="$case_dir/fakebin:$PATH" \
    "$DELIVER" "$@"
}

test_empty_scan_and_pending_record_never_push() {
  local case_dir rc
  case_dir=$(make_case pending)
  run_delivery "$case_dir" >/dev/null 2>&1 || fail "empty queue scan should succeed"
  [ ! -s "$case_dir/push.log" ] || fail "empty queue scan pushed"
  write_record "$case_dir" pending
  set +e
  run_delivery "$case_dir" task-x1 >"$case_dir/out" 2>"$case_dir/err"
  rc=$?
  set -e
  expect_code 1 "$rc" "pending record must be refused"
  assert_grep 'pending explicit approval' "$case_dir/err" "pending refusal was unclear"
  [ ! -s "$case_dir/push.log" ] || fail "pending record pushed"
  [ ! -s "$case_dir/gh.log" ] || fail "pending record opened a PR"
  pass "mx-deliver pushes only approved records"
}

test_approved_record_delivers_once_and_sanitizes_credentials() {
  local case_dir pushes creates
  case_dir=$(make_case success)
  write_record "$case_dir" approved
  GH_TOKEN=agent-leak GITHUB_TOKEN=agent-leak MX_AGENT_GH_TOKEN=read-leak \
    MX_DELIVERY_GH_TOKEN=service-only \
    run_delivery "$case_dir" task-x1 >"$case_dir/out" 2>"$case_dir/err" \
    || fail "approved record should deliver"

  pushes=$(wc -l < "$case_dir/push.log" | tr -d '[:space:]')
  creates=$(grep -c '^pr create ' "$case_dir/gh.log" || true)
  [ "$pushes" = 1 ] || fail "approved record pushed $pushes times"
  [ "$creates" = 1 ] || fail "approved record created $creates PRs"
  assert_grep 'pr=https://github.com/example/repo/pull/42' "$case_dir/state/task-x1.meta" \
    "delivery did not feed the PR URL through mx-pr-check"
  assert_present "$case_dir/state/task-x1.delivered" "ready record was not archived"
  assert_absent "$case_dir/state/task-x1.ready-to-push" "ready record remained after delivery"
  assert_grep 'GH_TOKEN=service-only GITHUB_TOKEN= MX_AGENT_GH_TOKEN= CODEX_THREAD_ID=' \
    "$case_dir/push-env.log" "git push inherited an agent credential or marker"
  assert_grep 'GH_TOKEN=service-only GITHUB_TOKEN= MX_AGENT_GH_TOKEN= CODEX_THREAD_ID=' \
    "$case_dir/gh-env.log" "gh inherited an agent credential or marker"

  MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" >/dev/null 2>&1 \
    || fail "empty post-delivery scan should be idempotent"
  pushes=$(wc -l < "$case_dir/push.log" | tr -d '[:space:]')
  creates=$(grep -c '^pr create ' "$case_dir/gh.log" || true)
  [ "$pushes" = 1 ] || fail "idempotent rerun pushed again"
  [ "$creates" = 1 ] || fail "idempotent rerun created another PR"
  pass "mx-deliver pins the approved SHA, records the PR, sanitizes credentials, and is idempotent"
}

test_head_movement_marks_stale_without_push() {
  local case_dir old_head rc
  case_dir=$(make_case stale-head)
  write_record "$case_dir" approved
  old_head=$("$REAL_GIT" -C "$case_dir/wt" rev-parse HEAD)
  "$REAL_GIT" -C "$case_dir/wt" commit -q --allow-empty -m "post-validation movement"
  set +e
  MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" task-x1 \
    >"$case_dir/out" 2>"$case_dir/err"
  rc=$?
  set -e
  expect_code 1 "$rc" "moved HEAD must be refused"
  assert_grep 'HEAD moved past the approved SHA' "$case_dir/err" "stale SHA refusal was unclear"
  assert_present "$case_dir/state/task-x1.ready-to-push.stale" "stale record was not marked"
  [ ! -s "$case_dir/push.log" ] || fail "stale record pushed"
  "$REAL_GIT" -C "$case_dir/origin.git" rev-parse --verify \
    "refs/heads/mx/task-x1" >/dev/null 2>&1 \
    && fail "stale branch appeared on the remote"
  [ -n "$old_head" ] || fail "fixture approved head was empty"
  pass "mx-deliver refuses and marks a branch that moved after validation"
}

test_agent_ambience_refuses_before_credentials_or_push() {
  local case_dir rc
  case_dir=$(make_case ambience)
  write_record "$case_dir" approved
  set +e
  env CODEX_THREAD_ID= \
    MX_DELIVERY_GH_TOKEN=service-only \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    MX_TEST_PUSH_LOG="$case_dir/push.log" MX_TEST_PUSH_ENV_LOG="$case_dir/push-env.log" \
    MX_TEST_GH_LOG="$case_dir/gh.log" MX_TEST_GH_ENV_LOG="$case_dir/gh-env.log" \
    MX_TEST_WORKTREE="$case_dir/wt" PATH="$case_dir/fakebin:$PATH" \
    "$DELIVER" task-x1 >"$case_dir/out" 2>"$case_dir/err"
  rc=$?
  set -e
  expect_code 3 "$rc" "agent ambience must use the dedicated refusal exit"
  assert_grep 'must run outside every broker, actor, daemon, and gate session' \
    "$case_dir/err" "agent ambience refusal was unclear"
  [ ! -s "$case_dir/push.log" ] || fail "agent ambience reached git push"
  [ ! -s "$case_dir/gh.log" ] || fail "agent ambience reached gh"
  pass "mx-deliver refuses agent-session ambience before remote access"
}

test_record_is_data_not_shell() {
  local case_dir rc marker
  case_dir=$(make_case inert-record)
  write_record "$case_dir" approved
  marker="$case_dir/executed"
  printf 'unknown=$(touch %s)\n' "$marker" >> "$case_dir/state/task-x1.ready-to-push"
  set +e
  MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" task-x1 \
    >"$case_dir/out" 2>"$case_dir/err"
  rc=$?
  set -e
  expect_code 1 "$rc" "unknown record key must be refused"
  assert_absent "$marker" "delivery record content executed as shell"
  [ ! -s "$case_dir/push.log" ] || fail "malformed record pushed"
  pass "mx-deliver parses records as inert closed-schema data"
}

test_spawn_shaped_agent_environment_cannot_push_or_authenticate_gh() {
  local case_dir agent_config rc
  case_dir=$(make_case actor-negative)
  agent_config="$case_dir/agent-gh-config"
  mkdir -p "$agent_config"
  chmod 700 "$agent_config"

  set +e
  env -u GH_TOKEN -u GITHUB_TOKEN -u GH_ENTERPRISE_TOKEN -u GITHUB_ENTERPRISE_TOKEN \
    -u GH_CONFIG_DIR -u SSH_AUTH_SOCK \
    GH_CONFIG_DIR="$agent_config" GH_PROMPT_DISABLED=1 \
    GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/usr/bin/false SSH_ASKPASS=/usr/bin/false \
    GIT_CONFIG_COUNT=2 \
    GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0= \
    GIT_CONFIG_KEY_1=remote.origin.pushurl \
    GIT_CONFIG_VALUE_1=/dev/null/multplx-agent-no-push \
    GIT_SSH_COMMAND='ssh -o BatchMode=yes -o IdentityAgent=none -o IdentitiesOnly=yes -o IdentityFile=/dev/null' \
    "$REAL_GIT" -C "$case_dir/wt" push origin mx/task-x1 \
    >"$case_dir/actor-push.out" 2>"$case_dir/actor-push.err"
  rc=$?
  set -e
  [ "$rc" -ne 0 ] || fail "spawn-shaped agent environment pushed to origin"
  "$REAL_GIT" -C "$case_dir/origin.git" rev-parse --verify refs/heads/mx/task-x1 \
    >/dev/null 2>&1 && fail "actor branch appeared on origin"

  set +e
  env -u GH_TOKEN -u GITHUB_TOKEN -u GH_ENTERPRISE_TOKEN -u GITHUB_ENTERPRISE_TOKEN \
    GH_CONFIG_DIR="$agent_config" GH_PROMPT_DISABLED=1 \
    "$REAL_GH" auth status >"$case_dir/actor-gh.out" 2>"$case_dir/actor-gh.err"
  rc=$?
  set -e
  [ "$rc" -ne 0 ] || fail "spawn-shaped agent environment retained gh authentication"
  pass "spawn-shaped agent environments cannot push origin or authenticate gh"
}

test_empty_scan_and_pending_record_never_push
test_approved_record_delivers_once_and_sanitizes_credentials
test_head_movement_marks_stale_without_push
test_agent_ambience_refuses_before_credentials_or_push
test_record_is_data_not_shell
test_spawn_shaped_agent_environment_cannot_push_or_authenticate_gh
