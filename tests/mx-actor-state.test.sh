#!/usr/bin/env bash
# Reconciliation tests for deep-review run records, native events, reports, and panes.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_git_identity actor-state-tests actor-state-tests@example.invalid

STATE_BIN="$ROOT/bin/mx-actor-state.sh"
TMP_ROOT=$(mx_test_tmproot mx-actor-state)

make_fake_tmux() {
  local fakebin=$1
  mkdir -p "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
case "$1" in
  display-message)
    [ "${MX_FAKE_PANE_GONE:-0}" != 1 ] || exit 1
    printf '%%1\n'
    ;;
  capture-pane)
    printf '%s\n' "${MX_FAKE_PANE_TEXT:-idle prompt}"
    ;;
  *) exit 0 ;;
esac
SH
  chmod +x "$fakebin/tmux"
}

make_case() {
  local name=$1 case_dir repo state id head
  case_dir="$TMP_ROOT/$name"
  repo="$case_dir/repo"
  state="$case_dir/state"
  id="state-$name"
  mkdir -p "$state"
  mx_git_init_commit "$repo"
  repo="$(cd "$repo" && pwd -P)"
  case_dir="$(cd "$case_dir" && pwd -P)"
  state="$case_dir/state"
  git -C "$repo" branch -M main
  git -C "$repo" checkout -qb "mx/$id"
  git -C "$repo" commit -q --allow-empty -m change
  head=$(git -C "$repo" rev-parse HEAD)
  mx_write_meta "$state/$id.meta" \
    "window=mx-$id" "worktree=$repo" "project=$repo" \
    "kind=delivery" "mode=deep-review"
  make_fake_tmux "$case_dir/fakebin"
  printf '%s\t%s\t%s\t%s\t%s\n' "$case_dir" "$repo" "$state" "$id" "$head"
}

write_run() {
  local state=$1 id=$2 repo=$3 head=$4 status=$5 step=$6
  mkdir -p "$state/$id.gate/findings"
  jq -n --arg task "$id" --arg worktree "$repo" --arg branch "mx/$id" \
    --arg head "$head" --arg status "$status" --arg step "$step" \
    '{version:1,task:$task,worktree:$worktree,branch:$branch,
      approved_head:$head,status:$status,step:$step,round:2}' \
    > "$state/$id.gate/run.json"
}

run_state() {
  local case_dir=$1 state=$2 id=$3
  shift 3
  PATH="$case_dir/fakebin:$PATH" MX_HOME="$(dirname "$state")" \
    MX_STATE_OVERRIDE="$state" "$@" "$STATE_BIN" "$id"
}

test_running_parked_passed_and_failed() {
  local case_dir repo state id head out status expected
  IFS=$'\t' read -r case_dir repo state id head <<EOF
$(make_case states)
EOF
  for status in running parked passed failed; do
    write_run "$state" "$id" "$repo" "$head" "$status" review
    out=$(run_state "$case_dir" "$state" "$id" env MX_FAKE_PANE_GONE=1)
    case "$status" in
      running) expected='state: working · source: run-step · validating (review round 2)' ;;
      parked) expected='state: parked · source: run-step · parked at review round 2' ;;
      passed) expected='state: done · source: run-step · validated local branch' ;;
      failed) expected='state: failed · source: run-step · validation failed at review' ;;
    esac
    assert_contains "$out" "$expected" "run status $status mapped incorrectly"
  done
  pass "actor-state maps attributed deep-review states even after endpoint exit"
}

test_exact_head_and_binding_attribution() {
  local case_dir repo state id head out
  IFS=$'\t' read -r case_dir repo state id head <<EOF
$(make_case binding)
EOF
  write_run "$state" "$id" "$repo" "$head" running test
  git -C "$repo" commit -q --allow-empty -m newer
  out=$(run_state "$case_dir" "$state" "$id" env MX_FAKE_PANE_TEXT='Working... esc to interrupt')
  assert_contains "$out" 'source: pane' "stale approved head was attributed"

  head=$(git -C "$repo" rev-parse HEAD)
  write_run "$state" "$id" "$repo" "$head" running test
  jq '.worktree="/wrong"' "$state/$id.gate/run.json" > "$state/$id.gate/tmp"
  mv "$state/$id.gate/tmp" "$state/$id.gate/run.json"
  out=$(run_state "$case_dir" "$state" "$id" env)
  assert_contains "$out" 'state: unknown · source: none · invalid deep-review run record' \
    "unsafe run binding did not fail closed"
  pass "actor-state attributes only exact current code and fails closed on unsafe bindings"
}

