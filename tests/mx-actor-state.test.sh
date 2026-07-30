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
