#!/usr/bin/env bash
# Behavior tests for mx-bootstrap.sh reporting and session-start clone refresh bounds.
#
# Bootstrap prints one block or line per actionable problem, optional verbose
# BOOTSTRAP_INFO fact, or completed bootstrap no-action fact and is silent when
# all is well. broker consumes the exact 'MISSING: treehouse (install: ...)',
# 'HEADROOM_INVALID: ...', and 'BOOTSTRAP_INFO: ...' lines, so those contracts
# are pinned verbatim. The cases are table-driven over whether the
# universally-required `treehouse get --help` advertises --lease.
# Dedicated system-sync cases pin the computed bootstrap timeout, explicit
# override, blank-env defaulting, partial-output relay, and pre-launch timeout
# scan.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

BASE_PATH=${MX_TEST_BASE_PATH:-/usr/bin:/bin:/usr/sbin:/sbin}
TMP_ROOT=$(mx_test_tmproot mx-bootstrap-tests)
export MX_BACKEND_CMUX_BUNDLE_BIN="$TMP_ROOT/no-bundled-cmux"

# Hermetic runtime-backend detection. These cases pin the backend per-home via
# config/backend; the dev shell's ambient runtime markers ($TMUX inside tmux,
# HERDR_ENV inside herdr, CMUX_* inside a cmux terminal) must not leak into
# mx_backend_name and flip a default-backend case onto a non-tmux backend. Unset
# them once so the suite resolves the tmux reference backend unless a case says
# otherwise - the same hermeticity discipline as pinning PATH via BASE_PATH.
unset TMUX TMUX_PANE HERDR_ENV HERDR_PANE_ID HERDR_SESSION HERDR_SOCKET_PATH \
  CMUX_WORKSPACE_ID CMUX_SURFACE_ID CMUX_SOCKET_PATH CMUX_TAB_ID CMUX_PANEL_ID 2>/dev/null || true

# A fake toolchain where every required tool is present.
# treehouse's `get --help` advertises --lease only when MX_FAKE_TREEHOUSE_LEASE_HELP=1.
make_fake_toolchain() {
  local dir=$1 fakebin
  fakebin=$(mx_fakebin "$dir")
  mx_fake_exit0 "$fakebin" tmux node
  cat > "$fakebin/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = auth ] && [ "${2:-}" = status ]; then
  exit 0
fi
exit 0
SH
  chmod +x "$fakebin/gh"
  cat > "$fakebin/treehouse" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = get ] && [ "${2:-}" = --help ]; then
  if [ "${MX_FAKE_TREEHOUSE_LEASE_HELP:-}" = 1 ]; then
    printf '%s\n' 'Usage: treehouse get [--lease] [--lease-holder <holder>]'
  else
    printf '%s\n' 'Usage: treehouse get'
  fi
  exit 0
fi
exit 0
SH
  chmod +x "$fakebin/treehouse"
  printf '%s\n' "$fakebin"
}

add_real_jq() {
  local fakebin=$1 real_jq
  real_jq=$(command -v jq 2>/dev/null) || fail "jq is required for dispatch profile validation tests"
  cat > "$fakebin/jq" <<SH
#!/usr/bin/env bash
exec '$real_jq' "\$@"
SH
  chmod +x "$fakebin/jq"
}

make_fake_system_sync_root() {
  local dir=$1 fake_root
  fake_root="$dir/fake-root"
  mkdir -p "$fake_root/bin"
  cat > "$fake_root/bin/mx-system-sync.sh" <<'SH'
#!/usr/bin/env bash
[ -z "${MX_FAKE_SYSTEM_SYNC_STARTED_MARKER:-}" ] || : > "$MX_FAKE_SYSTEM_SYNC_STARTED_MARKER"
printf '%s\n' 'alpha: synced'
printf '%s\n' 'beta: skipped: no origin remote'
exec perl -e 'sleep 300'
SH
  chmod +x "$fake_root/bin/mx-system-sync.sh"
  printf '%s\n' "$fake_root"
}

add_origin_backed_projects() {
  local home=$1 count=$2 i repo
  mkdir -p "$home/projects"
  i=1
  while [ "$i" -le "$count" ]; do
    repo=$(printf '%s/projects/repo-%02d' "$home" "$i")
    git init -q "$repo"
    git -C "$repo" remote add origin "file://$home/remotes/repo-$i.git"
    i=$((i + 1))
  done
}

add_no_origin_projects() {
  local home=$1 count=$2 i repo
  mkdir -p "$home/projects"
  i=1
  while [ "$i" -le "$count" ]; do
    repo=$(printf '%s/projects/local-%02d' "$home" "$i")
    git init -q "$repo"
    i=$((i + 1))
  done
}

