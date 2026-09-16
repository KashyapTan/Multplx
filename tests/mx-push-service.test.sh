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
    "mode=deep-review"
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
if [ "${*: -3}" = "remote get-url origin" ]; then
  printf '%s\n' "${MX_TEST_ORIGIN_URL:-https://github.com/example/repo.git}"
  exit 0
fi
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
  "pr create"|"pr edit")
    printf '%s\n' "$*" >> "$MX_TEST_GH_LOG"
    while [ "$#" -gt 0 ]; do
      if [ "$1" = --body-file ]; then cp "$2" "$MX_TEST_GH_LOG.body"; break; fi
      if [ "$1" = --body ]; then printf '%s\n' "$2" > "$MX_TEST_GH_LOG.body"; break; fi
      shift
    done
    touch "$MX_TEST_PR_EXISTS"
    printf '%s\n' "${MX_TEST_PR_URL:-https://github.com/example/repo/pull/42}"
    ;;
  "pr list")
    if [ -e "$MX_TEST_PR_EXISTS" ] || [ -n "${MX_TEST_EXISTING_PR_URL:-}" ]; then
      printf '[{"url":"%s","headRefName":"%s","baseRefName":"main","state":"OPEN","isCrossRepository":false}]\n' \
        "${MX_TEST_EXISTING_PR_URL:-${MX_TEST_PR_URL:-https://github.com/example/repo/pull/42}}" \
        "${MX_TEST_PR_HEAD_BRANCH:-mx/task-x1}"
    else
      printf '[]\n'
    fi
    ;;
  "pr view")
    case "$*" in
      *url,headRefName,baseRefName,state,isCrossRepository*)
        printf '{"url":"%s","headRefName":"%s","baseRefName":"main","state":"OPEN","isCrossRepository":false,"headRefOid":"%s"}\n' \
          "${MX_TEST_EXISTING_PR_URL:-${MX_TEST_PR_URL:-https://github.com/example/repo/pull/42}}" \
          "${MX_TEST_PR_HEAD_BRANCH:-mx/task-x1}" \
          "$($REAL_GIT -C "$MX_TEST_WORKTREE" rev-parse HEAD)"
        ;;
      *) "$REAL_GIT" -C "$MX_TEST_WORKTREE" rev-parse HEAD ;;
    esac
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
    MX_TEST_PR_EXISTS="$case_dir/pr-exists" \
    MX_TEST_WORKTREE="$case_dir/wt" \
    PATH="$case_dir/fakebin:$PATH" \
    "$DELIVER" "$@"
}

