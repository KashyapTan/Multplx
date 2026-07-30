#!/usr/bin/env bash
# tests/lib.sh - shared primitives for broker behavior tests.
#
# Source this from a test file:
#   # shellcheck source=tests/lib.sh
#   . "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
#
# It provides the boilerplate every test file used to re-roll: ok/not-ok
# reporters, a self-cleaning temp root, fakebin/PATH-shim helpers, deterministic
# git identity and fixture builders, state/<id>.meta writers, and the common
# string/exit-code/file assertions. It deliberately does NOT bundle the
# behavior-specific fake tmux/treehouse mocks: those encode terminal
# and lifecycle assumptions that differ per suite and belong with the tests that
# own them.
#
# ROOT is exported as the Multplx repo root (this file lives in tests/), so a
# sourcing test can use "$ROOT/bin/..." without recomputing it.

# Idempotent guard: behavior-area helper files (daemon-helpers.sh,
# wake-helpers.sh) source this library for ROOT/fail/pass, and the test that
# includes them may also source it directly. Re-sourcing must not wipe the
# registered-cleanup array or reset state.
if [ -n "${MX_TEST_LIB_SOURCED:-}" ]; then
  return 0
fi
MX_TEST_LIB_SOURCED=1

# Exempt broker's own test suite from the gate-lifecycle refusal
# (bin/mx-gate-refuse-lib.sh). Focused gate tests run FROM a gate worktree, so
# without this every
# test that drives the real mx-spawn/mx-send/mx-teardown would be refused during
# broker's own validation. A confused gate agent never sources this helper, so
# the boundary against the real hazard is unaffected. tests/mx-gate-refuse.test.sh
# strips this to verify real refusal.
export MX_GATE_REFUSE_BYPASS=1

# Start behavior tests from neutral agent ambience.
# Dedicated boundary cases set these markers explicitly; ambient desktop
# harness markers must not alter otherwise unrelated command fixtures.
unset CLAUDECODE CODEX_THREAD_ID PI_CODING_AGENT DEEP_REVIEW_GATE

# Existing behavior suites test the lower-level spawn lifecycle in isolation.
# Plan-07 headroom and queue suites unset this and own capacity enforcement.
export MX_HEADROOM_SKIP_QUEUE=${MX_HEADROOM_SKIP_QUEUE:-1}

# Resolve the repo root from this library's own location. Consumed by sourcing
# test files, not by this library, so it reads as "unused" here.
# shellcheck disable=SC2034
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# --- reporters --------------------------------------------------------------

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

pass() {
  printf 'ok - %s\n' "$1"
}

# --- self-cleaning temp root ------------------------------------------------
#
# mx_test_tmproot <prefix> echoes a fresh temp dir and registers it for removal
# on EXIT. The first call installs the cleanup trap. A test file that needs
# extra teardown (e.g. killing a daemon) should define its own EXIT trap and
# call mx_test_cleanup from inside it so registered dirs are still removed.

MX_TEST_CLEANUP_DIRS=()

mx_test_cleanup() {
  local d
  for d in "${MX_TEST_CLEANUP_DIRS[@]:-}"; do
    [ -n "$d" ] || continue
    chmod -R u+w "$d" 2>/dev/null || true
    rm -rf "$d"
  done
}

mx_test_tmproot() {
  local prefix=${1:-mx-test} root
  root=$(mktemp -d "${TMPDIR:-/tmp}/${prefix}.XXXXXX")
  # Command substitution runs this function in a subshell, where registering
  # parent cleanup is impossible and an EXIT trap would delete the directory
  # before the caller can use the echoed path. The runner-owned TMPDIR still
  # contains and removes those legacy call sites.
  if [ "${BASH_SUBSHELL:-0}" -eq 0 ]; then
    if [ "${#MX_TEST_CLEANUP_DIRS[@]}" -eq 0 ]; then
      trap mx_test_cleanup EXIT
    fi
    MX_TEST_CLEANUP_DIRS+=("$root")
  fi
  printf '%s\n' "$root"
}

# mx_test_tmproot_into <variable> [prefix]: assign and register a temp root in
# the current shell so standalone focused runs receive the same cleanup as the
# resource scheduler.
mx_test_tmproot_into() {
  local variable=$1 prefix=${2:-mx-test} root
  root=$(mktemp -d "${TMPDIR:-/tmp}/${prefix}.XXXXXX")
  if [ "${#MX_TEST_CLEANUP_DIRS[@]}" -eq 0 ]; then
    trap mx_test_cleanup EXIT
  fi
  MX_TEST_CLEANUP_DIRS+=("$root")
  printf -v "$variable" '%s' "$root"
}