run_bootstrap_timeout_case() {
  local home=$1 fake_root=$2 fakebin=$3 override started_marker git_record wait_for_marker
  override=__unset__
  started_marker=${5:-}
  git_record=${6:-}
  wait_for_marker=${7:-0}
  [ "$#" -lt 4 ] || override=$4
  (
    # shellcheck disable=SC2317,SC2329 # Exported and invoked by the bootstrap subprocess.
    sleep() {
      local inc=${1:-1}
      SECONDS=$((SECONDS + inc))
      # Advance fake time quickly, but yield on every tick so the background
      # system-sync process can deterministically write its partial output before
      # the simulated timeout kills it, even on a busy full-suite runner.
      command sleep 0.01
    }
    # shellcheck disable=SC2317,SC2329 # Exported and invoked by the bootstrap subprocess.
    git() {
      local tries
      if [ "${MX_FAKE_GIT_WAIT_FOR_SYSTEM_START:-}" = 1 ] && [ -n "${MX_FAKE_SYSTEM_SYNC_STARTED_MARKER:-}" ]; then
        tries=0
        while [ "$tries" -lt 5 ] && [ ! -e "$MX_FAKE_SYSTEM_SYNC_STARTED_MARKER" ]; do
          command sleep 0.01
          tries=$((tries + 1))
        done
      fi
      if [ -n "${MX_FAKE_GIT_SYNC_STARTED_RECORD:-}" ] && [ -n "${MX_FAKE_SYSTEM_SYNC_STARTED_MARKER:-}" ] && [ -e "$MX_FAKE_SYSTEM_SYNC_STARTED_MARKER" ]; then
        printf '%s\n' "$*" >> "$MX_FAKE_GIT_SYNC_STARTED_RECORD"
      fi
      command git "$@"
    }
    export -f sleep
    export -f git
    if [ "$override" = __unset__ ]; then
      PATH="$fakebin:$BASE_PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$fake_root" \
        MX_FAKE_SYSTEM_SYNC_STARTED_MARKER="$started_marker" \
        MX_FAKE_GIT_SYNC_STARTED_RECORD="$git_record" \
        MX_FAKE_GIT_WAIT_FOR_SYSTEM_START="$wait_for_marker" \
        MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh" 2>/dev/null
    else
      PATH="$fakebin:$BASE_PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$fake_root" \
        MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT="$override" \
        MX_FAKE_SYSTEM_SYNC_STARTED_MARKER="$started_marker" \
        MX_FAKE_GIT_SYNC_STARTED_RECORD="$git_record" \
        MX_FAKE_GIT_WAIT_FOR_SYSTEM_START="$wait_for_marker" \
        MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh" 2>/dev/null
    fi
  )
}

assert_timeout_report() {
  local out=$1 expected_timeout=$2 timing timeout elapsed
  timing=$(printf '%s\n' "$out" | sed -n 's/^SYSTEM_SYNC: system: skipped: bootstrap refresh timed out (timeout=\([0-9][0-9]*\)s elapsed=\([0-9][0-9]*\)s)$/\1 \2/p')
  [ -n "$timing" ] || fail "missing system-sync timeout report"
  timeout=${timing%% *}
  elapsed=${timing#* }
  [ "$timeout" -eq "$expected_timeout" ] || fail "expected timeout=${expected_timeout}s, got timeout=${timeout}s"
  [ "$elapsed" -ge "$timeout" ] || fail "expected elapsed >= timeout, got elapsed=${elapsed}s timeout=${timeout}s"
}

# Each row (fields are '^'-separated; the install URL contains a literal '|'):
#   <label>^<lease 1/0>^<mode>^<expect>^<notcontains>
#   mode=empty -> output must be empty (expect/notcontains ignored)
#   mode=exact -> output must equal <expect>
#   mode=grep  -> output must contain <expect> (fixed string); <notcontains> must not appear
test_bootstrap_reporting() {
  local label lease mode expect notcontains case_dir fakebin out n
  n=0
  while IFS='^' read -r label lease mode expect notcontains; do
    [ -n "$label" ] || continue
    n=$((n + 1))
    case_dir="$TMP_ROOT/case-$n"
    mkdir -p "$case_dir/home"
    fakebin=$(make_fake_toolchain "$case_dir")
    # MX_ROOT_OVERRIDE points the worktree-tangle check at the non-git home dir so
    # it stays inert: this suite pins tool detection, not the tangle guard, and the
    # ambient checkout (CI runs on a feature branch) must not leak a TANGLE line in.
    out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
      MX_FAKE_TREEHOUSE_LEASE_HELP="$lease" "$ROOT/bin/mx-bootstrap.sh")
    case "$mode" in
      empty)
        [ -z "$out" ] || fail "$label: expected silence, got: $out" ;;
      exact)
        [ "$out" = "$expect" ] || fail "$label: expected '$expect', got: $out" ;;
      grep)
        printf '%s\n' "$out" | grep -Fx "$expect" >/dev/null || fail "$label: missing '$expect' (got: $out)"
        if [ -n "$notcontains" ]; then
          printf '%s\n' "$out" | grep -F "$notcontains" >/dev/null && fail "$label: unexpected '$notcontains' in: $out"
        fi
        ;;
    esac
  done <<'ROWS'