canonicalize_task() {
  local case_dir=$1 remote_url=${2:-https://github.com/example/repo.git} meta model root binding local_origin
  meta="$case_dir/state/task-x1.meta"
  local_origin=$($REAL_GIT -C "$case_dir/project" remote get-url origin)
  $REAL_GIT -C "$case_dir/project" remote set-url origin "$remote_url"
  mv "$meta" "$meta.hold"
  binding=$(MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/target/release/mx" project register "$case_dir/project") \
    || fail 'project fixture registration failed'
  mv "$meta.hold" "$meta"
  $REAL_GIT -C "$case_dir/project" remote set-url origin "$local_origin"
  model=$(MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/target/release/mx" task-model inspect task-x1) || fail 'legacy fixture inspect failed'
  root="root-home:$case_dir"
  model=$(printf '%s' "$model" | jq -c --argjson binding "$binding" \
    --arg home "$case_dir" --arg state "$case_dir/state" --arg root "$root" '
      .legacy_unknown=false |
      .owner_home=$home | .owner_state=$state | .parent_home=$home | .parent_state=$state |
      .parent_id=$root | .root_id=$root |
      .project=$binding |
      .attempt={id:"attempt-task-x1",generation:1,brief_revision:1} |
      .allocation={
        allocation_id:"allocation-task-x1",lease_id:"lease-task-x1",generation:1,
        project_id:$binding.project_id,checkout_id:$binding.checkout_id,
        common_git_identity:$binding.common_git_identity,path:($home+"/wt"),
        base_revision:$binding.starting_revision,task_id:"task-x1",
        attempt_id:"attempt-task-x1",persistent:false
      } |
      .accepted_brief_revision=1 |
      .briefs=[{revision:1,scope:"publish task-x1",acceptance_criteria:[],source_artifacts:[],reason:"test"}] |
      .assignments=[{generation:1,role:.role,brief_revision:1,reason:"test"}]') \
    || fail 'canonical fixture model failed'
  printf 'schema_version=2\ncanonical_model=%s\n' "$model" >> "$meta"
  chmod 600 "$meta"
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
  assert_grep 'legacy pending handoff' "$case_dir/err" "pending refusal was unclear"
  [ ! -s "$case_dir/push.log" ] || fail "pending record pushed"
  [ ! -s "$case_dir/gh.log" ] || fail "pending record opened a PR"
  pass "mx-deliver never promotes legacy pending records"
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
  assert_grep 'GH_TOKEN=service-only GITHUB_TOKEN= MX_AGENT_GH_TOKEN=read-leak CODEX_THREAD_ID=' \
    "$case_dir/push-env.log" "explicit credential override was not scoped to forge authentication"
  assert_grep 'GH_TOKEN=service-only GITHUB_TOKEN= MX_AGENT_GH_TOKEN=read-leak CODEX_THREAD_ID=' \
    "$case_dir/gh-env.log" "gh did not receive the explicit credential override"

  MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" >/dev/null 2>&1 \
    || fail "empty post-delivery scan should be idempotent"
  pushes=$(wc -l < "$case_dir/push.log" | tr -d '[:space:]')
  creates=$(grep -c '^pr create ' "$case_dir/gh.log" || true)
  [ "$pushes" = 1 ] || fail "idempotent rerun pushed again"
  [ "$creates" = 1 ] || fail "idempotent rerun created another PR"
  pass "mx-deliver pins the exact SHA, registers one PR, scopes an explicit credential override, and is idempotent"
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

test_agent_ambience_can_publish_with_ordinary_credentials() {
  local case_dir
  case_dir=$(make_case ambience)
  write_record "$case_dir" approved
  env CODEX_THREAD_ID=agent-session GH_TOKEN=ordinary-agent-token \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    MX_TEST_PUSH_LOG="$case_dir/push.log" MX_TEST_PUSH_ENV_LOG="$case_dir/push-env.log" \
    MX_TEST_GH_LOG="$case_dir/gh.log" MX_TEST_GH_ENV_LOG="$case_dir/gh-env.log" \
    MX_TEST_PR_EXISTS="$case_dir/pr-exists" MX_TEST_WORKTREE="$case_dir/wt" PATH="$case_dir/fakebin:$PATH" \
    "$DELIVER" task-x1 >"$case_dir/out" 2>"$case_dir/err"
  assert_grep 'GH_TOKEN=ordinary-agent-token' "$case_dir/push-env.log" \
    "agent publication did not inherit ordinary authentication"
  assert_grep 'CODEX_THREAD_ID=agent-session' "$case_dir/gh-env.log" \
    "publication stripped the task identity marker"
  pass "mx-deliver allows agent publication with ordinary inherited authentication"
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

test_exact_sha_validation_waiver_stays_truthful() {
  local case_dir head bindings request operation target state_digest
  case_dir=$(make_case waived-validation)
  head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  jq '.status="failed" | .summary="Validation failed and was explicitly waived." | .risk_level="high" | .risk_rationale="A known validation failure remains."' \
    "$case_dir/state/task-x1.gate/run.json" >"$case_dir/run.json"
  mv "$case_dir/run.json" "$case_dir/state/task-x1.gate/run.json"
  chmod 600 "$case_dir/state/task-x1.gate/run.json"
  bindings=$(MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/bin/mx-override-bindings.sh" validation task-x1 "$head") \
    || fail "waived validation bindings failed"
  operation=$(printf '%s' "$bindings" | jq -r '.operation')
  target=$(printf '%s' "$bindings" | jq -r '.target')
  state_digest=$(printf '%s' "$bindings" | jq -r '.expected_state_digest')
  request=$(MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/bin/mx-maintainer-override.sh" request --boundary validation.waive-gate \
      --task task-x1 --project "$(printf '%s' "$bindings" | jq -r '.project')" --operation "$operation" --target "$target" \
      --expected-state "$state_digest" --consequence "Create a maintainer-waived delivery handoff for this exact SHA without recording validation as passed.") \
    || fail "validation waiver request failed"
  MX_HOME="$case_dir"
  MX_STATE_OVERRIDE="$case_dir/state"
  export MX_HOME MX_STATE_OVERRIDE
  # shellcheck source=bin/mx-maintainer-override-lib.sh
  . "$ROOT/bin/mx-maintainer-override-lib.sh"
  mx_override_require_primary_lock() { return 0; }
  mx_override_grant "$request" "Grant validation.waive-gate for $operation on $target only." \
    || fail "validation waiver grant failed"
  "$ROOT/bin/mx-validation-waive.sh" task-x1 "$head" "$request" --title "Waived validation delivery" \
    >/dev/null || fail "exact-SHA validation waiver failed"
  [ "$(jq -r '.status' "$case_dir/state/task-x1.gate/run.json")" = failed ] \
    || fail "validation waiver fabricated a passed gate"
  assert_grep 'validation=waived' "$case_dir/state/task-x1.ready-to-push" \
    "waived handoff is not labeled waived"
  sed 's/^approval=pending$/approval=approved/' "$case_dir/state/task-x1.ready-to-push" \
    >"$case_dir/approved"
  mv "$case_dir/approved" "$case_dir/state/task-x1.ready-to-push"
  chmod 600 "$case_dir/state/task-x1.ready-to-push"
  MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" task-x1 \
    >"$case_dir/out" 2>"$case_dir/err" || fail "truthful waived delivery was not accepted"
  assert_grep 'Maintainer-waived for exact SHA' "$case_dir/gh.log" \
    "delivery body did not disclose that validation was waived"
  pass "one exact validation SHA is waived without ever being recorded as passed"
}

test_empty_scan_and_pending_record_never_push
test_approved_record_delivers_once_and_sanitizes_credentials
test_head_movement_marks_stale_without_push
test_agent_ambience_can_publish_with_ordinary_credentials
test_record_is_data_not_shell
test_exact_sha_validation_waiver_stays_truthful

test_direct_pr_owned_handoff() {
  local case_dir head pushes creates
  case_dir=$(make_case direct-pr)
  canonicalize_task "$case_dir"
  head=$(git -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$head" --summary 'Direct change' \
    --checks 'passed: focused check' --limitations 'none reported' \
    >"$case_dir/out" 2>"$case_dir/err" || fail "ordinary prepare failed: $(cat "$case_dir/err")"
  assert_grep 'version=4' "$case_dir/state/task-x1.ready-to-push" "ordinary schema absent"
  assert_no_grep 'approval=' "$case_dir/state/task-x1.ready-to-push" "ordinary publication retained approval"
  if run_delivery "$case_dir" approve task-x1 --sha "$head" >/dev/null 2>&1; then fail "retired approval accepted"; fi
  run_delivery "$case_dir" task-x1 >"$case_dir/out" 2>"$case_dir/err" || fail "direct delivery failed: $(cat "$case_dir/err")"
  [ -f "$case_dir/state/task-x1.delivered" ] || fail "publication request not archived"
  assert_grep 'focused check' "$case_dir/gh.log.body" 'reported check missing from PR body'
  assert_grep 'pr=https://github.com/example/repo/pull/42' "$case_dir/state/task-x1.meta" 'canonical PR was not registered'
  pushes=$(wc -l < "$case_dir/push.log" | tr -d ' ')
  creates=$(grep -c '^pr create ' "$case_dir/gh.log" || true)
  run_delivery "$case_dir" task-x1 >"$case_dir/retry-out" 2>"$case_dir/retry-err" \
    || fail "completed publication retry did not reconcile: $(cat "$case_dir/retry-err")"
  assert_grep 'already published' "$case_dir/retry-out" 'completed retry did not report reconciliation'
  [ "$(wc -l < "$case_dir/push.log" | tr -d ' ')" -eq "$pushes" ] \
    || fail 'completed retry pushed again'
  [ "$(grep -c '^pr create ' "$case_dir/gh.log" || true)" -eq "$creates" ] \
    || fail 'completed retry created another PR'
  printf 'new head after publication\n' >> "$case_dir/wt/change.txt"
  $REAL_GIT -C "$case_dir/wt" add change.txt
  $REAL_GIT -C "$case_dir/wt" commit -q -m 'new head after publication'
  if run_delivery "$case_dir" task-x1 >"$case_dir/stale-out" 2>"$case_dir/stale-err"; then
    fail 'completed retry resurrected a receipt after HEAD changed'
  fi
  pass "ordinary agent publication records exact revision evidence without approval"
}
test_direct_pr_owned_handoff

test_direct_registration_cannot_rewind_current_revision() {
  local case_dir old_head new_head model
  case_dir=$(make_case direct-registration-stale)
  canonicalize_task "$case_dir"
  old_head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$old_head" --summary 'Current evidence' >/dev/null \
    || fail 'direct registration stale prepare failed'
  printf 'newer revision\n' >> "$case_dir/wt/change.txt"
  $REAL_GIT -C "$case_dir/wt" add change.txt
  $REAL_GIT -C "$case_dir/wt" commit -q -m 'newer revision'
  new_head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  if env -u CODEX_THREAD_ID -u CLAUDECODE -u PI_CODING_AGENT \
    MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    MX_TEST_PUSH_LOG="$case_dir/push.log" MX_TEST_PUSH_ENV_LOG="$case_dir/push-env.log" \
    MX_TEST_GH_LOG="$case_dir/gh.log" MX_TEST_GH_ENV_LOG="$case_dir/gh-env.log" \
    MX_TEST_PR_EXISTS="$case_dir/pr-exists" MX_TEST_WORKTREE="$case_dir/wt" \
    PATH="$case_dir/fakebin:$PATH" \
    "$ROOT/bin/mx-pr-check.sh" task-x1 https://github.com/example/repo/pull/42 \
    >"$case_dir/out" 2>"$case_dir/err"; then
    fail 'stale direct PR registration rewound current delivery evidence'
  fi
  assert_grep 'differs from the current delivery revision' "$case_dir/err" \
    'stale direct registration refusal was unclear'
  model=$(MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/target/release/mx" task-model inspect task-x1) || fail 'stale registration inspect failed'
  [ "$(printf '%s' "$model" | jq -r '.delivery.current_commit')" = "$old_head" ] \
    || fail 'stale registration replaced current delivery revision'
  if printf '%s' "$model" | jq -e --arg head "$new_head" \
    '.delivery.history[] | select(.commit == $head and .outcome == "published")' >/dev/null; then
    fail 'stale registration emitted a current published outcome'
  fi
  pass 'direct PR registration cannot rewind newer revision-bound evidence'
}
test_direct_registration_cannot_rewind_current_revision

test_user_owned_local_checkout_is_retained() {
  local case_dir before_status before_head after_status after_head
  case_dir=$(make_case user-owned-local)
  canonicalize_task "$case_dir"
  sed 's/^mode=deep-review$/mode=local-only/' "$case_dir/state/task-x1.meta" > "$case_dir/meta"
  mv "$case_dir/meta" "$case_dir/state/task-x1.meta"
  chmod 600 "$case_dir/state/task-x1.meta"
  printf 'borrowed source material\n' > "$case_dir/project/borrowed.txt"
  before_status=$($REAL_GIT -C "$case_dir/project" status --porcelain)
  before_head=$($REAL_GIT -C "$case_dir/project" rev-parse HEAD)
  if MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/bin/mx-merge-local.sh" task-x1 >"$case_dir/out" 2>"$case_dir/err"; then
    fail 'user-owned source checkout was locally landed'
  fi
  assert_grep 'user-owned' "$case_dir/err" 'user-owned local landing refusal was unclear'
  after_status=$($REAL_GIT -C "$case_dir/project" status --porcelain)
  after_head=$($REAL_GIT -C "$case_dir/project" rev-parse HEAD)
  [ "$after_status" = "$before_status" ] || fail 'user-owned source index or files changed'
  [ "$after_head" = "$before_head" ] || fail 'user-owned source branch moved'
  pass 'local outcome retains a dirty user-owned checkout and task branch for human integration'
}
test_user_owned_local_checkout_is_retained

test_concurrent_task_bound_publications_stay_isolated() {
  local first second first_head second_head first_pid second_pid
  first=$(make_case concurrent-one)
  second=$(make_case concurrent-two)
  canonicalize_task "$first" https://github.com/example/repo-one.git
  canonicalize_task "$second" https://github.com/example/repo-two.git
  first_head=$($REAL_GIT -C "$first/wt" rev-parse HEAD)
  second_head=$($REAL_GIT -C "$second/wt" rev-parse HEAD)
  MX_TEST_ORIGIN_URL=https://github.com/example/repo-one.git \
    run_delivery "$first" prepare task-x1 --sha "$first_head" --summary 'First forge' >/dev/null \
    || fail 'first concurrent prepare failed'
  MX_TEST_ORIGIN_URL=https://github.com/example/repo-two.git \
    run_delivery "$second" prepare task-x1 --sha "$second_head" --summary 'Second forge' >/dev/null \
    || fail 'second concurrent prepare failed'
  (
    MX_TEST_ORIGIN_URL=https://github.com/example/repo-one.git \
      MX_TEST_PR_URL=https://github.com/example/repo-one/pull/42 \
      run_delivery "$first" task-x1 >"$first/out" 2>"$first/err"
  ) & first_pid=$!
  (
    MX_TEST_ORIGIN_URL=https://github.com/example/repo-two.git \
      MX_TEST_PR_URL=https://github.com/example/repo-two/pull/42 \
      run_delivery "$second" task-x1 >"$second/out" 2>"$second/err"
  ) & second_pid=$!
  wait "$first_pid" || fail "first concurrent publication failed: $(cat "$first/err")"
  wait "$second_pid" || fail "second concurrent publication failed: $(cat "$second/err")"
  assert_grep 'pr=https://github.com/example/repo-one/pull/42' "$first/state/task-x1.meta" \
    'first publication crossed forge identity'
  assert_grep 'pr=https://github.com/example/repo-two/pull/42' "$second/state/task-x1.meta" \
    'second publication crossed forge identity'
  assert_no_grep 'repo-two' "$first/state/task-x1.meta" 'second forge leaked into first task outcome'
  assert_no_grep 'repo-one' "$second/state/task-x1.meta" 'first forge leaked into second task outcome'
  pass 'concurrent task-bound publications keep distinct repositories and outcomes isolated'
}
test_concurrent_task_bound_publications_stay_isolated

test_direct_pr_stale_binding_never_pushes() {
  local case_dir head
  case_dir=$(make_case direct-stale)
  canonicalize_task "$case_dir"
  head=$(git -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$head" --summary 'Direct stale control' >/dev/null || fail 'prepare stale fixture failed'
  printf 'new dirty material\n' > "$case_dir/wt/change.txt"
  if run_delivery "$case_dir" task-x1 >/dev/null 2>&1; then fail 'dirty approved direct handoff delivered'; fi
  [ -f "$case_dir/state/task-x1.ready-to-push.stale" ] || fail 'dirty direct record not marked stale'
  [ ! -s "$case_dir/push.log" ] || fail 'dirty direct work pushed'
  pass 'direct-PR rechecks an approved worktree and refuses stale material before transport'
}
test_direct_pr_stale_binding_never_pushes

test_task_bound_origin_and_pr_repository_are_enforced() {
  local case_dir head model
  case_dir=$(make_case task-bound-forge)
  canonicalize_task "$case_dir"
  head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$head" --summary 'Bound forge' \
    >/dev/null || fail 'bound forge prepare failed'
  $REAL_GIT -C "$case_dir/project" worktree add -q -b mx/foreign-allocation \
    "$case_dir/foreign-wt" main
  model=$(MX_HOME="$case_dir" MX_STATE_OVERRIDE="$case_dir/state" \
    "$ROOT/target/release/mx" task-model inspect task-x1) || fail 'allocation fixture inspect failed'
  model=$(printf '%s' "$model" | jq -c --arg path "$case_dir/foreign-wt" '.allocation.path=$path')
  sed '/^canonical_model=/d' "$case_dir/state/task-x1.meta" > "$case_dir/meta"
  printf 'canonical_model=%s\n' "$model" >> "$case_dir/meta"
  mv "$case_dir/meta" "$case_dir/state/task-x1.meta"
  chmod 600 "$case_dir/state/task-x1.meta"
  if run_delivery "$case_dir" task-x1 >"$case_dir/allocation-out" 2>"$case_dir/allocation-err"; then
    fail 'same-repository foreign allocation published'
  fi
  assert_grep 'differs from the current task allocation' "$case_dir/allocation-err" \
    'foreign allocation refusal was unclear'
  model=$(printf '%s' "$model" | jq -c --arg path "$case_dir/wt" '.allocation.path=$path')
  sed '/^canonical_model=/d' "$case_dir/state/task-x1.meta" > "$case_dir/meta"
  printf 'canonical_model=%s\n' "$model" >> "$case_dir/meta"
  mv "$case_dir/meta" "$case_dir/state/task-x1.meta"
  chmod 600 "$case_dir/state/task-x1.meta"
  if MX_TEST_ORIGIN_URL=https://github.com/other/repo.git \
    run_delivery "$case_dir" task-x1 >"$case_dir/origin-out" 2>"$case_dir/origin-err"; then
    fail 'context-switched origin published'
  fi
  assert_grep 'origin differs from task-bound' "$case_dir/origin-err" \
    'context-switched origin refusal was unclear'
  [ ! -s "$case_dir/push.log" ] || fail 'context-switched origin reached push'
  if MX_TEST_EXISTING_PR_URL=https://github.com/other/repo/pull/42 \
    run_delivery "$case_dir" task-x1 >"$case_dir/pr-out" 2>"$case_dir/pr-err"; then
    fail 'foreign repository PR was accepted'
  fi
  assert_grep 'PR repository differs from the task-bound' "$case_dir/pr-err" \
    'foreign PR refusal was unclear'
  [ ! -s "$case_dir/push.log" ] || fail 'foreign PR reached push'
  if MX_TEST_EXISTING_PR_URL=https://github.com/example/repo/pull/42 \
    MX_TEST_PR_HEAD_BRANCH=mx/other-task \
    run_delivery "$case_dir" task-x1 >"$case_dir/branch-out" 2>"$case_dir/branch-err"; then
    fail 'same-repository foreign branch PR was accepted'
  fi
  assert_grep 'branch, base, or repository changed' "$case_dir/branch-err" \
    'foreign branch PR refusal was unclear'
  [ ! -s "$case_dir/push.log" ] || fail 'foreign branch PR reached push'
  pass 'publication remains bound to the task project, origin, and PR repository'
}
test_task_bound_origin_and_pr_repository_are_enforced

test_unrelated_publication_history_is_refused() {
  local case_dir head
  case_dir=$(make_case unrelated-history)
  canonicalize_task "$case_dir"
  $REAL_GIT -C "$case_dir/wt" checkout -q --orphan unrelated
  $REAL_GIT -C "$case_dir/wt" rm -q -rf .
  printf 'unrelated\n' > "$case_dir/wt/unrelated.txt"
  $REAL_GIT -C "$case_dir/wt" add unrelated.txt
  $REAL_GIT -C "$case_dir/wt" commit -q -m unrelated
  $REAL_GIT -C "$case_dir/wt" branch -D mx/task-x1 >/dev/null
  $REAL_GIT -C "$case_dir/wt" branch -m mx/task-x1
  head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$head" --summary 'Unrelated history' >/dev/null \
    || fail 'unrelated history prepare failed'
  if run_delivery "$case_dir" task-x1 >"$case_dir/out" 2>"$case_dir/err"; then
    fail 'unrelated publication history was accepted'
  fi
  assert_grep 'unrelated to the task starting revision' "$case_dir/err" \
    'unrelated history refusal was unclear'
  [ ! -s "$case_dir/push.log" ] || fail 'unrelated history reached push'
  pass 'publication head must descend from the immutable task starting revision'
}
test_unrelated_publication_history_is_refused

test_pr_created_before_registration_retries_without_duplicate() {
  local case_dir head creates edits_before edits_after
  case_dir=$(make_case retry-created-pr)
  canonicalize_task "$case_dir"
  head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$head" --summary 'Retry-safe publication' \
    >/dev/null || fail 'retry fixture prepare failed'
  if MX_PR_CHECK_FAULT_AFTER_STAGE=1 run_delivery "$case_dir" task-x1 \
    >"$case_dir/first-out" 2>"$case_dir/first-err"; then
    fail 'injected PR registration failure unexpectedly completed'
  fi
  assert_present "$case_dir/state/task-x1.ready-to-push" 'failed registration lost publication request'
  assert_grep '"stage": "pr-observed"' "$case_dir/state/task-x1.publication-"* \
    'PR-observed operation receipt was not durable'
  creates=$(grep -c '^pr create ' "$case_dir/gh.log" || true)
  [ "$creates" -eq 1 ] || fail "first attempt created $creates PRs"
  run_delivery "$case_dir" task-x1 >"$case_dir/retry-out" 2>"$case_dir/retry-err" \
    || fail "PR-created retry did not reconcile: $(cat "$case_dir/retry-err")"
  [ "$(grep -c '^pr create ' "$case_dir/gh.log" || true)" -eq 1 ] \
    || fail 'retry created a duplicate PR'
  assert_absent "$case_dir/state/task-x1.ready-to-push" 'successful retry left request pending'

  case_dir=$(make_case retry-pr-mismatch)
  canonicalize_task "$case_dir"
  head=$($REAL_GIT -C "$case_dir/wt" rev-parse HEAD)
  run_delivery "$case_dir" prepare task-x1 --sha "$head" --summary 'Receipt identity' \
    >/dev/null || fail 'mismatch fixture prepare failed'
  MX_PR_CHECK_FAULT_AFTER_STAGE=1 run_delivery "$case_dir" task-x1 \
    >"$case_dir/first-out" 2>"$case_dir/first-err" || true
  edits_before=$(grep -c '^pr edit ' "$case_dir/gh.log" || true)
  if MX_TEST_EXISTING_PR_URL=https://github.com/example/repo/pull/43 \
    run_delivery "$case_dir" task-x1 >"$case_dir/mismatch-out" 2>"$case_dir/mismatch-err"; then
    fail 'changed PR identity was accepted on retry'
  fi
  edits_after=$(grep -c '^pr edit ' "$case_dir/gh.log" || true)
  [ "$edits_after" -eq "$edits_before" ] || fail 'changed PR was edited before receipt mismatch refusal'
  assert_no_grep 'pr edit https://github.com/example/repo/pull/43' "$case_dir/gh.log" \
    'foreign retry PR was mutated'
  pass 'PR-created retries converge on one canonical PR and reject changed receipt identity'
}
test_pr_created_before_registration_retries_without_duplicate

# A later approved revision updates the same PR and retains the old exact receipt.
test_existing_pr_revision_preserves_receipt_and_refuses_unsafe_history() {
  local case_dir old_sha new_sha archive kind
  for kind in success symlink conflict wrong-pr; do
    case_dir=$(make_case "revision-$kind")
    write_record "$case_dir" approved
    MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" task-x1 >"$case_dir/first-out" 2>"$case_dir/first-err" || fail 'initial revision delivery failed'
    old_sha=$("$REAL_GIT" -C "$case_dir/wt" rev-parse HEAD)
    cp "$case_dir/state/task-x1.delivered" "$case_dir/prior-receipt"
    "$REAL_GIT" -C "$case_dir/wt" commit -q --allow-empty -m 'approved CI correction'
    new_sha=$("$REAL_GIT" -C "$case_dir/wt" rev-parse HEAD)
    jq --arg sha "$new_sha" '.approved_head=$sha | .summary="CI correction validated"' "$case_dir/state/task-x1.gate/run.json" > "$case_dir/run-next"
    cp "$case_dir/run-next" "$case_dir/state/task-x1.gate/run.json"
    write_record "$case_dir" approved
    archive="$case_dir/state/task-x1.delivered-$old_sha"
    case "$kind" in
      symlink) ln -s "$case_dir/prior-receipt" "$archive" ;;
      conflict) printf 'conflicting evidence\n' > "$archive"; chmod 600 "$archive" ;;
    esac
    if [ "$kind" = success ]; then
      MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" task-x1 >"$case_dir/second-out" 2>"$case_dir/second-err" || fail 'second approved revision did not deliver'
      cmp "$case_dir/prior-receipt" "$archive" || fail 'prior receipt bytes changed'
      assert_grep "approved_sha=$new_sha" "$case_dir/state/task-x1.delivered" 'new receipt does not bind new SHA'
      assert_grep "$new_sha:refs/heads/mx/task-x1" "$case_dir/push.log" 'update push did not pin exact SHA'
      [ "$(grep -c '^pr create ' "$case_dir/gh.log")" -eq 1 ] || fail 'revision created another PR'
      assert_grep 'pr edit https://github.com/example/repo/pull/42 ' "$case_dir/gh.log" 'recorded PR was not updated'
      assert_grep 'CI correction validated' "$case_dir/gh.log.body" 'PR content retained stale validation summary'
      assert_absent "$case_dir/state/task-x1.ready-to-push" 'successful update left ready record'
    else
      if MX_TEST_EXISTING_PR_URL="https://github.com/example/repo/pull/99" MX_DELIVERY_GH_TOKEN=service-only run_delivery "$case_dir" task-x1 >"$case_dir/refused-out" 2>"$case_dir/refused-err"; then fail "unsafe $kind update passed"; fi
      [ "$(wc -l < "$case_dir/push.log" | tr -d '[:space:]')" -eq 1 ] || fail "unsafe $kind update pushed"
      cmp "$case_dir/prior-receipt" "$case_dir/state/task-x1.delivered" || fail "unsafe $kind update changed receipt"
      assert_present "$case_dir/state/task-x1.ready-to-push" 'refused update lost pending evidence'
    fi
  done
  pass 'existing PR revisions preserve receipts and refuse unsafe history or changed PR identity before push'
}
test_existing_pr_revision_preserves_receipt_and_refuses_unsafe_history