test_native_precedence_and_stale_status() {
  local case_dir repo state id head out
  IFS=$'\t' read -r case_dir repo state id head <<EOF
$(make_case precedence)
EOF
  write_run "$state" "$id" "$repo" "$head" running lint
  printf 'needs-decision [key=old]: stale choice\n' > "$state/$id.status"
  out=$(run_state "$case_dir" "$state" "$id" env)
  assert_contains "$out" 'status-log superseded by deep-review run' \
    "active run did not supersede stale decision event"

  # A fake Herdr adapter is unnecessary here: the shared resolver is directly
  # covered by mx-signal-precedence.test.sh.
  write_run "$state" "$id" "$repo" "$head" parked review
  out=$(run_state "$case_dir" "$state" "$id" env)
  assert_contains "$out" 'state: parked · source: run-step' \
    "parked run was hidden by matching status history"
  pass "actor-state reconciles stale status history under the shared precedence"
}

test_no_run_falls_back_to_report_then_pane() {
  local case_dir repo state id head out
  IFS=$'\t' read -r case_dir repo state id head <<EOF
$(make_case fallback)
EOF
  printf 'paused: release window\n' > "$state/$id.status"
  out=$(run_state "$case_dir" "$state" "$id" env)
  assert_contains "$out" 'state: paused · source: status-log · release window' \
    "schema-valid report fallback changed"
  rm "$state/$id.status"
  out=$(run_state "$case_dir" "$state" "$id" env MX_FAKE_PANE_TEXT='Working... esc to interrupt')
  assert_contains "$out" 'state: working · source: pane · harness busy' \
    "busy-pane fallback changed"
  pass "actor-state preserves validated-report and pane fallback without a gate run"
}

test_running_parked_passed_and_failed
test_exact_head_and_binding_attribution
test_native_precedence_and_stale_status
test_no_run_falls_back_to_report_then_pane

test_recorded_backend_projection() (
  local case_dir repo state id head backend output endpoint
  IFS=$'\t' read -r case_dir repo state id head <<EOF
$(make_case recorded)
EOF
  cat > "$case_dir/fakebin/herdr" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$MX_FAKE_HERDR_LOG"
[ "$1" != server ] || { echo forbidden-server-start >> "$MX_FAKE_HERDR_LOG"; exit 9; }
[ "${MX_FAKE_UNREADABLE:-0}" != 1 ] || { echo 'fixture backend unavailable' >&2; exit 1; }
[ "${MX_FAKE_STOPPED:-0}" != 1 ] || { echo '{"error":{"code":"server_unavailable"}}'; exit 1; }
case "$1 ${2:-}" in
  'pane get') if [ "${MX_FAKE_MISSING:-0}" = 1 ]; then echo '{"error":{"code":"pane_not_found"}}'; else echo '{"result":{"pane":{"pane_id":"w1:p2"}}}'; fi ;;
  'agent get')
    case "${MX_FAKE_AGENT_FAILURE:-}" in
      transport) echo 'fixture agent socket unavailable' >&2; exit 1 ;;
      api) echo '{"error":{"code":"agent_read_failed","message":"fixture agent socket unavailable"}}'; exit 1 ;;
      malformed) echo '{"result":{"agent":{"unexpected":"fixture unknown status"}}}'; exit 0 ;;
      unknown) echo '{"result":{"agent":{"agent_status":"fixture unsupported status"}}}'; exit 0 ;;
    esac
    if [ "${MX_FAKE_NO_AGENT:-0}" = 1 ]; then echo '{"error":{"code":"agent_not_found"}}'; else echo '{"result":{"agent":{"agent_status":"working"}}}'; fi ;;
  'status --json') echo '{"client":{"version":"0.7.4","protocol":16},"server":{"running":true}}' ;;
  'pane read') echo "${MX_FAKE_PANE_TEXT:-Working}" ;;
  *) exit 2 ;;
esac
SH
  cat > "$case_dir/fakebin/cmux" <<'SH'