treehouse --lease support is accepted silently^1^empty^^
treehouse without --lease reports an upgrade^0^grep^MISSING: treehouse (install: curl -fsSL https://kunchenguid.github.io/treehouse/install.sh | sh)^
ROWS
  pass "bootstrap reports treehouse lease and owned headroom contracts"
}

test_git_is_required_with_supported_install_instruction() {
  local case_dir fakebin bash_env out expected
  case_dir="$TMP_ROOT/git-required"
  mkdir -p "$case_dir/home/config"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  fakebin=$(make_fake_toolchain "$case_dir")
  bash_env="$case_dir/no-git.bash"
  cat > "$bash_env" <<'SH'
command() {
  if [ "${1:-}" = -v ] && [ "${2:-}" = git ]; then
    return 1
  fi
  builtin command "$@"
}
git() {
  return 127
}
SH

  out=$(PATH="$fakebin:$BASE_PATH" BASH_ENV="$bash_env" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
  expected="MISSING: git (install: brew install git  # or the platform's package manager)"
  [ "$out" = "$expected" ] || fail "missing git should report the supported install instruction, got: $out"
  pass "bootstrap requires git with an install instruction"
}

# Build a fake toolchain with tmux REMOVED and the named backend session CLI(s)
# plus jq added, so a backend that must NOT require tmux can be proven silent
# with tmux absent. Echoes the fakebin dir. The removed tmux is what makes these
# cases catch the old "everything demands tmux" bug: with the buggy
# TOOLS list a herdr/cmux home would report MISSING: tmux here.
make_fake_toolchain_no_tmux() {  # <case-dir> <extra-cli...>
  local dir=$1 fakebin
  shift
  fakebin=$(make_fake_toolchain "$dir")
  rm -f "$fakebin/tmux"
  mx_fake_exit0 "$fakebin" jq "$@"
  printf '%s\n' "$fakebin"
}

test_session_provider_backends_do_not_require_tmux() {
  local backend cli case_dir fakebin out
  # herdr/cmux are session providers only: they require their own CLI and jq,
  # while universal treehouse provides their worktrees. With all genuine deps
  # present and tmux absent, bootstrap must be silent.
  while IFS='^' read -r backend cli; do
    [ -n "$backend" ] || continue
    case_dir="$TMP_ROOT/$backend-no-tmux"
    mkdir -p "$case_dir/home/config"
    printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
    printf '%s\n' "$backend" > "$case_dir/home/config/backend"
    fakebin=$(make_fake_toolchain_no_tmux "$case_dir" "$cli")
    out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
      MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
    [ -z "$out" ] || fail "backend=$backend with tmux absent but its own deps present should be silent, got: $out"
  done <<'ROWS'
herdr^herdr
cmux^cmux
ROWS
  pass "bootstrap: session-provider backends require their own CLI + jq and universal treehouse, never tmux"
}

test_session_provider_backends_gate_own_cli_not_tmux() {
  local backend cli case_dir fakebin out missing
  # With the backend's OWN session CLI absent (and tmux also absent), bootstrap
  # must fail closed on the genuine dep and never substitute a false tmux demand.
  while IFS='^' read -r backend cli; do
    [ -n "$backend" ] || continue
    case_dir="$TMP_ROOT/$backend-missing-cli"
    mkdir -p "$case_dir/home/config"
    printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
    printf '%s\n' "$backend" > "$case_dir/home/config/backend"
    # Toolchain has jq + treehouse but NOT the session CLI and NOT tmux.
    fakebin=$(make_fake_toolchain_no_tmux "$case_dir")
    out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
      MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
    if [ "$backend" = herdr ]; then
      missing="MISSING_MANUAL: herdr (instructions: https://herdr.dev)"
    else
      missing="MISSING: $cli"
    fi
    assert_contains "$out" "$missing" "backend=$backend must fail closed on its own missing session CLI"
    if [ "$backend" = herdr ]; then
      assert_not_contains "$out" "MISSING: herdr (install:" \
        "backend=herdr must not advertise manual guidance as an executable install command"
    fi
    assert_not_contains "$out" "MISSING: tmux" "backend=$backend must not demand tmux when its own CLI is missing"
  done <<'ROWS'
herdr^herdr
cmux^cmux
ROWS
  pass "bootstrap: a session-provider backend gates its own CLI, never a false tmux requirement"
}

test_herdr_install_requires_manual_action() {
  local out status
  out=$("$ROOT/bin/mx-bootstrap.sh" install herdr 2>&1)
  status=$?
  [ "$status" -ne 0 ] || fail "install herdr should fail instead of evaluating its manual-install hint"
  [ "$out" = "error: herdr requires manual installation (instructions: https://herdr.dev)" ] \
    || fail "install herdr should return actionable manual-install guidance, got: $out"
  pass "bootstrap: Herdr manual-install guidance is never executed as a shell command"
}

test_cmux_bundled_cli_satisfies_dependency() {
  local case_dir fakebin bundle out
  case_dir="$TMP_ROOT/cmux-bundled-cli"
  mkdir -p "$case_dir/home/config" "$case_dir/bundle"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  printf '%s\n' cmux > "$case_dir/home/config/backend"
  fakebin=$(make_fake_toolchain_no_tmux "$case_dir")
  mx_fake_exit0 "$case_dir/bundle" cmux
  bundle="$case_dir/bundle/cmux"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_BACKEND_CMUX_BUNDLE_BIN="$bundle" MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
  [ -z "$out" ] || fail "a usable bundled cmux CLI should satisfy bootstrap without a PATH shim, got: $out"
  pass "bootstrap: the bundled cmux CLI satisfies the active backend dependency"
}

test_unknown_backend_reports_invalid_configuration() {
  local case_dir fakebin out
  case_dir="$TMP_ROOT/unknown-backend"
  mkdir -p "$case_dir/home/config"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  printf '%s\n' bogus > "$case_dir/home/config/backend"
  fakebin=$(make_fake_toolchain "$case_dir")
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
  assert_contains "$out" "BACKEND_INVALID: bogus (known: tmux herdr cmux)" \
    "bootstrap should report an unknown resolved backend"
  assert_not_contains "$out" "MISSING: tmux" "an unknown backend should not silently fall back to tmux dependencies"
  pass "bootstrap: unknown resolved backends fail closed with an actionable diagnostic"
}

test_json_backends_require_jq_not_tmux() {
  local backend case_dir fakebin bash_env out
  # herdr/cmux parse their backend's JSON output, so jq is a genuine dep.
  # jq lives in a system BASE_PATH dir on many hosts, so force it missing with a
  # command()/jq() override (the same technique the git-required case uses) to keep
  # the assertion host-independent.
  while IFS='^' read -r backend; do
    [ -n "$backend" ] || continue
    case_dir="$TMP_ROOT/$backend-missing-jq"
    mkdir -p "$case_dir/home/config"
    printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
    printf '%s\n' "$backend" > "$case_dir/home/config/backend"
    # Session CLI present, tmux absent, jq deliberately NOT stubbed and masked below.
    fakebin=$(make_fake_toolchain "$case_dir")
    rm -f "$fakebin/tmux"
    mx_fake_exit0 "$fakebin" "$backend"
    bash_env="$case_dir/no-jq.bash"
    cat > "$bash_env" <<'SH'
command() {
  if [ "${1:-}" = -v ] && [ "${2:-}" = jq ]; then
    return 1
  fi
  builtin command "$@"
}
jq() {
  return 127
}
SH
    out=$(PATH="$fakebin:$BASE_PATH" BASH_ENV="$bash_env" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
      MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
    assert_contains "$out" "MISSING: jq" "backend=$backend must fail closed on missing jq"
    assert_not_contains "$out" "MISSING: tmux" "backend=$backend must not demand tmux when jq is missing"
  done <<'ROWS'
herdr
cmux
ROWS
  pass "bootstrap: JSON-emitting backends require jq (their genuine dep), never tmux"
}

test_treehouse_requirement_is_unconditional() {
  local case_dir fakebin out missing count
  missing='MISSING: treehouse (install: curl -fsSL https://kunchenguid.github.io/treehouse/install.sh | sh)'

  # An invalid backend has no verified dependency delta. It must not suppress
  # the universal treehouse lease-capability check.
  case_dir="$TMP_ROOT/invalid-backend-old-treehouse"
  mkdir -p "$case_dir/home/config"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  printf '%s\n' bogus > "$case_dir/home/config/backend"
  fakebin=$(make_fake_toolchain "$case_dir")
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    "$ROOT/bin/mx-bootstrap.sh")
  assert_contains "$out" "BACKEND_INVALID: bogus (known: tmux herdr cmux)" \
    "invalid backend setup must remain actionable"
  assert_contains "$out" "$missing" \
    "invalid backend setup must not suppress the treehouse durable-lease check"
  count=$(printf '%s\n' "$out" | grep -Fxc "$missing")
  [ "$count" -eq 1 ] || fail "old treehouse should produce exactly one missing diagnostic, got $count"

  # The command-presence probe is universal for the same reason.
  case_dir="$TMP_ROOT/invalid-backend-missing-treehouse"
  mkdir -p "$case_dir/home/config"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  printf '%s\n' bogus > "$case_dir/home/config/backend"
  fakebin=$(make_fake_toolchain "$case_dir")
  rm -f "$fakebin/treehouse"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
  assert_contains "$out" "$missing" \
    "invalid backend setup must not suppress the treehouse command probe"
  count=$(printf '%s\n' "$out" | grep -Fxc "$missing")
  [ "$count" -eq 1 ] || fail "missing treehouse should produce exactly one missing diagnostic, got $count"

  pass "bootstrap: treehouse presence and durable-lease support are unconditional requirements"
}

test_system_sync_timeout_scales_with_origin_backed_project_count() {
  local case_dir home fakebin fake_root out
  case_dir="$TMP_ROOT/system-timeout-scaled"
  home="$case_dir/home"
  mkdir -p "$home/config"
  printf '%s\n' manual > "$home/config/backlog-backend"
  add_origin_backed_projects "$home" 18
  add_no_origin_projects "$home" 3
  fakebin=$(make_fake_toolchain "$case_dir")
  fake_root=$(make_fake_system_sync_root "$case_dir")

  out=$(run_bootstrap_timeout_case "$home" "$fake_root" "$fakebin")

  assert_contains "$out" $'SYSTEM_SYNC: alpha: synced\nSYSTEM_SYNC: beta: skipped: no origin remote' "bootstrap timeout should relay partial system-sync output first"
  assert_timeout_report "$out" 59
  pass "bootstrap computes a system-size-aware default timeout and preserves partial system-sync output"
}

test_system_sync_timeout_floor_preserves_small_systems() {
  local case_dir home fakebin fake_root out
  case_dir="$TMP_ROOT/system-timeout-small"
  home="$case_dir/home"
  mkdir -p "$home/config"
  printf '%s\n' manual > "$home/config/backlog-backend"
  add_origin_backed_projects "$home" 2
  fakebin=$(make_fake_toolchain "$case_dir")
  fake_root=$(make_fake_system_sync_root "$case_dir")

  out=$(run_bootstrap_timeout_case "$home" "$fake_root" "$fakebin")

  assert_timeout_report "$out" 20
  pass "bootstrap keeps the quick 20s default for small systems"
}

test_system_sync_timeout_explicit_override_wins() {
  local case_dir home fakebin fake_root out
  case_dir="$TMP_ROOT/system-timeout-override"
  home="$case_dir/home"
  mkdir -p "$home/config"
  printf '%s\n' manual > "$home/config/backlog-backend"
  add_origin_backed_projects "$home" 18
  fakebin=$(make_fake_toolchain "$case_dir")
  fake_root=$(make_fake_system_sync_root "$case_dir")

  out=$(run_bootstrap_timeout_case "$home" "$fake_root" "$fakebin" 7)

  assert_timeout_report "$out" 7
  assert_not_contains "$out" "timeout=59s" "explicit override should not be replaced by the computed timeout"
  pass "bootstrap preserves MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT as an explicit override"
}

test_system_sync_timeout_empty_override_uses_default() {
  local case_dir home fakebin fake_root out
  case_dir="$TMP_ROOT/system-timeout-empty-override"
  home="$case_dir/home"
  mkdir -p "$home/config"
  printf '%s\n' manual > "$home/config/backlog-backend"
  add_origin_backed_projects "$home" 18
  fakebin=$(make_fake_toolchain "$case_dir")
  fake_root=$(make_fake_system_sync_root "$case_dir")

  out=$(run_bootstrap_timeout_case "$home" "$fake_root" "$fakebin" "")

  assert_timeout_report "$out" 59
  assert_not_contains "$out" "timeout=20s" "blank timeout env should not force the legacy floor on a large system"
  pass "bootstrap treats a blank timeout override as unset"
}

test_system_sync_timeout_is_computed_before_launch() {
  local case_dir home fakebin fake_root out started_marker git_record
  case_dir="$TMP_ROOT/system-timeout-launch-order"
  home="$case_dir/home"
  started_marker="$case_dir/system-started"
  git_record="$case_dir/git-after-start"
  mkdir -p "$home/config"
  printf '%s\n' manual > "$home/config/backlog-backend"
  add_origin_backed_projects "$home" 3
  fakebin=$(make_fake_toolchain "$case_dir")
  fake_root=$(make_fake_system_sync_root "$case_dir")

  out=$(run_bootstrap_timeout_case "$home" "$fake_root" "$fakebin" __unset__ "$started_marker" "$git_record" 1)

  [ ! -s "$git_record" ] || fail "system sync launched before timeout scan finished: $(tr '\n' ';' < "$git_record")"
  assert_contains "$out" $'SYSTEM_SYNC: alpha: synced\nSYSTEM_SYNC: beta: skipped: no origin remote' "launch-order case should relay partial system-sync output before reporting its timeout"
  assert_timeout_report "$out" 20
  pass "bootstrap computes the timeout before launching system sync"
}

make_routine_bootstrap_fixture() {
  local case_dir=$1 fakebin root home sm c1
  root="$case_dir/root"
  home="$case_dir/home"
  sm="$case_dir/sm"
  mx_git_identity
  mkdir -p "$home/config" "$home/state"
  printf '%s\n' codex > "$home/config/actor-harness"
  printf '%s\n' '{"rules":[{"when":"normal work","use":{"harness":"codex"}}],"default":{"harness":"claude","effort":"low"}}' \
    > "$home/config/actor-dispatch.json"
  git init -q -b main "$root"
  {
    printf '%s\n' '.mx-daemon-home'
    printf '%s\n' 'config/actor-harness'
    printf '%s\n' 'config/actor-dispatch.json'
  } > "$root/.gitignore"
  printf '%s\n' 'instructions' > "$root/AGENTS.md"
  mkdir -p "$root/bin" "$root/.agents/skills"
  printf '%s\n' 'echo ok' > "$root/bin/mx-spawn.sh"
  printf '%s\n' 'skill' > "$root/.agents/skills/example.md"
  git -C "$root" add -A
  git -C "$root" commit -qm initial
  c1=$(git -C "$root" rev-parse HEAD)
  git -C "$root" worktree add -q --detach "$sm" "$c1"
  printf '%s\n' sm > "$sm/.mx-daemon-home"
  {
    printf 'window=broker:mx-sm\n'
    printf 'kind=daemon\n'
    printf 'harness=codex\n'
    printf 'home=%s\n' "$sm"
  } > "$home/state/sm.meta"
  fakebin=$(make_fake_toolchain "$case_dir")
  add_real_jq "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
case "${1:-}" in
  display-message) printf '%s\n' codex ;;
  list-windows) printf '%s\n' mx-sm ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  printf '%s|%s|%s\n' "$root" "$home" "$fakebin"
}