# --- timing, bounded waits, and leak diagnostics -----------------------------

mx_test_now_ms() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import time; print(int(time.monotonic() * 1000))'
  else
    echo $(($(date +%s) * 1000))
  fi
}

# mx_test_timed_case <label> <command> [args...]
# Emits opt-in, machine-readable case markers without changing the command's
# output, assertion labels, or exit status.
mx_test_timed_case() {
  local label=$1
  shift
  local started finished duration status had_errexit=0
  started=$(mx_test_now_ms)
  printf 'MX_TEST_CASE_BEGIN label=%s\n' "$label"
  case $- in *e*) had_errexit=1 ;; esac
  set +e
  "$@"
  status=$?
  [ "$had_errexit" -eq 0 ] || set -e
  finished=$(mx_test_now_ms)
  duration=$((finished - started))
  [ "$duration" -ge 0 ] || duration=0
  printf 'MX_TEST_CASE_END label=%s exit=%s duration_ms=%s\n' "$label" "$status" "$duration"
  return "$status"
}

# mx_test_wait_until <timeout-ms> <description> <command> [args...]
# Polls the observable condition with a short adaptive interval and emits the
# last command output on timeout. The command succeeds when the condition is
# true. This helper does not alter any production timeout.
mx_test_wait_until() {
  local timeout_ms=$1 description=$2
  shift 2
  local started now elapsed attempt=0 delay=0.01 diagnostic
  started=$(mx_test_now_ms)
  diagnostic=$(mktemp "${TMPDIR:-/tmp}/mx-test-wait.XXXXXX")
  while :; do
    if "$@" >"$diagnostic" 2>&1; then
      rm -f "$diagnostic"
      return 0
    fi
    now=$(mx_test_now_ms)
    elapsed=$((now - started))
    if [ "$elapsed" -ge "$timeout_ms" ]; then
      printf 'not ok - timed out after %sms waiting for %s\n' "$timeout_ms" "$description" >&2
      if [ -s "$diagnostic" ]; then
        printf '%s\n' '--- last condition output ---' >&2
        cat "$diagnostic" >&2
      fi
      rm -f "$diagnostic"
      return 1
    fi
    attempt=$((attempt + 1))
    [ "$attempt" -lt 20 ] || delay=0.05
    sleep "$delay"
  done
}