#!/usr/bin/env bash
[ "${MX_FAKE_UNREADABLE:-0}" != 1 ] || { echo 'fixture backend unavailable' >&2; exit 1; }
[ "${MX_FAKE_MALFORMED:-0}" != 1 ] || { echo '{"unreadable":true}'; exit 0; }
case "$1" in
  list-panes) if [ "${MX_FAKE_MISSING:-0}" = 1 ]; then echo '{"panes":[]}'; else jq -nc --arg surface "${MX_FAKE_SURFACE:-s1}" '{panes:[{surface_ids:[$surface],selected_surface_id:$surface}]}'; fi ;;
  workspace) jq -nc --arg title "$MX_FAKE_CMUX_TITLE" '{workspaces:[{id:"w1",title:$title}]}' ;;
  read-screen) echo '{"text":"Working... esc to interrupt"}' ;;
  *) exit 2 ;;
esac
SH
  chmod +x "$case_dir/fakebin/herdr" "$case_dir/fakebin/cmux"
  export PATH="$case_dir/fakebin:$PATH" MX_HOME="$case_dir" MX_STATE_OVERRIDE="$state" MX_ROOT_OVERRIDE="$ROOT"
  export MX_FAKE_HERDR_LOG="$case_dir/herdr.log"
  export MX_FAKE_CMUX_TITLE="mx-broker-$(printf '%s' "$ROOT" | shasum -a 256 | cut -c1-8)-$id"
  export MX_HERDR_BIN="$case_dir/fakebin/herdr" MX_BACKEND_CMUX_BIN="$case_dir/fakebin/cmux"
  for backend in herdr cmux; do
    endpoint='named:w1:p2'; [ "$backend" != cmux ] || endpoint='w1:s1'
    mx_write_meta "$state/$id.meta" "window=$endpoint" "backend=$backend" "worktree=$repo" 'kind=scout'
    output=$("$STATE_BIN" "$id") || fail "$backend actor-state dispatch refused"
    if [ "$backend" = herdr ]; then assert_contains "$output" 'state: working' 'Herdr native state lost'; fi
    "$ROOT/bin/mx-doctor.sh" --check stateless-sessions --json > "$case_dir/doctor" || fail "healthy $backend doctor refused"
    "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot" || fail "$backend snapshot failed"
    jq -e '.tasks[0].endpoint.exists == true' "$case_dir/snapshot" >/dev/null || fail "healthy $backend marked absent"
    MX_FAKE_UNREADABLE=1 "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
    jq -e '.tasks[0].endpoint.exists == null and (.tasks[0].endpoint.detail | length > 0)' "$case_dir/snapshot" >/dev/null || fail "unreadable $backend marked absent"
    if [ "$backend" = cmux ]; then
      MX_FAKE_MALFORMED=1 "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
      jq -e '.tasks[0].endpoint.exists == null and (.tasks[0].endpoint.detail | contains("cmux inventory"))' "$case_dir/snapshot" >/dev/null || fail 'malformed cmux inventory marked absent'
    fi
    if [ "$backend" = herdr ]; then
      MX_FAKE_NO_AGENT=1 "$STATE_BIN" "$id" > "$case_dir/out" || fail 'Herdr capture fallback refused'
      MX_FAKE_STOPPED=1 "$STATE_BIN" "$id" > "$case_dir/out" || fail 'stopped Herdr state failed'
      assert_grep 'server_unavailable' "$case_dir/out" 'stopped server detail lost'
      MX_FAKE_STOPPED=1 "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
      jq -e '.tasks[0].endpoint.exists == null and (.tasks[0].endpoint.detail | contains("server_unavailable"))' "$case_dir/snapshot" >/dev/null || fail 'stopped Herdr marked absent'
      if MX_FAKE_STOPPED=1 "$ROOT/bin/mx-doctor.sh" --check stateless-sessions --json > "$case_dir/doctor"; then fail 'stopped server diagnosed healthy'; fi
      assert_grep 'server_unavailable' "$case_dir/doctor" 'doctor lost stopped-server detail'
      mx_write_meta "$state/$id.meta" "window=$endpoint" "backend=$backend" "worktree=$repo" 'kind=daemon'
      MX_FAKE_NO_AGENT=1 "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
      jq -e '.tasks[0].endpoint.exists == true and .tasks[0].endpoint.agent_alive == "dead"' "$case_dir/snapshot" >/dev/null || fail 'agent-less Herdr pane misclassified'
      "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
      jq -e '.tasks[0].endpoint.agent_alive == "alive"' "$case_dir/snapshot" >/dev/null || fail 'live Herdr daemon misclassified'
      for failure in transport api malformed unknown; do
        MX_FAKE_AGENT_FAILURE=$failure "$STATE_BIN" "$id" > "$case_dir/out" || fail "Herdr $failure actor-state refused"
        grep -q 'state: unknown.*native state.*fixture' "$case_dir/out" || fail "Herdr $failure current-state diagnostic lost"
        MX_FAKE_AGENT_FAILURE=$failure "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
        jq -e '.tasks[0].endpoint | .exists == true and .agent_alive == "unknown" and (.detail | contains("fixture"))' "$case_dir/snapshot" >/dev/null || fail "Herdr $failure agent failure lost endpoint presence or diagnostic"
        jq -e '.tasks[0].current_state | .state == "unknown" and (.detail | contains("fixture"))' "$case_dir/snapshot" >/dev/null || fail "Herdr $failure snapshot current-state diagnostic lost"
        printf 'paused: release window\n' > "$state/$id.status"
        MX_FAKE_AGENT_FAILURE=$failure "$STATE_BIN" "$id" > "$case_dir/out"
        assert_grep 'state: paused · source: status-log · release window' "$case_dir/out" 'native failure hid validated report'
        rm "$state/$id.status"
      done
      mx_write_meta "$state/$id.meta" "window=$endpoint" "backend=$backend" "worktree=$repo" 'kind=delivery'
      write_run "$state" "$id" "$repo" "$head" running review
      MX_FAKE_AGENT_FAILURE=transport "$STATE_BIN" "$id" > "$case_dir/out"
      assert_grep 'state: working · source: run-step · validating' "$case_dir/out" 'native failure hid attributed run'
      rm "$state/$id.gate/run.json"
      mx_write_meta "$state/$id.meta" "window=$endpoint" "backend=$backend" "worktree=$repo" 'kind=scout'
      MX_FAKE_AGENT_FAILURE=transport MX_FAKE_PANE_TEXT='Working... esc to interrupt' "$STATE_BIN" "$id" > "$case_dir/out"
      assert_grep 'state: working · source: pane · harness busy' "$case_dir/out" 'native failure hid pane fallback'
      if grep -q '^server\|^status' "$MX_FAKE_HERDR_LOG"; then fail 'passive read attempted server readiness'; fi
    else
      mkdir -p "$case_dir/elsewhere"
      (
        cd "$case_dir/elsewhere" || exit 1
        env -u MX_ROOT_OVERRIDE "$ROOT/bin/mx-doctor.sh" --check stateless-sessions --json > "$case_dir/doctor" || fail 'cmux doctor depends on caller directory'
        env -u MX_ROOT_OVERRIDE "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot" || fail 'cmux snapshot from another directory failed'
        jq -e '.tasks[0].endpoint.exists == true and .tasks[0].current_state.state == "working"' "$case_dir/snapshot" >/dev/null || fail 'cmux snapshot observations disagree outside source directory'
      ) || fail 'cmux wrapper root resolution failed'
      MX_FAKE_SURFACE=s2 "$STATE_BIN" "$id" > "$case_dir/out" || fail 'replacement surface state refused'
      assert_grep 'state: working' "$case_dir/out" 'replacement surface capture lost'
      MX_FAKE_SURFACE=s2 "$ROOT/bin/mx-doctor.sh" --check stateless-sessions --json > "$case_dir/doctor" || fail 'replacement surface diagnosed missing'
      MX_FAKE_SURFACE=s2 "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
      jq -e '.tasks[0].endpoint.exists == true' "$case_dir/snapshot" >/dev/null || fail 'replacement surface marked absent'
    fi
    MX_FAKE_MISSING=1 "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
    jq -e '.tasks[0].endpoint.exists == false' "$case_dir/snapshot" >/dev/null || fail "absent $backend not distinguished"
  done
  mx_write_meta "$state/$id.meta" 'window=x' 'backend=unsupported' "worktree=$repo" 'kind=scout'
  "$ROOT/bin/mx-system-snapshot.sh" --json > "$case_dir/snapshot"
  jq -e '.tasks[0].current_state.detail | contains("unknown backend")' "$case_dir/snapshot" >/dev/null || fail 'snapshot discarded actor-state failure'
  pass 'recorded Herdr/cmux actor-state, doctor and snapshot paths preserve live, absent and unreadable observations'
)
test_recorded_backend_projection