run_routine_bootstrap_fixture() {
  local shell=$1 case_dir=$2 fixture root home fakebin
  fixture=$(make_routine_bootstrap_fixture "$case_dir")
  root=${fixture%%|*}
  fixture=${fixture#*|}
  home=${fixture%%|*}
  fakebin=${fixture#*|}
  PATH="$fakebin:$BASE_PATH" MX_BACKEND=tmux MX_HOME="$home" MX_ROOT_OVERRIDE="$root" \
    MX_FAKE_TREEHOUSE_LEASE_HELP=1 \
    "$shell" "$ROOT/bin/mx-bootstrap.sh"
}

test_routine_bootstrap_confirmations_are_silent() {
  local out
  out=$(run_routine_bootstrap_fixture bash "$TMP_ROOT/routine-silent")
  [ -z "$out" ] || fail "routine bootstrap confirmations should be silent, got: $out"
  pass "bootstrap keeps routine backlog, harness, dispatch, and already-live liveness confirmations silent"
}

test_routine_bootstrap_contract_runs_under_system_bash() {
  local out
  [ -x /bin/bash ] || { pass "bootstrap routine contract skipped without /bin/bash"; return; }
  out=$(run_routine_bootstrap_fixture /bin/bash "$TMP_ROOT/routine-bash")
  [ -z "$out" ] || fail "routine bootstrap contract should be silent under /bin/bash, got: $out"
  pass "bootstrap routine contract runs under system /bin/bash"
}

test_bootstrap_info_is_no_load_and_actionable_lines_trigger() {
  local trigger
  # shellcheck disable=SC2016 # The backtick-delimited skill names are literal Markdown.
  trigger=$(sed -n '/- `bootstrap-diagnostics`/,/- `diagnostic-reasoning`/p' "$ROOT/example_agents.md")
  assert_contains "$trigger" "actionable diagnostic line" "bootstrap-diagnostics trigger should be action-scoped"
  assert_contains "$trigger" "BOOTSTRAP_INFO:" "bootstrap-diagnostics trigger should classify BOOTSTRAP_INFO as no-load"
  assert_contains "$trigger" "HEADROOM_INVALID" "invalid owned headroom must trigger diagnostics loading"
  assert_contains "$trigger" "VPLAN_INVALID" "invalid bundled vplan must trigger diagnostics loading"
  assert_not_contains "$trigger" "ACTOR_HARNESS_OVERRIDE:" "harness override confirmation must not trigger diagnostics loading"
  assert_not_contains "$trigger" "ACTOR_DISPATCH: active" "active dispatch confirmation must not trigger diagnostics loading"
  assert_not_contains "$trigger" "already-live" "already-live daemon liveness must not trigger diagnostics loading"
  pass "bootstrap diagnostics trigger excludes benign lines and keeps actionable prefixes"
}

test_vplan_self_check_failure_is_actionable() {
  local case_dir fakebin broken out expected
  case_dir="$TMP_ROOT/vplan-invalid"
  mkdir -p "$case_dir/home/config"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  fakebin=$(make_fake_toolchain "$case_dir")
  broken="$case_dir/broken-vplan"
  cat > "$broken" <<'SH'
#!/usr/bin/env bash
exit 1
SH
  chmod +x "$broken"
  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_VPLAN_SELF_CHECK_OVERRIDE="$broken" MX_FAKE_TREEHOUSE_LEASE_HELP=1 \
    "$ROOT/bin/mx-bootstrap.sh")
  expected="VPLAN_INVALID: bundled mx-vplan.sh self-check failed"
  [ "$out" = "$expected" ] || fail "broken vplan self-check should report '$expected', got: $out"
  pass "bootstrap reports bundled vplan self-check failures"
}

test_actor_dispatch_active_rules_are_verbose_bootstrap_info() {
  local case_dir fakebin out expect
  case_dir="$TMP_ROOT/dispatch-active"
  mkdir -p "$case_dir/home/config"
  printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
  printf '%s\n' '{"rules":[{"when":"fresh news","use":{"harness":"codex"},"why":"current context"},{"when":"big feature","use":[{"harness":"claude","model":"claude-sonnet-5","effort":"high"},{"harness":"codex","model":"gpt-5.5","effort":"high"}]},{"when":"legacy feature","use":[{"harness":"claude"},{"harness":"codex"}],"select":"quota-balanced"}],"default":[{"harness":"pi","model":"anthropic/claude-sonnet-5","effort":"high"},{"harness":"codex","model":"gpt-5.5","effort":"high"}]}' > "$case_dir/home/config/actor-dispatch.json"
  fakebin=$(make_fake_toolchain "$case_dir")
  add_real_jq "$fakebin"

  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
  [ -z "$out" ] || fail "active dispatch profile should be silent by default, got: $out"

  out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
    MX_BOOTSTRAP_VERBOSE_FACTS=1 MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")

  expect=$'BOOTSTRAP_INFO: vplan self-check passed\nBOOTSTRAP_INFO: headroom self-check passed\nBOOTSTRAP_INFO: actor dispatch active config/actor-dispatch.json\nBOOTSTRAP_INFO: actor dispatch rule: fresh news -> codex\nBOOTSTRAP_INFO: actor dispatch rule: big feature -> quota-balanced[claude/claude-sonnet-5/high, codex/gpt-5.5/high]\nBOOTSTRAP_INFO: actor dispatch rule: legacy feature -> quota-balanced[claude, codex]\nBOOTSTRAP_INFO: actor dispatch default: quota-balanced[pi/anthropic/claude-sonnet-5/high, codex/gpt-5.5/high]'
  [ "$out" = "$expect" ] || fail "active dispatch verbose info block mismatch"$'\n'"expected: $expect"$'\n'"actual:   $out"
  pass "bootstrap surfaces active actor-dispatch rules only as verbose BOOTSTRAP_INFO"
}

test_actor_dispatch_validation() {
  local label body expect mode case_dir fakebin out n
  n=0
  while IFS='^' read -r label body mode expect; do
    [ -n "$label" ] || continue
    n=$((n + 1))
    case_dir="$TMP_ROOT/dispatch-$n"
    mkdir -p "$case_dir/home/config"
    printf '%s\n' manual > "$case_dir/home/config/backlog-backend"
    printf '%s\n' "$body" > "$case_dir/home/config/actor-dispatch.json"
    fakebin=$(make_fake_toolchain "$case_dir")
    add_real_jq "$fakebin"
    out=$(PATH="$fakebin:$BASE_PATH" MX_HOME="$case_dir/home" MX_ROOT_OVERRIDE="$case_dir/home" \
      MX_FAKE_TREEHOUSE_LEASE_HELP=1 "$ROOT/bin/mx-bootstrap.sh")
    case "$mode" in
      empty)
        [ -z "$out" ] || fail "$label: expected silence, got: $out" ;;
      exact)
        [ "$out" = "$expect" ] || fail "$label: expected '$expect', got: $out" ;;
      grep)
        printf '%s\n' "$out" | grep -Fx "$expect" >/dev/null || fail "$label: missing '$expect' (got: $out)" ;;
    esac
  done <<'ROWS'
