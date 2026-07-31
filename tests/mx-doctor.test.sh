#!/usr/bin/env bash
# Behavior tests for the read-only invariant sweep and closed repair whitelist.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DOCTOR="$ROOT/bin/mx-doctor.sh"
mx_test_tmproot_into TMP_ROOT mx-doctor

REAL_NODE=$(command -v node)
REAL_JQ=$(command -v jq)
REAL_GIT=$(command -v git)
REAL_GH=$(command -v gh)

make_fakebin() {
  local dir=$1 fakebin
  fakebin=$(mx_fakebin "$dir")
  ln -s "$REAL_NODE" "$fakebin/node"
  ln -s "$REAL_JQ" "$fakebin/jq"
  ln -s "$REAL_GIT" "$fakebin/git"
  ln -s "$REAL_GH" "$fakebin/gh"
  cat >"$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
case "${1:-}" in
  display-message)
    case " ${MX_DOCTOR_FAKE_LIVE_TARGETS:-} " in
      *" ${4:-} "*) printf '%s\n' '%1'; exit 0 ;;
      *) exit 1 ;;
    esac
    ;;
esac
exit 0
SH
  cat >"$fakebin/treehouse" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = get ] && [ "${2:-}" = --help ]; then
  printf '%s\n' 'Usage: treehouse get [--lease] [--lease-holder <holder>]'
  exit 0
fi
if [ "${1:-}" = status ]; then
  [ -z "${MX_DOCTOR_FAKE_TREEHOUSE_STATUS:-}" ] \
    || cat "$MX_DOCTOR_FAKE_TREEHOUSE_STATUS"
  exit 0
fi
exit 0
SH
  cat >"$fakebin/lsof" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *-iTCP:4870-4909*) exit 1 ;;
esac
case "${MX_DOCTOR_LSOF_MODE:-none}" in
  none) exit 1 ;;
  holder) printf '%s\n' 'COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME'; exit 0 ;;
  error) printf '%s\n' 'lsof: injected uncertainty' >&2; exit 2 ;;
esac
exit 2
SH
  chmod +x "$fakebin/tmux" "$fakebin/treehouse" "$fakebin/lsof"
  printf '%s\n' "$fakebin"
}

make_case() {
  local name=$1 dir="$TMP_ROOT/$1" home="$TMP_ROOT/$1/home" fakebin
  mkdir -p "$home/state" "$home/data" "$home/config" "$home/projects" "$dir/root"
  : >"$dir/treehouse.status"
  fakebin=$(make_fakebin "$dir")
  printf '%s|%s|%s|%s\n' "$dir" "$home" "$dir/root" "$fakebin"
}

read_case() {
  IFS='|' read -r CASE_DIR HOME_DIR ROOT_DIR FAKEBIN_DIR <<EOF
$1
EOF
  STATUS_FILE="$CASE_DIR/treehouse.status"
}

run_doctor() {
  MX_HOME="$HOME_DIR" \
  MX_ROOT_OVERRIDE="$ROOT_DIR" \
  MX_STATE_OVERRIDE="$HOME_DIR/state" \
  MX_DATA_OVERRIDE="$HOME_DIR/data" \
  MX_CONFIG_OVERRIDE="$HOME_DIR/config" \
  MX_PROJECTS_OVERRIDE="$HOME_DIR/projects" \
  MX_DOCTOR_COMPAT_PATHS="${MX_DOCTOR_TEST_COMPAT_PATHS:-}" \
  MX_DOCTOR_TREEHOUSE_STATUS_FILE="$STATUS_FILE" \
  MX_DOCTOR_LOCK_STALE_SECS=0 \
  PATH="$FAKEBIN_DIR:/usr/bin:/bin" \
    "$DOCTOR" "$@"
}

run_doctor_capture() {
  local output_path=$1 status_name=$2 result
  shift 2
  set +e
  run_doctor "$@" >"$output_path" 2>&1
  result=$?
  printf -v "$status_name" '%s' "$result"
  set -e
}

portable_mtime() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %m "$1"
  else
    stat -c %Y "$1"
  fi
}