# mx_test_assert_no_processes_for <literal>
# Fails with a diagnostic if a surviving process command contains the private
# test-owned path or token. Call after the suite's normal teardown.
mx_test_assert_no_processes_for() {
  local literal=$1 matches
  MX_TEST_LEAK_NEEDLE=$literal
  export MX_TEST_LEAK_NEEDLE
  matches=$(ps -axo pid=,ppid=,command= 2>/dev/null \
    | awk -v self="$$" '
        index($0, ENVIRON["MX_TEST_LEAK_NEEDLE"]) && $1 != self { print }
      ' || true)
  unset MX_TEST_LEAK_NEEDLE
  [ -z "$matches" ] || fail "test-owned processes leaked for '$literal':"$'\n'"$matches"
}

# --- fakebin / PATH shims ---------------------------------------------------
#
# mx_fakebin <dir> creates <dir>/fakebin and echoes it; prepend it to PATH to
# shadow real tools with stubs. mx_fake_exit0 drops trivial exit-0 stubs for the
# named tools into a fakebin dir.

mx_fakebin() {
  local dir=$1 fakebin="$1/fakebin"
  mkdir -p "$fakebin"
  printf '%s\n' "$fakebin"
}

mx_fake_exit0() {
  local fakebin=$1 tool
  shift
  for tool in "$@"; do
    cat > "$fakebin/$tool" <<'SH'
#!/usr/bin/env bash
exit 0
SH
    chmod +x "$fakebin/$tool"
  done
}

# --- deterministic git identity and fixtures --------------------------------

# mx_git_identity [name] [email]: export a fixed author/committer identity so
# fixture commits never depend on the host git config.
mx_git_identity() {
  export GIT_AUTHOR_NAME=${1:-fmtest} GIT_AUTHOR_EMAIL=${2:-fmtest@example.invalid}
  export GIT_COMMITTER_NAME=$GIT_AUTHOR_NAME GIT_COMMITTER_EMAIL=$GIT_AUTHOR_EMAIL
}

# mx_git_init_commit <dir>: create a git repo at <dir> with a README and one
# commit. Uses an inline identity so it works whether or not mx_git_identity was
# called.
mx_git_init_commit() {
  local dir=$1
  mkdir -p "$dir"
  git -C "$dir" init -q
  printf '# %s\n' "$(basename "$dir")" > "$dir/README.md"
  git -C "$dir" add README.md
  git -C "$dir" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm initial
}

# mx_git_add_origin <repo> <bare>: clone <repo> bare into <bare> and register it
# as <repo>'s origin via a file:// URL (so later clones resolve an absolute path).
mx_git_add_origin() {
  local repo=$1 remote=$2 remote_abs
  git clone --quiet --bare "$repo" "$remote"
  remote_abs=$(cd "$remote" && pwd)
  git -C "$repo" remote add origin "file://$remote_abs"
}

# mx_git_worktree <repo> <worktree> <branch>: init <repo> with one commit, then
# add a worktree on a fresh branch.
mx_git_worktree() {
  local repo=$1 worktree=$2 branch=$3
  mx_git_init_commit "$repo"
  git -C "$repo" worktree add --quiet -b "$branch" "$worktree"
}

# mx_test_make_git_template <dir>: create one immutable, committed repository
# fixture. Cases must clone it through mx_test_clone_git_template and never
# mutate this template in place.
mx_test_make_git_template() {
  local template=$1
  mx_git_init_commit "$template"
  chmod -R a-w "$template"
}

# mx_test_clone_git_template <template> <destination>: make an independent
# no-hardlink clone so indexes, refs, and object mutation remain case-private.
mx_test_clone_git_template() {
  local template=$1 destination=$2
  git clone --quiet --no-hardlinks "$template" "$destination"
  chmod -R u+w "$destination"
}

# mx_test_make_tree_template <dir> <builder> [args...]: build a read-only byte
# template once. mx_test_clone_tree_template gives each case writable copies.
mx_test_make_tree_template() {
  local template=$1
  shift
  [ ! -e "$template" ] || fail "template already exists: $template"
  "$@" "$template"
  chmod -R a-w "$template"
}

mx_test_clone_tree_template() {
  local template=$1 destination=$2
  cp -R "$template" "$destination"
  chmod -R u+w "$destination"
}

# --- state/<id>.meta writers ------------------------------------------------

# mx_write_meta <file> <key=val> ...: write the given key=val lines to a meta
# file (truncating any prior content).
mx_write_meta() {
  local file=$1 kv
  shift
  : > "$file"
  for kv in "$@"; do
    printf '%s\n' "$kv" >> "$file"
  done
}

# mx_write_daemon_meta <file> <home> [window] [projects]: write the standard
# kind=daemon meta block used across the daemon suites. window defaults
# to broker:mx-<basename-of-home-dir's parent id>? No - window is explicit;
# defaults to broker:mx-domain and projects to alpha to match the common case.
mx_write_daemon_meta() {
  local file=$1 home=$2 window=${3:-broker:mx-domain} projects=${4:-alpha}
  mx_write_meta "$file" \
    "window=$window" \
    "worktree=$home" \
    "project=$home" \
    "harness=echo" \
    "kind=daemon" \
    "mode=daemon" \
    "yolo=off" \
    "home=$home" \
    "projects=$projects"
}

# --- common assertions ------------------------------------------------------

# assert_contains <haystack> <needle> <msg>
assert_contains() {
  case "$1" in
    *"$2"*) : ;;
    *) fail "$3 (missing: '$2')"$'\n'"--- output ---"$'\n'"$1" ;;
  esac
}

# assert_not_contains <haystack> <needle> <msg>
assert_not_contains() {
  case "$1" in
    *"$2"*) fail "$3 (unexpected: '$2')"$'\n'"--- output ---"$'\n'"$1" ;;
    *) : ;;
  esac
}

# expect_code <expected> <actual> <label>
expect_code() {
  local expected=$1 actual=$2 label=$3
  [ "$actual" = "$expected" ] || fail "$label: expected exit $expected, got $actual"
}

# assert_grep <pattern> <file> <msg>: fixed-string grep must match in <file>.
# `--` guards patterns that begin with '-' (e.g. backlog/registry lines).
assert_grep() {
  grep -F -- "$1" "$2" >/dev/null || fail "$3"
}

# assert_no_grep <pattern> <file> <msg>: fixed-string grep must NOT match.
assert_no_grep() {
  ! grep -F -- "$1" "$2" >/dev/null || fail "$3"
}

# assert_absent <path> <msg>: path must not exist.
assert_absent() {
  [ ! -e "$1" ] || fail "$2"
}

# assert_present <path> <msg>: path must exist.
assert_present() {
  [ -e "$1" ] || fail "$2"
}