malformed dispatch config is flagged^{"rules":[^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - malformed JSON
unverified dispatch harness is flagged^{"rules":[{"when":"anything","use":{"harness":"spaceship"}}],"default":{"harness":"codex"}}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - unverified harness: spaceship
unsupported codex max effort is flagged^{"rules":[{"when":"big feature","use":{"harness":"codex","model":"gpt-5","effort":"max"}}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - invalid effort: codex:max
pi max effort is accepted^{"rules":[{"when":"deep coding","use":{"harness":"pi","model":"openai-codex/gpt-5.6-sol","effort":"max"}}]}^empty^
array use with quota-balanced is accepted^{"rules":[{"when":"big feature","use":[{"harness":"claude","model":"claude-sonnet-5","effort":"high"},{"harness":"codex","model":"gpt-5.5","effort":"high"}],"select":"quota-balanced"}]}^empty^
array use without select is accepted^{"rules":[{"when":"big feature","use":[{"harness":"claude"},{"harness":"codex"}]}]}^empty^
one-element array use is accepted^{"rules":[{"when":"focused feature","use":[{"harness":"claude"}]}]}^empty^
default array is accepted^{"default":[{"harness":"pi","model":"anthropic/claude-sonnet-5"},{"harness":"codex"}]}^empty^
one-element default array is accepted^{"default":[{"harness":"codex"}]}^empty^
empty array use is flagged^{"rules":[{"when":"big feature","use":[]}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - each rule needs at least one use profile
array profile without harness is flagged^{"rules":[{"when":"big feature","use":[{"model":"gpt-5.5"}]}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - each use profile needs harness
array profile with malformed model is flagged^{"rules":[{"when":"big feature","use":[{"harness":"codex","model":5}]}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - use profile model and effort must be non-empty strings when present
unknown select is flagged^{"rules":[{"when":"big feature","use":[{"harness":"claude"},{"harness":"codex"}],"select":"mystery"}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - unknown select: mystery
array profile unsupported effort is flagged^{"rules":[{"when":"big feature","use":[{"harness":"codex","effort":"max"}]}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - invalid effort: codex:max
empty default array is flagged^{"default":[]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - default needs at least one profile
non-object default array entry is flagged^{"default":["codex"]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - each default profile must be an object
default array profile without harness is flagged^{"default":[{"model":"gpt-5.5"}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - each default profile needs harness
default array malformed effort is flagged^{"default":[{"harness":"codex","effort":3}]}^exact^ACTOR_DISPATCH: invalid config/actor-dispatch.json - default profile model and effort must be non-empty strings when present
ROWS
  pass "bootstrap validates actor-dispatch.json and reports malformed or unverified configs"
}

test_bootstrap_reporting
test_git_is_required_with_supported_install_instruction
test_session_provider_backends_do_not_require_tmux
test_session_provider_backends_gate_own_cli_not_tmux
test_herdr_install_requires_manual_action
test_cmux_bundled_cli_satisfies_dependency
test_unknown_backend_reports_invalid_configuration
test_json_backends_require_jq_not_tmux
test_treehouse_requirement_is_unconditional
test_system_sync_timeout_scales_with_origin_backed_project_count
test_system_sync_timeout_floor_preserves_small_systems
test_system_sync_timeout_explicit_override_wins
test_system_sync_timeout_empty_override_uses_default
test_system_sync_timeout_is_computed_before_launch
test_routine_bootstrap_confirmations_are_silent
test_routine_bootstrap_contract_runs_under_system_bash
test_bootstrap_info_is_no_load_and_actionable_lines_trigger
test_vplan_self_check_failure_is_actionable
test_actor_dispatch_active_rules_are_verbose_bootstrap_info
test_actor_dispatch_validation