tree_snapshot() {
  local root=$1 path kind payload
  find "$root" -print | LC_ALL=C sort | while IFS= read -r path; do
    if [ -L "$path" ]; then
      kind=link
      payload=$(readlink "$path")
    elif [ -f "$path" ]; then
      kind=file
      payload=$(shasum -a 256 "$path" | awk '{print $1}')
    elif [ -d "$path" ]; then
      kind=dir
      payload=-
    else
      kind=other
      payload=-
    fi
    printf '%s\t%s\t%s\t%s\n' \
      "${path#"$root"}" "$kind" "$(portable_mtime "$path")" "$payload"
  done
}

write_meta() {
  local id=$1 worktree=$2 window=${3:-broker:mx-$1}
  mx_write_meta "$HOME_DIR/state/$id.meta" \
    "window=$window" \
    "worktree=$worktree" \
    "project=$ROOT_DIR" \
    "harness=codex" \
    "kind=delivery"
}

make_stale_watcher_lock() {
  mkdir -p "$HOME_DIR/state/.watch.lock"
  printf '%s\n' 99999999 >"$HOME_DIR/state/.watch.lock/pid"
  printf '%s\n' dead-identity >"$HOME_DIR/state/.watch.lock/pid-identity"
  printf '%s\n' "$HOME_DIR" >"$HOME_DIR/state/.watch.lock/mx-home"
  printf '%s\n' "$DOCTOR" >"$HOME_DIR/state/.watch.lock/watcher-path"
  touch -t 202001010000 "$HOME_DIR/state/.watch.lock" \
    "$HOME_DIR/state/.watch.lock/pid" \
    "$HOME_DIR/state/.watch.lock/pid-identity"
}

assert_check() {
  local name=$1 expected_code=$2 expected_text=$3 output status
  output="$CASE_DIR/$name.out"
  run_doctor_capture "$output" status --check "$name"
  expect_code "$expected_code" "$status" "$name severity"
  assert_grep "$expected_text" "$output" "$name output mismatch"
}

test_clean_fixture_and_exit_zero() {
  read_case "$(make_case clean)"
  local output="$CASE_DIR/doctor.out" status
  run_doctor_capture "$output" status
  expect_code 0 "$status" "clean doctor sweep"
  assert_grep 'summary: 14 OK · 0 WARN · 0 FAIL          exit 0' "$output" \
    "clean sweep summary mismatch"
  pass "clean fixture reports every check OK and exits zero"
}

test_each_check_classifies_its_fixture() {
  local wt now run_dir link

  read_case "$(make_case watcher-lock)"
  make_stale_watcher_lock
  assert_check watcher-lock 2 'FAIL  watcher-lock'

  read_case "$(make_case watcher-beacon)"
  wt="$CASE_DIR/wt"
  mkdir -p "$wt"
  write_meta beacon "$wt"
  assert_check watcher-beacon 1 'WARN  watcher-beacon'

  read_case "$(make_case orphan-worktrees)"
  wt="$CASE_DIR/orphan-wt"
  mkdir -p "$wt"
  printf '1    leased      %s (held by ghost)\n' "$wt" >"$STATUS_FILE"
  assert_check orphan-worktrees 2 'active treehouse path'

  read_case "$(make_case dangling-pids)"
  wt="$CASE_DIR/wt"
  mkdir -p "$wt"
  write_meta deadpid "$wt"
  printf '%s\n' 'pid=99999999' 'pid_identity=dead-identity' \
    >>"$HOME_DIR/state/deadpid.meta"
  assert_check dangling-pids 2 'records dead pid'

  read_case "$(make_case stateless-sessions)"
  wt="$CASE_DIR/wt"
  mkdir -p "$wt"
  write_meta nostate "$wt"
  assert_check stateless-sessions 2 'has no live tmux endpoint'

  read_case "$(make_case wake-queue-orphans)"
  printf '1\t1\tsignal\tghost.status\tworking: still here\n' \
    >"$HOME_DIR/state/.wake-queue"
  assert_check wake-queue-orphans 2 'reference absent task metadata'

  read_case "$(make_case open-holds)"
  {
    printf '## In flight\n\n'
    printf '## Queued\n\n'
    printf '%s\n' '- [ ] ghost-decision-release - Choose release (repo: broker) (kind: maintainer) (hold: choose release) (hold-kind: maintainer)'
    printf '%s\n\n' '  Origin: ghost' '  Decision key: release' '  State: awaiting maintainer decision.'
    printf '## Done\n'
  } >"$HOME_DIR/data/backlog.md"
  assert_check open-holds 2 'hold origin ghost has no task metadata'

  read_case "$(make_case dispatch-queue-age)"
  mkdir -p "$HOME_DIR/state/.dispatch-queue"
  {
    printf '%s\n' 'version=1' 'task_id=old' "project=$ROOT_DIR" \
      'harness=' 'model=' 'effort=' 'backend=tmux' 'kind=delivery' 'enqueued_at=1'
  } >"$HOME_DIR/state/.dispatch-queue/old.request"
  assert_check dispatch-queue-age 1 'exceed 172800s'

  read_case "$(make_case gate-runs)"
  mkdir -p "$HOME_DIR/state/ghost.gate"
  printf '%s\n' '{"version":1,"task":"ghost","status":"running"}' \
    >"$HOME_DIR/state/ghost.gate/run.json"
  assert_check gate-runs 2 'running with no live task endpoint'

  read_case "$(make_case workflow-runs)"
  run_dir="$HOME_DIR/state/sample.workflow"
  mkdir -p "$run_dir/stages"
  printf '%s\n' '{"version":1,"run":"sample","status":"running","current_stage":"build"}' \
    >"$run_dir/run.json"
  assert_check workflow-runs 2 'says running without a live reconcile lock'

  read_case "$(make_case orphan-servers)"
  mkdir -p "$HOME_DIR/state/.vplan"
  {
    printf '%s\n' 'version=1' "artifact=$CASE_DIR/plan.html" 'port=4870' \
      'pid=99999999' 'pid_identity=dead-identity' 'token=test'
  } >"$HOME_DIR/state/.vplan/stale.run"
  assert_check orphan-servers 2 'stale vplan record'

  read_case "$(make_case tools)"
  rm -f "$FAKEBIN_DIR/treehouse"
  assert_check tools 2 'missing treehouse'

  read_case "$(make_case primary-tangle)"
  mx_git_init_commit "$ROOT_DIR"
  git -C "$ROOT_DIR" branch -M main
  git -C "$ROOT_DIR" checkout -qb feature
  assert_check primary-tangle 2 'primary checkout is on feature branch feature'

  read_case "$(make_case compat-symlinks)"
  link="$CASE_DIR/legacy-link"
  ln -s "$CASE_DIR/absent-target" "$link"
  MX_DOCTOR_TEST_COMPAT_PATHS="$link" \
    run_doctor_capture "$CASE_DIR/compat.out" now --check compat-symlinks
  expect_code 1 "$now" "dangling compatibility link"
  assert_grep 'dangling compatibility link' "$CASE_DIR/compat.out" \
    "compatibility check did not report the dangling link"

  pass "every named check classifies a crafted unhealthy fixture"
}

test_default_mode_makes_zero_fixture_mutations() {
  read_case "$(make_case read-only)"
  local wt="$CASE_DIR/missing-wt" before="$CASE_DIR/before" after="$CASE_DIR/after" status
  make_stale_watcher_lock
  write_meta dead "$wt"
  printf '%s\n' 'pid=99999999' 'pid_identity=dead-identity' \
    >>"$HOME_DIR/state/dead.meta"
  printf '1\t1\tsignal\tghost.status\tworking: orphan\n' \
    >"$HOME_DIR/state/.wake-queue"
  tree_snapshot "$HOME_DIR" >"$before"
  run_doctor_capture "$CASE_DIR/doctor.out" status
  expect_code 2 "$status" "dirty read-only sweep"
  tree_snapshot "$HOME_DIR" >"$after"
  cmp -s "$before" "$after" || fail "default doctor mode changed fixture bytes, paths, or mtimes"
  pass "default mode makes zero fixture mutations"
}

test_fix_whitelist_idempotence_and_fail_safe_uncertainty() {
  read_case "$(make_case fixes)"
  local wt="$CASE_DIR/missing-wt" status before_second="$CASE_DIR/before-second" after_second="$CASE_DIR/after-second"
  make_stale_watcher_lock
  write_meta dead "$wt"
  printf '%s\n' 'pid=99999999' 'pid_identity=dead-identity' \
    >>"$HOME_DIR/state/dead.meta"
  printf '1\t1\tsignal\tghost.status\tworking: orphan\n' \
    >"$HOME_DIR/state/.wake-queue"

  run_doctor_capture "$CASE_DIR/fix.out" status --fix
  expect_code 2 "$status" "fix sweep retains non-whitelisted failures"
  assert_absent "$HOME_DIR/state/.watch.lock" "fix did not clear the proven stale watcher lock"
  [ ! -s "$HOME_DIR/state/.wake-queue" ] || fail "fix did not prune the orphan wake row"
  assert_present "$HOME_DIR/state/dead.meta" "fix removed non-whitelisted task metadata"
  assert_grep 'cleared provably stale watcher lock' "$CASE_DIR/fix.out" \
    "fix report omitted watcher-lock remediation"
  assert_grep 'pruned 1 wake queue row' "$CASE_DIR/fix.out" \
    "fix report omitted wake-queue remediation"
  assert_grep 'bin/mx-teardown.sh' "$CASE_DIR/fix.out" \
    "non-whitelisted finding omitted its owner command"

  tree_snapshot "$HOME_DIR" >"$before_second"
  run_doctor_capture "$CASE_DIR/fix-second.out" status --fix
  expect_code 2 "$status" "idempotent fix sweep retains unrelated failure"
  tree_snapshot "$HOME_DIR" >"$after_second"
  cmp -s "$before_second" "$after_second" \
    || fail "second --fix changed an already-repaired fixture"

  read_case "$(make_case uncertainty)"
  make_stale_watcher_lock
  set +e
  MX_DOCTOR_LSOF_MODE=error run_doctor --fix --check watcher-lock \
    >"$CASE_DIR/uncertain.out" 2>&1
  status=$?
  set -e
  expect_code 2 "$status" "uncertain stale-lock proof"
  assert_present "$HOME_DIR/state/.watch.lock" \
    "doctor cleared a watcher lock after lsof uncertainty"
  assert_grep 'staleness cannot be proven safely' "$CASE_DIR/uncertain.out" \
    "uncertain proof did not explain the refusal"
  pass "--fix is closed, idempotent, and fails safe on lsof uncertainty"
}

test_exit_codes_and_json_contract() {
  read_case "$(make_case json-clean)"
  local status json="$CASE_DIR/doctor.json" wt="$CASE_DIR/wt"
  run_doctor_capture "$json" status --json
  expect_code 0 "$status" "clean JSON sweep"
  jq -e '
    .schema == "mx-doctor.v1" and
    .worst_severity == "OK" and .exit_code == 0 and
    .summary == {"ok":14,"warn":0,"fail":0} and
    (.findings | length) == 14
  ' "$json" >/dev/null || fail "clean JSON contract mismatch"

  read_case "$(make_case json-warn)"
  wt="$CASE_DIR/wt"
  mkdir -p "$wt"
  write_meta beacon "$wt"
  run_doctor_capture "$json" status --json --check watcher-beacon
  expect_code 1 "$status" "WARN JSON sweep"
  jq -e '.worst_severity == "WARN" and .exit_code == 1' "$json" >/dev/null \
    || fail "WARN JSON severity does not match exit code"

  read_case "$(make_case json-fail)"
  make_stale_watcher_lock
  run_doctor_capture "$json" status --json --check watcher-lock
  expect_code 2 "$status" "FAIL JSON sweep"
  jq -e '.worst_severity == "FAIL" and .exit_code == 2' "$json" >/dev/null \
    || fail "FAIL JSON severity does not match exit code"
  pass "human and JSON modes share deterministic 0/1/2 severity exits"
}

test_clean_fixture_and_exit_zero
test_each_check_classifies_its_fixture
test_default_mode_makes_zero_fixture_mutations
test_fix_whitelist_idempotence_and_fail_safe_uncertainty
test_exit_codes_and_json_contract

echo "ALL TESTS PASSED"
