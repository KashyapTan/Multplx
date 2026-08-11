#!/usr/bin/env bash
# tests/mx-backend.test.sh - P1 runtime-backend extraction conformance
# (data/mx-backend-design-d7/report.md, herdr-addendum.md "events as the core
# abstraction"). bin/mx-backend.sh and bin/backends/tmux.sh move the tmux
# command sequences that mx-send.sh, mx-peek.sh, mx-spawn.sh, and
# mx-teardown.sh used to run inline into named adapter functions. This suite:
#
#   1. Unit-tests bin/mx-backend.sh's selection, meta, and dispatch helpers.
#   2. Runs the PRE-REFACTOR versions of mx-send.sh, mx-peek.sh, mx-spawn.sh,
#      and mx-teardown.sh (checked out from the merge-base with `main`, the
#      commit this branch started from) against the SAME fake tmux/treehouse
#      binaries and fixtures as the REFACTORED versions in this checkout, then
#      diffs the two command logs byte-for-byte - the report's P1 checklist
#      item "run current main scripts and refactored scripts against the same
#      fake tools and compare command logs".
#   3. Asserts the `--backend`/`MX_BACKEND` selection refuses unknown backends
#      and the blocked `codex-app` backend loudly.
#
# mx-watch.sh's signal/stale/check/heartbeat wake-string contract is already
# exercised end-to-end against this refactor by tests/mx-watch-triage.test.sh
# and tests/wake-helpers.sh (same fake-tmux convention, run against the
# now-refactored bin/mx-watch.sh); this suite adds one direct old-vs-new
# diff for the stale-pane path specifically, since that is the one wake path
# that now calls through mx_backend_capture instead of tmux directly.
# The real tmux smoke test (create session, send text + Enter, capture, list,
# kill) lives in tests/mx-backend-tmux-smoke.test.sh.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_git_identity fmtest fmtest@example.invalid

# shellcheck source=/dev/null
. "$ROOT/bin/mx-backend.sh"

TMP_ROOT=$(mx_test_tmproot mx-backend-tests)

# mx_backend_detect's cmux fallback (bundle id + process ancestry,
# docs/cmux-backend.md "Runtime auto-detection") consults uname, lsappinfo,
# and ps. FAKE_NONDARWIN_BIN pins uname to Linux so the whole fallback is
# deterministically inert for every assertion that expects NO detection,
# regardless of the ambient runtime this suite itself executes inside (a real
# cmux tab would otherwise leak a bundle-id or ancestry match into results).
FAKE_NONDARWIN_BIN="$TMP_ROOT/fake-nondarwin-bin"
mkdir -p "$FAKE_NONDARWIN_BIN"
printf '#!/bin/sh\necho Linux\n' > "$FAKE_NONDARWIN_BIN/uname"
chmod +x "$FAKE_NONDARWIN_BIN/uname"

# make_cmux_fallback_fakebin: PATH fakes for the DETECTING side of the cmux
# fallback - uname pinned to Darwin, lsappinfo echoing $MX_FAKE_LSAPPINFO_OUT
# (empty output mirrors the real lsappinfo's app-not-running behavior: prints
# nothing, exit 0), and a ps answering `-o ppid=/-o comm= -p <pid>` from the
# tab-separated "pid ppid comm" table file named by $MX_FAKE_PS_TABLE.
make_cmux_fallback_fakebin() {  # <dir> -> echoes fakebin dir
  local fb="$1/fakebin-cmux-fallback"
  mkdir -p "$fb"
  printf '#!/bin/sh\necho Darwin\n' > "$fb/uname"
  cat > "$fb/lsappinfo" <<'SH'
#!/bin/sh
[ -n "${MX_FAKE_LSAPPINFO_OUT:-}" ] && printf '%s\n' "$MX_FAKE_LSAPPINFO_OUT"
exit 0
SH
  cat > "$fb/ps" <<'SH'
#!/bin/sh
# supports exactly: ps -o ppid= -p <pid> / ps -o comm= -p <pid>
field=${2:-} pid=${4:-}
while IFS="	" read -r tpid tppid tcomm; do
  if [ "$tpid" = "$pid" ]; then
    case "$field" in
      ppid=) printf '%s\n' "$tppid" ;;
      comm=) printf '%s\n' "$tcomm" ;;
    esac
    exit 0
  fi
done < "${MX_FAKE_PS_TABLE:?}"
exit 1
SH
  chmod +x "$fb/uname" "$fb/lsappinfo" "$fb/ps"
  printf '%s\n' "$fb"
}

# The commit this branch started from - the P1 "current main" baseline.
resolve_base_ref() {
  local ref base
  for ref in main refs/heads/main origin/main refs/remotes/origin/main origin/HEAD refs/remotes/origin/HEAD; do
    if git -C "$ROOT" rev-parse --verify -q "$ref^{commit}" >/dev/null; then
      base=$(git -C "$ROOT" merge-base HEAD "$ref" 2>/dev/null) || continue
      [ -n "$base" ] || continue
      printf '%s\n' "$base"
      return 0
    fi
  done
  return 1
}
BASE_REF=$(resolve_base_ref) \
  || fail "mx-backend baseline requires local main or origin/main; fetch the default branch before running this test"

# --- shared: a pre-refactor bin/ shim --------------------------------------
#
# build_old_bin echoes a directory whose bin/ subdir holds the PRE-REFACTOR
# mx-send.sh, mx-peek.sh, mx-watch.sh, mx-spawn.sh, mx-teardown.sh, and any
# changed source-library dependency (all extracted from BASE_REF), plus copies
# of every OTHER sibling script those five entrypoints source, so those copies are exactly
# what BASE_REF would have used too. Copies keep BASH_SOURCE-based sibling
# resolution inside the synthetic tree on both macOS and Linux; symlinks make
# that resolution shell/platform-dependent. MX_ROOT_OVERRIDE pointed at this dir's
# root makes "$MX_ROOT/bin/mx-project-mode.sh" (etc.) resolve correctly.
# mx-backend.sh (and its bin/backends/ adapters) is the dispatcher every one
# of the five REFACTORED scripts sources; it must be a real, reachable file in
# the old bin/ too or `. "$SCRIPT_DIR/mx-backend.sh"` aborts under set -eu -
# hence it is a copied sibling, not an extracted-from-BASE_REF file: for a
# tmux-only conformance run the tmux adapter's behavior is what is under test,
# and that is unchanged by any later (e.g. non-tmux backend) addition to
# mx-backend.sh's own dispatch surface.
OLD_BIN_UNCHANGED_SIBLINGS="mx-gate-refuse-lib.sh mx-guard.sh mx-lock-lib.sh mx-pr-lib.sh mx-tangle-lib.sh mx-tmux-lib.sh mx-composer-lib.sh mx-wake-lib.sh mx-classify-lib.sh mx-supervision-lib.sh mx-ff-lib.sh mx-config-inherit-lib.sh mx-project-mode.sh mx-harness.sh mx-actor-state.sh mx-decision-hold.sh mx-backlog-lib.sh mx-backend.sh mx-operational-input.sh mx-rust-runtime.sh"
# A pull-request merge may add a new main-only dependency that the branch's older baseline does not have yet.
OLD_BIN_OPTIONAL_SIBLINGS="mx-pending-reply-lib.sh mx-maintainer-override-lib.sh"
OLD_BIN_REFACTORED="mx-send.sh mx-peek.sh mx-watch.sh mx-spawn.sh mx-teardown.sh mx-marker-lib.sh"

build_old_bin() {  # <name> -> echoes root dir (root/bin/<script> is the entry point)
  local name=$1 root bin f
  root="$TMP_ROOT/$name"
  bin="$root/bin"
  mkdir -p "$bin"
  for f in $OLD_BIN_UNCHANGED_SIBLINGS; do
    cp "$ROOT/bin/$f" "$bin/$f"
  done
  for f in $OLD_BIN_OPTIONAL_SIBLINGS; do
    [ -f "$ROOT/bin/$f" ] || continue
    cp "$ROOT/bin/$f" "$bin/$f"
  done
  cp -R "$ROOT/bin/backends" "$bin/backends"
  for f in $OLD_BIN_REFACTORED; do
    git -C "$ROOT" show "$BASE_REF:bin/$f" > "$bin/$f"
    chmod +x "$bin/$f"
  done
  # This suite compares backend command logs, not the retired backlog backend.
  # Retarget the historical teardown fixture onto the owned compatibility
  # functions so the baseline can run without reconstructing an external tool.
  local legacy_lib='mx-tasks'"-axi-lib.sh"
  local legacy_function='mx_tasks'"_axi_backend_available"
  sed -e "s/$legacy_lib/mx-backlog-lib.sh/g" \
    -e "s/$legacy_function/mx_backlog_backend_available/g" \
    "$bin/mx-teardown.sh" > "$bin/mx-teardown.sh.next"
  mv "$bin/mx-teardown.sh.next" "$bin/mx-teardown.sh"
  chmod +x "$bin/mx-teardown.sh"
  printf '%s\n' "$root"
}

# --- mx-backend.sh unit tests ------------------------------------------------

test_backend_name_precedence() {
  local dir cfg
  dir="$TMP_ROOT/name-precedence"; cfg="$dir/config"
  mkdir -p "$cfg"

  # TMUX/HERDR_ENV/CMUX_WORKSPACE_ID explicitly unset in a subshell so this
  # stays deterministic regardless of the runtime this test suite itself
  # happens to execute inside (e.g. a real tmux pane, which is the normal case
  # for a maintainer's session).
  # mx_backend_name reads MX_BACKEND_CONFIG_DIR (bound once, at mx-backend.sh
  # source time, from MX_CONFIG_OVERRIDE); a later MX_CONFIG_OVERRIDE=... prefix
  # on the function call itself does not re-bind it, so these calls set
  # MX_BACKEND_CONFIG_DIR directly.
  [ "$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID __CFBundleIdentifier; PATH="$FAKE_NONDARWIN_BIN:$PATH" MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name)" = tmux ] \
    || fail "mx_backend_name should default to tmux with no env/config/detection markers"

  printf 'tmux\n' > "$cfg/backend"
  [ "$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID; MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name)" = tmux ] \
    || fail "mx_backend_name should read config/backend"

  [ "$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID; MX_BACKEND=tmux MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name)" = tmux ] \
    || fail "MX_BACKEND env should win over config/backend"

  pass "mx_backend_name: MX_BACKEND env > config/backend > default tmux"
}

# mx_backend_detect: environment-marker runtime auto-detection (mirrors
# mx-harness.sh's detect_own layer). Every case explicitly controls TMUX,
# HERDR_ENV, and CMUX_WORKSPACE_ID - and, where no detection is expected, the
# cmux fallback inputs (__CFBundleIdentifier plus a non-Darwin uname fake) -
# so results never depend on the ambient shell this suite runs inside (a real
# tmux pane or cmux tab, both normal cases for a maintainer's session).
test_backend_detect_precedence() {
  local out

  if out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID __CFBundleIdentifier; PATH="$FAKE_NONDARWIN_BIN:$PATH" mx_backend_detect); then
    fail "mx_backend_detect should return 1 (undetected) with no markers set, got '$out'"
  fi

  out=$(unset TMUX CMUX_WORKSPACE_ID; HERDR_ENV=1 mx_backend_detect) \
    || fail "mx_backend_detect should succeed when HERDR_ENV=1"
  [ "$out" = herdr ] || fail "mx_backend_detect should report herdr for HERDR_ENV=1 alone, got '$out'"

  out=$(unset HERDR_ENV CMUX_WORKSPACE_ID; TMUX='fake,1,0' mx_backend_detect) \
    || fail "mx_backend_detect should succeed when \$TMUX is set"
  [ "$out" = tmux ] || fail "mx_backend_detect should report tmux for \$TMUX alone, got '$out'"

  out=$(unset TMUX HERDR_ENV; CMUX_WORKSPACE_ID='fake-uuid' mx_backend_detect) \
    || fail "mx_backend_detect should succeed when CMUX_WORKSPACE_ID is set"
  [ "$out" = cmux ] || fail "mx_backend_detect should report cmux for CMUX_WORKSPACE_ID alone, got '$out'"

  # Nesting: tmux started inside a herdr pane carries BOTH markers. Innermost
  # (tmux) must win, since that is the surface broker is actually running on.
  out=$(unset CMUX_WORKSPACE_ID; TMUX='fake,1,0' HERDR_ENV=1 mx_backend_detect) \
    || fail "mx_backend_detect should succeed with both markers present"
  [ "$out" = tmux ] || fail "mx_backend_detect should resolve nesting innermost-first (tmux over herdr), got '$out'"

  # Nesting: tmux started inside a cmux-provided shell carries BOTH markers.
  # cmux is a terminal application, not a nestable multiplexer, so the
  # innermost multiplexer (tmux) must still win.
  out=$(unset HERDR_ENV; TMUX='fake,1,0' CMUX_WORKSPACE_ID='fake-uuid' mx_backend_detect) \
    || fail "mx_backend_detect should succeed with tmux and cmux markers present"
  [ "$out" = tmux ] || fail "mx_backend_detect should resolve nesting innermost-first (tmux over cmux), got '$out'"

  # Nesting: herdr started inside a cmux-provided shell carries BOTH markers.
  # Same reasoning: herdr (the innermost multiplexer) must win over cmux.
  out=$(unset TMUX; HERDR_ENV=1 CMUX_WORKSPACE_ID='fake-uuid' mx_backend_detect) \
    || fail "mx_backend_detect should succeed with herdr and cmux markers present"
  [ "$out" = herdr ] || fail "mx_backend_detect should resolve nesting innermost-first (herdr over cmux), got '$out'"

  # Pathological: all three markers present. tmux still wins (innermost of all).
  out=$(TMUX='fake,1,0' HERDR_ENV=1 CMUX_WORKSPACE_ID='fake-uuid' mx_backend_detect) \
    || fail "mx_backend_detect should succeed with all three markers present"
  [ "$out" = tmux ] || fail "mx_backend_detect should resolve nesting innermost-first with all three markers (tmux wins), got '$out'"

  pass "mx_backend_detect: no markers -> undetected, HERDR_ENV=1 -> herdr, \$TMUX -> tmux, CMUX_WORKSPACE_ID -> cmux, nested combinations resolve innermost-first"
}

# mx_backend_detect's cmux FALLBACK signals (docs/cmux-backend.md "Runtime
# auto-detection"): cmux's bundled claude wrapper strips every CMUX_* env var
# on its passthrough path, so a claude-under-cmux broker has no
# CMUX_WORKSPACE_ID; detection then falls back to __CFBundleIdentifier and,
# after that, a process-ancestry walk - macOS-only, and never outranking the
# $TMUX/HERDR_ENV innermost-first checks.
test_backend_detect_cmux_fallback_bundle_id() {
  local dir fb out
  dir="$TMP_ROOT/detect-fallback-bundle"; mkdir -p "$dir"
  fb=$(make_cmux_fallback_fakebin "$dir")

  out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID; PATH="$fb:$PATH" __CFBundleIdentifier='com.cmuxterm.app' mx_backend_detect) \
    || fail "mx_backend_detect should fall back to the cmux bundle id when CMUX_WORKSPACE_ID is absent"
  [ "$out" = cmux ] || fail "bundle-id fallback should report cmux, got '$out'"

  (
    unset TMUX HERDR_ENV CMUX_WORKSPACE_ID
    PATH="$fb:$PATH" __CFBundleIdentifier='com.cmuxterm.app' mx_backend_detect >/dev/null || exit 1
    [ "$MX_BACKEND_DETECT_SIGNAL" = bundle-id ] || exit 2
  ) || fail "bundle-id fallback should set MX_BACKEND_DETECT_SIGNAL=bundle-id (subshell exit $?)"

  # A foreign bundle id (an ordinary terminal app) must not match.
  if out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID; PATH="$fb:$PATH" MX_FAKE_PS_TABLE="$dir/no-table" __CFBundleIdentifier='com.apple.Terminal' mx_backend_detect); then
    fail "a non-cmux __CFBundleIdentifier should not detect cmux, got '$out'"
  fi

  pass "mx_backend_detect: falls back to __CFBundleIdentifier=com.cmuxterm.app when CMUX_WORKSPACE_ID is absent (signal bundle-id; foreign bundle ids rejected)"
}

test_backend_detect_cmux_fallback_requires_darwin() {
  local out
  if out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID; PATH="$FAKE_NONDARWIN_BIN:$PATH" __CFBundleIdentifier='com.cmuxterm.app' mx_backend_detect); then
    fail "the cmux fallback must be macOS-only (cmux itself is), got '$out' on a non-Darwin uname"
  fi
  pass "mx_backend_detect: the cmux fallback signals are macOS-only (inert on a non-Darwin uname)"
}

# The false positive the innermost-first ordering must keep absorbing: a tmux
# server started from a cmux tab inherits __CFBundleIdentifier=com.cmuxterm.app
# into every pane (verified live, docs/cmux-backend.md), so the bundle-id
# fallback WILL match inside such panes - $TMUX winning first is what keeps
# the result correct. Same for a herdr pane whose server was started from a
# cmux tab.
test_backend_detect_cmux_fallback_tmux_nested_false_positive() {
  local dir fb out
  dir="$TMP_ROOT/detect-fallback-nested"; mkdir -p "$dir"
  fb=$(make_cmux_fallback_fakebin "$dir")

  out=$(unset HERDR_ENV CMUX_WORKSPACE_ID; PATH="$fb:$PATH" TMUX='fake,1,0' __CFBundleIdentifier='com.cmuxterm.app' mx_backend_detect) \
    || fail "mx_backend_detect should still succeed with \$TMUX plus an inherited cmux bundle id"
  [ "$out" = tmux ] || fail "\$TMUX must win over an inherited cmux bundle id (tmux-inside-cmux pane), got '$out'"

  out=$(unset TMUX CMUX_WORKSPACE_ID; PATH="$fb:$PATH" HERDR_ENV=1 __CFBundleIdentifier='com.cmuxterm.app' mx_backend_detect) \
    || fail "mx_backend_detect should still succeed with HERDR_ENV=1 plus an inherited cmux bundle id"
  [ "$out" = herdr ] || fail "HERDR_ENV=1 must win over an inherited cmux bundle id (herdr-inside-cmux pane), got '$out'"

  pass "mx_backend_detect: an inherited cmux bundle id never outranks \$TMUX or HERDR_ENV (tmux/herdr-inside-cmux false positive absorbed)"
}

test_backend_detect_cmux_fallback_ancestry_pid_match() {
  local dir fb table
  dir="$TMP_ROOT/detect-ancestry-pid"; mkdir -p "$dir"
  fb=$(make_cmux_fallback_fakebin "$dir")
  table="$dir/ps-table"
  # $$ is this test script's own pid - the walk starts there. The cmux app
  # pid (66666) is matched via the lsappinfo bundle-id resolution, with a
  # deliberately non-standard install path so only the pid can match.
  printf '%s\t77777\t/bin/zsh\n77777\t66666\t/usr/bin/login\n66666\t1\t/Users/x/Custom.app/Contents/MacOS/custom\n' "$$" > "$table"

  (
    unset TMUX HERDR_ENV CMUX_WORKSPACE_ID __CFBundleIdentifier
    PATH="$fb:$PATH" MX_FAKE_PS_TABLE="$table" MX_FAKE_LSAPPINFO_OUT='"pid"=66666' mx_backend_detect >/dev/null || exit 1
    [ "$MX_BACKEND_DETECTED" = cmux ] || exit 2
    [ "$MX_BACKEND_DETECT_SIGNAL" = ancestry ] || exit 3
  ) || fail "ancestry fallback should detect cmux via the lsappinfo-resolved app pid (subshell exit $?)"

  pass "mx_backend_detect: ancestry fallback matches the lsappinfo-resolved (bundle-id) cmux app pid in the parent chain"
}

test_backend_detect_cmux_fallback_ancestry_comm_match() {
  local dir fb table
  dir="$TMP_ROOT/detect-ancestry-comm"; mkdir -p "$dir"
  fb=$(make_cmux_fallback_fakebin "$dir")
  table="$dir/ps-table"
  # lsappinfo resolves nothing (empty output, like the real one for a
  # non-running or non-GUI-visible app); the bundle-shaped comm path is the
  # remaining match, at a non-/Applications install location on purpose.
  printf '%s\t77777\t/bin/zsh\n77777\t66666\t/usr/bin/login\n66666\t1\t/Users/x/Applications/cmux.app/Contents/MacOS/cmux\n' "$$" > "$table"

  (
    unset TMUX HERDR_ENV CMUX_WORKSPACE_ID __CFBundleIdentifier MX_FAKE_LSAPPINFO_OUT
    PATH="$fb:$PATH" MX_FAKE_PS_TABLE="$table" mx_backend_detect >/dev/null || exit 1
    [ "$MX_BACKEND_DETECTED" = cmux ] || exit 2
    [ "$MX_BACKEND_DETECT_SIGNAL" = ancestry ] || exit 3
  ) || fail "ancestry fallback should detect cmux via a bundle-shaped comm path when lsappinfo resolves nothing (subshell exit $?)"

  pass "mx_backend_detect: ancestry fallback matches a bundle-shaped cmux comm path at any install location when lsappinfo cannot resolve a pid"
}

# From inside tmux, ancestry can never reach cmux: the tmux server reparents
# to launchd (verified live - the reference machine's own tmux server, started
# from a cmux tab, has ppid 1), so the walk stops at ppid 1 undetected. This
# pins the walk's launchd stop as the structural guarantee behind that.
test_backend_detect_cmux_fallback_ancestry_stops_at_launchd() {
  local dir fb table out
  dir="$TMP_ROOT/detect-ancestry-stop"; mkdir -p "$dir"
  fb=$(make_cmux_fallback_fakebin "$dir")
  table="$dir/ps-table"
  printf '%s\t77777\t/bin/zsh\n77777\t1\ttmux\n' "$$" > "$table"

  if out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID __CFBundleIdentifier MX_FAKE_LSAPPINFO_OUT; PATH="$fb:$PATH" MX_FAKE_PS_TABLE="$table" mx_backend_detect); then
    fail "ancestry fallback should stop undetected at a launchd-reparented chain, got '$out'"
  fi
  pass "mx_backend_detect: ancestry fallback stops undetected at launchd (a reparented tmux server never reaches cmux)"
}

# The auto-detect NOTICE must say when cmux was selected via a fallback
# signal, so a maintainer can tell a wrapper-stripped claude-under-cmux spawn
# apart from the primary-marker case.
test_backend_name_cmux_fallback_notice() {
  local dir cfg fb out errfile
  dir="$TMP_ROOT/name-fallback-notice"; cfg="$dir/config-empty"; mkdir -p "$cfg"
  fb=$(make_cmux_fallback_fakebin "$dir")
  errfile="$dir/err.txt"

  : > "$errfile"
  out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID; PATH="$fb:$PATH" __CFBundleIdentifier='com.cmuxterm.app' MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = cmux ] || fail "mx_backend_name should auto-detect cmux via the bundle-id fallback, got '$out'"
  assert_contains "$(cat "$errfile")" "FALLBACK signal __CFBundleIdentifier" \
    "the fallback-detected cmux notice did not name the bundle-id fallback signal"
  assert_contains "$(cat "$errfile")" "EXPERIMENTAL cmux backend" \
    "the fallback-detected cmux notice lost the experimental warning"
  assert_contains "$(cat "$errfile")" "--backend tmux" \
    "the fallback-detected cmux notice lost the opt-out"

  # The primary-marker notice is unchanged: it names CMUX_WORKSPACE_ID and
  # carries no FALLBACK wording.
  : > "$errfile"
  out=$(unset TMUX HERDR_ENV; CMUX_WORKSPACE_ID='fake-uuid' MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = cmux ] || fail "mx_backend_name should auto-detect cmux from CMUX_WORKSPACE_ID, got '$out'"
  assert_contains "$(cat "$errfile")" "(CMUX_WORKSPACE_ID)" \
    "the primary-marker cmux notice no longer names CMUX_WORKSPACE_ID"
  case "$(cat "$errfile")" in
    *FALLBACK*) fail "the primary-marker cmux notice must not carry FALLBACK wording" ;;
  esac

  pass "mx_backend_name: a fallback-detected cmux prints a NOTICE naming the fallback signal; the primary-marker notice is unchanged"
}

# mx_backend_name's auto-detect step: fires only when MX_BACKEND/config/backend
# are both absent, selects between the three markers exactly as
# mx_backend_detect does, and is loud only when it selects herdr or cmux -
# never when it selects tmux (today's default-path behavior must stay
# byte-for-byte silent).
test_backend_name_autodetect_notice() {
  local dir cfg out errfile

  dir="$TMP_ROOT/name-autodetect"; cfg="$dir/config-empty"; mkdir -p "$cfg"
  errfile="$dir/err.txt"

  : > "$errfile"
  out=$(unset TMUX HERDR_ENV CMUX_WORKSPACE_ID __CFBundleIdentifier; PATH="$FAKE_NONDARWIN_BIN:$PATH" MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = tmux ] || fail "mx_backend_name should default to tmux with no detection markers, got '$out'"
  [ -s "$errfile" ] && fail "mx_backend_name must stay silent with no detection markers"$'\n'"$(cat "$errfile")"

  : > "$errfile"
  out=$(unset TMUX CMUX_WORKSPACE_ID; HERDR_ENV=1 MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = herdr ] || fail "mx_backend_name should auto-detect herdr from HERDR_ENV=1, got '$out'"
  assert_contains "$(cat "$errfile")" "EXPERIMENTAL herdr backend" \
    "mx_backend_name did not print a loud notice when auto-detecting herdr"
  assert_contains "$(cat "$errfile")" "config/backend" \
    "mx_backend_name's auto-detect notice did not name the opt-out"

  : > "$errfile"
  out=$(unset HERDR_ENV CMUX_WORKSPACE_ID; TMUX='fake,1,0' MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = tmux ] || fail "mx_backend_name should auto-detect tmux from \$TMUX, got '$out'"
  [ -s "$errfile" ] && fail "auto-detecting tmux must stay silent (today's unchanged default-path behavior)"$'\n'"$(cat "$errfile")"

  : > "$errfile"
  out=$(unset TMUX HERDR_ENV; CMUX_WORKSPACE_ID='fake-uuid' MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = cmux ] || fail "mx_backend_name should auto-detect cmux from CMUX_WORKSPACE_ID, got '$out'"
  assert_contains "$(cat "$errfile")" "EXPERIMENTAL cmux backend" \
    "mx_backend_name did not print a loud notice when auto-detecting cmux"
  assert_contains "$(cat "$errfile")" "config/backend" \
    "mx_backend_name's cmux auto-detect notice did not name the opt-out"
  assert_contains "$(cat "$errfile")" "--backend tmux" \
    "mx_backend_name's cmux auto-detect notice did not name the --backend tmux opt-out"

  : > "$errfile"
  out=$(unset CMUX_WORKSPACE_ID; TMUX='fake,1,0' HERDR_ENV=1 MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = tmux ] || fail "nested tmux-in-herdr should auto-detect tmux (innermost first), got '$out'"
  [ -s "$errfile" ] && fail "nested tmux-in-herdr auto-detect (result tmux) must stay silent"$'\n'"$(cat "$errfile")"

  : > "$errfile"
  out=$(unset HERDR_ENV; TMUX='fake,1,0' CMUX_WORKSPACE_ID='fake-uuid' MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name 2>"$errfile")
  [ "$out" = tmux ] || fail "nested tmux-in-cmux should auto-detect tmux (innermost first), got '$out'"
  [ -s "$errfile" ] && fail "nested tmux-in-cmux auto-detect (result tmux) must stay silent"$'\n'"$(cat "$errfile")"

  pass "mx_backend_name: auto-detect selects herdr or cmux (loud notice) or tmux (silent, including nested tmux-in-herdr/tmux-in-cmux)"
}

# Explicit configuration (MX_BACKEND env or config/backend) always wins over
# runtime auto-detection, even when a detection marker points the other way.
test_backend_name_explicit_beats_detection() {
  local dir cfg out

  dir="$TMP_ROOT/name-explicit-beats-detect"
  cfg="$dir/config-tmux"; mkdir -p "$cfg"; printf 'tmux\n' > "$cfg/backend"
  mkdir -p "$dir/config-empty"

  # mx_backend_name reads MX_BACKEND_CONFIG_DIR (bound once, at mx-backend.sh
  # source time, from MX_CONFIG_OVERRIDE); a later MX_CONFIG_OVERRIDE=... prefix
  # on the function call itself does not re-bind it, so these calls set
  # MX_BACKEND_CONFIG_DIR directly to control which config dir is checked.
  out=$(unset TMUX; HERDR_ENV=1 MX_BACKEND=tmux MX_BACKEND_CONFIG_DIR="$dir/config-empty" mx_backend_name)
  [ "$out" = tmux ] || fail "MX_BACKEND=tmux should win over an ambient HERDR_ENV=1 auto-detect marker, got '$out'"

  out=$(unset TMUX; HERDR_ENV=1 MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name)
  [ "$out" = tmux ] || fail "config/backend=tmux should win over an ambient HERDR_ENV=1 auto-detect marker, got '$out'"

  # The same opt-out must work for an ambient cmux auto-detect marker: a
  # maintainer who is running broker inside a cmux terminal but explicitly
  # wants tmux is never overridden by CMUX_WORKSPACE_ID.
  out=$(unset TMUX HERDR_ENV; CMUX_WORKSPACE_ID='fake-uuid' MX_BACKEND=tmux MX_BACKEND_CONFIG_DIR="$dir/config-empty" mx_backend_name)
  [ "$out" = tmux ] || fail "MX_BACKEND=tmux should win over an ambient CMUX_WORKSPACE_ID auto-detect marker, got '$out'"

  out=$(unset TMUX HERDR_ENV; CMUX_WORKSPACE_ID='fake-uuid' MX_BACKEND='' MX_BACKEND_CONFIG_DIR="$cfg" mx_backend_name)
  [ "$out" = tmux ] || fail "config/backend=tmux should win over an ambient CMUX_WORKSPACE_ID auto-detect marker, got '$out'"

  pass "mx_backend_name: an explicit MX_BACKEND or config/backend setting always wins over runtime auto-detection, including an ambient cmux marker"
}

test_backend_validate_refuses_unknown() {
  mx_backend_validate tmux 2>/dev/null || fail "mx_backend_validate should accept tmux"
  local out
  # bogus names a backend with no adapter at all; tmux, herdr,
  # and cmux are all known adapters and spawn-supported.
  out=$(mx_backend_validate bogus 2>&1) && fail "mx_backend_validate should refuse bogus (no such adapter)"
  assert_contains "$out" "unknown backend 'bogus'" "mx_backend_validate did not name the rejected backend"
  out=$(mx_backend_validate codex-app 2>&1) && fail "mx_backend_validate should refuse codex-app"
  assert_contains "$out" "unknown backend 'codex-app'" "mx_backend_validate accepted codex-app"
  out=$(mx_backend_validate "tmux herdr" 2>&1) && fail "mx_backend_validate should refuse a multi-token backend name"
  assert_contains "$out" "unknown backend 'tmux herdr'" "mx_backend_validate accepted a multi-token backend name"
  pass "mx_backend_validate: implemented adapters accepted, unknown and blocked codex-app backends refused loudly"
}

test_backend_source_shell_portable() {
  local out status
  # zsh does not word-split unquoted expansions; sourcing mx-backend.sh from
  # an interactive zsh session must still recognize known backend names.
  if command -v zsh >/dev/null 2>&1; then
    zsh -c "cd '$ROOT' && source bin/mx-backend.sh && mx_backend_source herdr && whence -w mx_backend_herdr_capture >/dev/null" 2>/dev/null \
      || fail "zsh: mx_backend_source herdr should load the adapter when sourced"
    out=$(zsh -c "cd '$ROOT' && source bin/mx-backend.sh && mx_backend_source bogus" 2>&1) \
      && fail "zsh: mx_backend_source bogus should fail"
    assert_contains "$out" "unknown backend 'bogus'" \
      "zsh: mx_backend_source did not reject bogus with the expected error"
    pass "zsh: mx_backend_source recognizes known backends and rejects unknown ones"
  else
    pass "zsh: shell-portable backend matching skipped (zsh not found)"
  fi

  bash -c "cd '$ROOT' && source bin/mx-backend.sh && mx_backend_source herdr && declare -F mx_backend_herdr_capture >/dev/null" 2>/dev/null \
    || fail "bash: mx_backend_source herdr should load the adapter when sourced"
  out=$(bash -c "cd '$ROOT' && source bin/mx-backend.sh && mx_backend_source bogus" 2>&1) \
    && fail "bash: mx_backend_source bogus should fail"
  assert_contains "$out" "unknown backend 'bogus'" \
    "bash: mx_backend_source did not reject bogus with the expected error"
  pass "bash: mx_backend_source recognizes known backends and rejects unknown ones"
}

test_backend_validate_spawn_accepts_known() {
  local out
  mx_backend_validate_spawn tmux 2>/dev/null || fail "mx_backend_validate_spawn should accept tmux"
  mx_backend_validate_spawn herdr 2>/dev/null || fail "mx_backend_validate_spawn should accept herdr"
  mx_backend_validate_spawn cmux 2>/dev/null || fail "mx_backend_validate_spawn should accept cmux"
  out=$(mx_backend_validate_spawn bogus 2>&1) && fail "mx_backend_validate_spawn should still refuse unknown backends"
  assert_contains "$out" "unknown backend 'bogus'" "mx_backend_validate_spawn did not preserve unknown-backend validation"
  out=$(mx_backend_validate_spawn codex-app 2>&1) && fail "mx_backend_validate_spawn should refuse codex-app"
  assert_contains "$out" "unknown backend 'codex-app'" "mx_backend_validate_spawn accepted codex-app"
  out=$(mx_backend_validate_spawn "tmux herdr" 2>&1) && fail "mx_backend_validate_spawn should refuse a multi-token backend name"
  assert_contains "$out" "unknown backend 'tmux herdr'" "mx_backend_validate_spawn accepted a multi-token backend name"
  pass "mx_backend_validate_spawn: all implemented lifecycle backends are spawn-supported"
}

test_meta_get_and_backend_of_meta() {
  local meta=$TMP_ROOT/meta-get.meta
  mx_write_meta "$meta" "window=broker:mx-x1" "harness=claude"
  [ "$(mx_meta_get "$meta" window)" = "broker:mx-x1" ] || fail "mx_meta_get did not read window="
  [ "$(mx_meta_get "$meta" missing)" = "" ] || fail "mx_meta_get should print nothing for an absent key"
  [ "$(mx_backend_of_meta "$meta")" = tmux ] || fail "mx_backend_of_meta should default absent backend= to tmux"

  printf 'backend=tmux\n' >> "$meta"
  [ "$(mx_backend_of_meta "$meta")" = tmux ] || fail "mx_backend_of_meta should read an explicit backend=tmux"

  pass "mx_meta_get / mx_backend_of_meta: read key=value, default backend to tmux"
}

test_resolve_selector_three_forms() {
  local state=$TMP_ROOT/resolve-state fakebin out
  mkdir -p "$state"
  mx_write_meta "$state/task1.meta" "window=broker:mx-task1"
  mx_write_meta "$state/dotfiles-d6.meta" "window=default:wA:p2" "backend=herdr"
  mx_write_meta "$state/mx-turnend-all-harnesses-v9.meta" "window=default:wB:p3" "backend=herdr"

  [ "$(mx_backend_resolve_selector 'sess:win' "$state")" = "sess:win" ] \
    || fail "explicit session:window should be used as-is"

  [ "$(mx_backend_resolve_selector 'dotfiles-d6' "$state")" = "default:wA:p2" ] \
    || fail "bare non-fm task id should resolve through exact metadata"
  [ "$(mx_backend_of_selector 'dotfiles-d6' 'default:wA:p2' "$state")" = herdr ] \
    || fail "bare non-fm task id should use its recorded backend"
  [ "$(mx_backend_expected_label_of_selector 'dotfiles-d6' "$state")" = "mx-dotfiles-d6" ] \
    || fail "bare non-fm task id should report the spawned mx-<id> label"

  [ "$(mx_backend_resolve_selector 'mx-turnend-all-harnesses-v9' "$state")" = "default:wB:p3" ] \
    || fail "exact mx-* task id should resolve through its exact metadata"
  [ "$(mx_backend_of_selector 'mx-turnend-all-harnesses-v9' 'default:wB:p3' "$state")" = herdr ] \
    || fail "exact mx-* task id should use exact metadata without stripping mx-"
  [ "$(mx_backend_expected_label_of_selector 'mx-turnend-all-harnesses-v9' "$state")" = "mx-mx-turnend-all-harnesses-v9" ] \
    || fail "exact mx-* task id should report the spawned mx-<id> label"

  [ "$(mx_backend_resolve_selector 'mx-task1' "$state")" = "broker:mx-task1" ] \
    || fail "legacy mx-<id> label should resolve through <id>.meta's window="
  [ "$(mx_backend_expected_label_of_selector 'mx-task1' "$state")" = "mx-task1" ] \
    || fail "legacy mx-<id> label should preserve its backend label"

  out=$(mx_backend_resolve_selector 'mx-missing' "$state" 2>&1) && fail "mx-<id> with no meta should fail"
  assert_contains "$out" "no metadata for mx-missing" "missing-meta error text changed"

  fakebin="$TMP_ROOT/resolve-fakebin"; mkdir -p "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
case "${1:-}" in
  list-windows) printf 'broker:adhoc\nother:otherwin\n' ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  out=$(PATH="$fakebin:$PATH" mx_backend_resolve_selector 'mx-adhoc' "$state" 2>&1) || true
  # mx-adhoc carries no meta file, so it is NOT the bare-name fallback path - it
  # is the mx-* meta-miss error path after exact-id and legacy-label metadata
  # lookup both miss.
  # Only a NON mx-* bare name falls through to the live-window search.
  assert_contains "$out" "no metadata for mx-adhoc" "an mx-* selector must always require meta, not silently fall back to a live search"

  out=$(PATH="$fakebin:$PATH" mx_backend_resolve_selector 'adhoc' "$state")
  [ "$out" = "broker:adhoc" ] || fail "an ad hoc bare name should resolve via the tmux live-window fallback, got '$out'"

  pass "mx_backend_resolve_selector: session:window literal, exact task id first, legacy mx-<id> label fallback, ad hoc bare name via tmux list-windows"
}

test_backend_of_selector_matches_explicit_target_meta() {
  local state=$TMP_ROOT/backend-selector-state
  mkdir -p "$state"
  mx_write_meta "$state/herdr-task.meta" "window=default:w1:p2" "backend=herdr"
  mx_write_meta "$state/dotfiles-d6.meta" "window=default:wA:p2" "backend=herdr"
  mx_write_meta "$state/mx-turnend-all-harnesses-v9.meta" "window=default:wB:p3" "backend=herdr"
  mx_write_meta "$state/tmux-task.meta" "window=broker:mx-tmux-task"
  mx_write_meta "$state/custom-window-task.meta" "window=custom-window"

  [ "$(mx_backend_of_selector 'dotfiles-d6' 'default:wA:p2' "$state")" = herdr ] \
    || fail "bare non-fm task id selector should use its recorded backend"
  [ "$(mx_backend_of_selector 'mx-turnend-all-harnesses-v9' 'default:wB:p3' "$state")" = herdr ] \
    || fail "exact mx-* task id selector should use exact metadata before legacy stripping"
  [ "$(mx_backend_of_selector 'mx-herdr-task' 'default:w1:p2' "$state")" = herdr ] \
    || fail "legacy mx-<id> selector should use its recorded backend"
  [ "$(mx_backend_resolve_selector 'custom-window' "$state")" = custom-window ] \
    || fail "raw window selector matching metadata should not require tmux fallback"
  [ "$(mx_backend_of_selector 'default:w1:p2' 'default:w1:p2' "$state")" = herdr ] \
    || fail "explicit backend target matching metadata should use that task's backend"
  [ "$(mx_backend_of_selector 'broker:mx-tmux-task' 'broker:mx-tmux-task' "$state")" = tmux ] \
    || fail "explicit tmux-shaped target with absent backend= should default to tmux"
  [ "$(mx_backend_of_selector 'manual:outside' 'manual:outside' "$state")" = tmux ] \
    || fail "explicit target with no matching metadata should keep the tmux compatibility default"

  pass "mx_backend_of_selector: exact task ids, legacy mx-<id> labels, and matching explicit targets inherit metadata backend"
}

# --- old vs new: mx-send.sh --------------------------------------------------

make_send_fakebin() {  # <dir> -> echoes fakebin dir; logs every tmux call to $MX_TMUX_LOG
  local dir=$1 fb="$1/fakebin"
  mkdir -p "$fb"
  cat > "$fb/tmux" <<'SH'
#!/usr/bin/env bash
set -u
{ printf 'tmux'; for a in "$@"; do printf '\x1f%s' "$a"; done; printf '\n'; } >> "${MX_TMUX_LOG:?}"
case "${1:-}" in
  send-keys) exit 0 ;;
  display-message)
    for a in "$@"; do case "$a" in *cursor_y*) printf '0\n'; exit 0 ;; esac; done
    printf 'fakepane\n'; exit 0 ;;
  capture-pane) printf '\xe2\x94\x82 \xe2\x94\x82\n'; exit 0 ;;
  list-windows) exit 0 ;;
esac
exit 0
SH
  chmod +x "$fb/tmux"
  printf '%s\n' "$fb"
}

run_send_case() {  # <bin-root> <fakebin> <log> <home> -- <send args...>
  local bin=$1 fb=$2 log=$3 home=$4; shift 4
  [ "${1:-}" = -- ] && shift
  : > "$log"
  env PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$bin" MX_HOME="$home" MX_TMUX_LOG="$log" \
    MX_SEND_SETTLE=0 MX_SEND_SLEEP=0 \
    "$bin/bin/mx-send.sh" "$@" >/dev/null 2>&1
}

strip_send_preflight() {  # <log>
  local preflight
  preflight=$'tmux\x1fdisplay-message\x1f-p\x1f-t\x1fsess:win\x1f#{pane_id}'
  awk -v preflight="$preflight" '$0 != preflight { print }' "$1"
}

test_send_conformance_old_vs_new() {
  local old_bin fb log_old log_new home rc_old rc_new filtered_old filtered_new
  old_bin=$(build_old_bin send-old)
  fb=$(make_send_fakebin "$TMP_ROOT/send-fake")
  home="$TMP_ROOT/send-home"; mkdir -p "$home/state"
  log_old="$TMP_ROOT/send-old.log"; log_new="$TMP_ROOT/send-new.log"
  filtered_old="$TMP_ROOT/send-old.filtered.log"; filtered_new="$TMP_ROOT/send-new.filtered.log"

  # Case 1: --key path.
  run_send_case "$old_bin" "$fb" "$log_old" "$home" -- "sess:win" --key Escape
  rc_old=$?
  run_send_case "$ROOT" "$fb" "$log_new" "$home" -- "sess:win" --key Escape
  rc_new=$?
  expect_code "$rc_old" "$rc_new" "mx-send --key: old vs new exit code"
  assert_contains "$(cat "$log_new")" $'\x1f''display-message'$'\x1f''-p'$'\x1f''-t'$'\x1f''sess:win'$'\x1f''#{pane_id}' \
    "mx-send --key did not verify the explicit tmux target before sending"
  strip_send_preflight "$log_old" > "$filtered_old"
  strip_send_preflight "$log_new" > "$filtered_new"
  diff -u "$filtered_old" "$filtered_new" > "$TMP_ROOT/send-diff-key.txt" 2>&1 \
    || fail "mx-send --key: tmux command log differs old vs new"$'\n'"$(cat "$TMP_ROOT/send-diff-key.txt")"
  assert_contains "$(cat "$log_new")" $'\x1f''Escape' "mx-send --key did not send the named key"

  # Case 2: plain text (0.3s settle, no popup).
  run_send_case "$old_bin" "$fb" "$log_old" "$home" -- "sess:win" hello maintainer
  rc_old=$?
  run_send_case "$ROOT" "$fb" "$log_new" "$home" -- "sess:win" hello maintainer
  rc_new=$?
  expect_code "$rc_old" "$rc_new" "mx-send plain text: old vs new exit code"
  strip_send_preflight "$log_old" > "$filtered_old"
  strip_send_preflight "$log_new" > "$filtered_new"
  diff -u "$filtered_old" "$filtered_new" > "$TMP_ROOT/send-diff-plain.txt" 2>&1 \
    || fail "mx-send plain text: tmux command log differs old vs new"$'\n'"$(cat "$TMP_ROOT/send-diff-plain.txt")"
  assert_contains "$(cat "$log_new")" $'\x1f''send-keys'$'\x1f''-t'$'\x1f''sess:win'$'\x1f''-l'$'\x1f''hello maintainer' \
    "mx-send did not send the literal text with send-keys -l"
  assert_contains "$(cat "$log_new")" $'\x1f''Enter' "mx-send did not submit with Enter"

  # Case 3: a slash command still opens the popup-settle path (verified
  # elsewhere in tests/mx-send-popup-settle.test.sh) and still ends in the
  # same tmux command shape: send-keys -l, then a retried Enter.
  run_send_case "$old_bin" "$fb" "$log_old" "$home" -- "sess:win" /some-skill
  rc_old=$?
  run_send_case "$ROOT" "$fb" "$log_new" "$home" -- "sess:win" /some-skill
  rc_new=$?
  expect_code "$rc_old" "$rc_new" "mx-send /skill: old vs new exit code"
  strip_send_preflight "$log_old" > "$filtered_old"
  strip_send_preflight "$log_new" > "$filtered_new"
  diff -u "$filtered_old" "$filtered_new" > "$TMP_ROOT/send-diff-slash.txt" 2>&1 \
    || fail "mx-send /skill: tmux command log differs old vs new"$'\n'"$(cat "$TMP_ROOT/send-diff-slash.txt")"

  pass "mx-send.sh: explicit tmux targets are verified, while --key/plain/slash send command shape stays old-compatible"
}

# --- old vs new: mx-peek.sh --------------------------------------------------

make_peek_fakebin() {  # <dir> <capture-output> -> echoes fakebin dir
  local dir=$1 payload=$2 fb="$1/fakebin"
  mkdir -p "$fb"
  printf '%s' "$payload" > "$dir/capture.out"
  cat > "$fb/tmux" <<SH
#!/usr/bin/env bash
set -u
{ printf 'tmux'; for a in "\$@"; do printf '\\x1f%s' "\$a"; done; printf '\\n'; } >> "\${MX_TMUX_LOG:?}"
case "\${1:-}" in
  capture-pane) cat "$dir/capture.out" ;;
esac
exit 0
SH
  chmod +x "$fb/tmux"
  printf '%s\n' "$fb"
}

test_peek_conformance_old_vs_new() {
  local old_bin fb log_old log_new home out_old out_new payload neutral_root
  payload=$'line one\nline two\nmaintainer on deck'
  old_bin=$(build_old_bin peek-old)
  fb=$(make_peek_fakebin "$TMP_ROOT/peek-fake" "$payload")
  home="$TMP_ROOT/peek-home"; mkdir -p "$home/state"
  log_old="$TMP_ROOT/peek-old.log"; log_new="$TMP_ROOT/peek-new.log"
  # A fresh non-git dir keeps mx-guard.sh's worktree-tangle check inert (it warns
  # to stderr, discarded below) - neither run needs MX_ROOT for anything beyond
  # that guard, since STATE/HOME are already overridden directly.
  neutral_root="$TMP_ROOT/peek-neutral-root"; mkdir -p "$neutral_root"

  : > "$log_old"
  out_old=$(PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$neutral_root" MX_HOME="$home" MX_TMUX_LOG="$log_old" \
    "$old_bin/bin/mx-peek.sh" "sess:win" 25 2>/dev/null)
  : > "$log_new"
  out_new=$(PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$neutral_root" MX_HOME="$home" MX_TMUX_LOG="$log_new" \
    "$ROOT/bin/mx-peek.sh" "sess:win" 25 2>/dev/null)

  [ "$out_old" = "$out_new" ] || fail "mx-peek output differs old vs new"$'\n'"--- old ---"$'\n'"$out_old"$'\n'"--- new ---"$'\n'"$out_new"
  [ "$out_new" = "$payload" ] || fail "mx-peek did not pass through the fake capture-pane output exactly"
  diff -u "$log_old" "$log_new" > "$TMP_ROOT/peek-diff.txt" 2>&1 \
    || fail "mx-peek: tmux command log differs old vs new"$'\n'"$(cat "$TMP_ROOT/peek-diff.txt")"
  assert_contains "$(cat "$log_new")" $'\x1f''capture-pane'$'\x1f''-p'$'\x1f''-t'$'\x1f''sess:win'$'\x1f''-S'$'\x1f''-25' \
    "mx-peek did not call capture-pane -p -t <target> -S -<lines> exactly"

  pass "mx-peek.sh: capture-pane invocation and output are byte-identical old vs new"
}

# --- old vs new: mx-spawn.sh --------------------------------------------------

make_spawn_fakebin() {  # <dir> <fake-worktree-path> -> echoes fakebin dir
  local dir=$1 wt=$2 fb="$1/fakebin"
  mkdir -p "$fb"
  cat > "$fb/tmux" <<SH
#!/usr/bin/env bash
set -u
{ printf 'tmux'; for a in "\$@"; do printf '\\x1f%s' "\$a"; done; printf '\\n'; } >> "\${MX_TMUX_LOG:?}"
case "\${1:-}" in
  display-message)
    for a in "\$@"; do case "\$a" in *pane_current_path*) printf '%s\\n' "$wt"; exit 0 ;; esac; done
    printf 'broker\\n'; exit 0 ;;
  list-windows) exit 0 ;;
esac
exit 0
SH
  chmod +x "$fb/tmux"
  mx_fake_exit0 "$fb" treehouse
  printf '%s\n' "$fb"
}

run_spawn_case() {  # <bin-root> <fakebin> <log> <state> <data> <config> <proj> -- <spawn args...>
  local bin=$1 fb=$2 log=$3 state=$4 data=$5 config=$6 proj=$7; shift 7
  [ "${1:-}" = -- ] && shift
  : > "$log"
  env PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$bin" \
    MX_STATE_OVERRIDE="$state" MX_DATA_OVERRIDE="$data" MX_CONFIG_OVERRIDE="$config" \
    MX_PROJECTS_OVERRIDE="$TMP_ROOT/unused-projects" \
    MX_SPAWN_NO_GUARD=1 TMUX="fake,1,0" MX_TMUX_LOG="$log" \
    "$bin/bin/mx-spawn.sh" "$@"
}

# NOTE: the old-vs-new spawn command-log conformance test that used to live here
# was retired. It asserted the P1 backend refactor was a byte-for-byte pure
# extraction of the spawn window-creation/targeting sequence, but that sequence
# is now DELIBERATELY changed: mx-spawn drives the tmux backend to capture a
# stable window id, pin the window name (automatic-rename/allow-rename off), and
# target that id for the rename-critical spawn steps (robustness under a
# maintainer's non-default tmux config). A byte-identical old-vs-new diff can no
# longer hold there by design. That intended sequence is now authoritatively and
# comprehensively verified - via a recording fake-tmux - by
# tests/mx-tangle-guard.test.sh ("mx-spawn: appends windows by session-colon,
# pins the name, and targets the window id"), and the real tmux create/kill path
# by tests/mx-backend-tmux-smoke.test.sh. The send/peek conformance
# tests below remain pure extractions and stay. (make_spawn_fakebin and
# run_spawn_case are retained: test_spawn_default_backend_writes_no_meta_field
# uses make_spawn_fakebin, and #294's run_spawn_symlink_case uses run_spawn_case.)

# --- symlinked project prefix must not false-refuse the isolation guard -----
#
# docs/herdr-backend.md "Known gaps": a real backend's pane_current_path read
# (tmux, herdr) reports the OS-level PHYSICALLY-resolved cwd. When the project
# itself lives under a symlinked prefix (e.g. macOS's /tmp -> /private/tmp),
# mx-spawn.sh's PROJ_ABS - a logical `cd && pwd` - differs string-for-string
# from that physical read even before treehouse moves the pane at all, so the
# worktree-discovery poll used to mistake an UNMOVED pane for one that had
# already left the project, handing validate_spawn_worktree the project's own
# directory as "the worktree" and tripping its false isolation refusal.
# make_spawn_symlink_fakebin's tmux stub returns an unmoved project path on the
# first pane_current_path poll, then the real worktree path from the second poll
# onward, so this test fails loudly if the PROJ_ABS/PROJ_ABS_REAL
# canonicalization in bin/mx-spawn.sh ever regresses.
make_spawn_symlink_fakebin() {  # <dir> <initial-project-path> <worktree-path> -> echoes fakebin dir
  local dir=$1 initial_path=$2 wt=$3 fb="$1/fakebin" counter="$1/poll-count"
  mkdir -p "$fb"
  : > "$counter"
  cat > "$fb/tmux" <<SH
#!/usr/bin/env bash
set -u
{ printf 'tmux'; for a in "\$@"; do printf '\\x1f%s' "\$a"; done; printf '\\n'; } >> "\${MX_TMUX_LOG:?}"
case "\${1:-}" in
  display-message)
    for a in "\$@"; do case "\$a" in *pane_current_path*)
      printf x >> "$counter"
      if [ "\$(wc -c < "$counter")" -le 1 ]; then
        printf '%s\\n' "$initial_path"
      else
        printf '%s\\n' "$wt"
      fi
      exit 0
    ;; esac; done
    printf 'broker\\n'; exit 0 ;;
  list-windows) exit 0 ;;
esac
exit 0
SH
  chmod +x "$fb/tmux"
  mx_fake_exit0 "$fb" treehouse
  printf '%s\n' "$fb"
}

run_spawn_symlink_case() {  # <label> <physical|logical>
  local label=$1 first_reply=$2 real_root link_root proj wt id fb data state config log out rc proj_phys initial_path
  real_root="$TMP_ROOT/symlink-real-$label"; link_root="$TMP_ROOT/symlink-link-$label"
  mkdir -p "$real_root"
  ln -s "$real_root" "$link_root"
  proj="$link_root/proj"
  wt="$TMP_ROOT/symlink-wt-$label"
  id="spawnsymlink$label"
  mx_git_worktree "$real_root/proj" "$wt" "mx/$id"
  # TMP_ROOT itself can already sit behind an OS-level symlink (e.g. macOS's
  # /var -> /private/var), so resolve the fakebin's "physical" reply with
  # pwd -P rather than string concatenation - it must match exactly what
  # mx-spawn.sh's own PROJ_ABS_REAL computes, including any symlink layers
  # ABOVE this test's own synthetic real_root/link_root pair.
  proj_phys=$(cd "$real_root/proj" && pwd -P)
  case "$first_reply" in
    physical) initial_path=$proj_phys ;;
    logical) initial_path=$proj ;;
    *) fail "unknown symlink first-reply mode: $first_reply" ;;
  esac
  fb=$(make_spawn_symlink_fakebin "$TMP_ROOT/symlink-fake-$label" "$initial_path" "$wt")
  data="$TMP_ROOT/symlink-data-$label"
  mkdir -p "$data/$id"
  printf 'test brief content\n' > "$data/$id/brief.md"
  state="$TMP_ROOT/symlink-state-$label"; config="$TMP_ROOT/symlink-config-$label"
  mkdir -p "$state" "$config"
  log="$TMP_ROOT/symlink-spawn-$label.log"

  out=$(run_spawn_case "$ROOT" "$fb" "$log" "$state" "$data" "$config" "$proj" -- "$id" "$proj" claude 2>&1)
  rc=$?
  expect_code 0 "$rc" "mx-spawn.sh should succeed for a project reached through a symlinked prefix when the backend reports $first_reply cwd"$'\n'"$out"
  assert_contains "$out" "worktree=$wt" \
    "mx-spawn.sh did not resolve a symlinked-prefix project to its real worktree when the backend reports $first_reply cwd"

  rm -rf "/tmp/mx-$id"
}

test_spawn_symlinked_project_prefix_avoids_false_refusal() {
  run_spawn_symlink_case physical physical
  run_spawn_symlink_case logical logical
  pass "mx-spawn.sh: a project reached through a symlinked prefix (e.g. macOS /tmp -> /private/tmp) does not trip the isolation guard's false refusal"
}

# --- old vs new: mx-teardown.sh ----------------------------------------------

make_teardown_fakebin() {  # <dir> -> echoes fakebin dir; logs tmux+treehouse calls
  local dir=$1 fb="$1/fakebin"
  mkdir -p "$fb"
  cat > "$fb/tmux" <<'SH'
#!/usr/bin/env bash
set -u
{ printf 'tmux'; for a in "$@"; do printf '\x1f%s' "$a"; done; printf '\n'; } >> "${MX_TMUX_LOG:?}"
exit 0
SH
  cat > "$fb/treehouse" <<'SH'
#!/usr/bin/env bash
set -u
{ printf 'treehouse'; for a in "$@"; do printf '\x1f%s' "$a"; done; printf '\n'; } >> "${MX_TMUX_LOG:?}"
exit 0
SH
  chmod +x "$fb/tmux" "$fb/treehouse"
  printf '%s\n' "$fb"
}

run_teardown_case() {
  local script=$1 fmroot=$2 fb=$3 log=$4 state=$5 data=$6 config=$7 id=$8
  : > "$log"
  env PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$fmroot" \
    MX_STATE_OVERRIDE="$state" MX_DATA_OVERRIDE="$data" MX_CONFIG_OVERRIDE="$config" \
    MX_TMUX_LOG="$log" \
    "$script" "$id"
}

test_teardown_conformance_old_vs_new() {
  local old_bin fb proj wt id
  local state_old state_new config_old config_new data log_old log_new out_old out_new rc_old rc_new
  old_bin=$(build_old_bin teardown-old)
  proj="$TMP_ROOT/teardown-project"; wt="$TMP_ROOT/teardown-wt"
  id="teardownconform1"
  mx_git_worktree "$proj" "$wt" "mx/$id"
  fb=$(make_teardown_fakebin "$TMP_ROOT/teardown-fake")

  data="$TMP_ROOT/teardown-data"
  mkdir -p "$data/$id"
  printf 'scout findings\n' > "$data/$id/report.md"
  printf '## In flight\n\n## Queued\n\n## Done\n' > "$data/backlog.md"

  state_old="$TMP_ROOT/teardown-state-old"; state_new="$TMP_ROOT/teardown-state-new"
  config_old="$TMP_ROOT/teardown-config-old"; config_new="$TMP_ROOT/teardown-config-new"
  mkdir -p "$state_old" "$state_new" "$config_old" "$config_new"

  mx_write_meta "$state_old/$id.meta" \
    "window=broker:mx-$id" "worktree=$wt" "project=$proj" "harness=claude" "kind=scout" "mode=deep-review" "yolo=off" \
    "decisions_reviewed=1" "decision_keys="
  mx_write_meta "$state_new/$id.meta" \
    "window=broker:mx-$id" "worktree=$wt" "project=$proj" "harness=claude" "kind=scout" "mode=deep-review" "yolo=off" \
    "decisions_reviewed=1" "decision_keys="
  touch "$state_old/.last-watcher-beat" "$state_new/.last-watcher-beat"

  log_old="$TMP_ROOT/teardown-old.log"; log_new="$TMP_ROOT/teardown-new.log"
  out_old=$(run_teardown_case "$old_bin/bin/mx-teardown.sh" "$old_bin" "$fb" "$log_old" "$state_old" "$data" "$config_old" "$id" 2>&1)
  rc_old=$?
  out_new=$(run_teardown_case "$ROOT/bin/mx-teardown.sh" "$ROOT" "$fb" "$log_new" "$state_new" "$data" "$config_new" "$id" 2>&1)
  rc_new=$?

  expect_code 0 "$rc_old" "old mx-teardown.sh (scout, report present) should succeed"$'\n'"$out_old"
  expect_code 0 "$rc_new" "new mx-teardown.sh (scout, report present) should succeed"$'\n'"$out_new"
  diff -u "$log_old" "$log_new" > "$TMP_ROOT/teardown-diff.txt" 2>&1 \
    || fail "mx-teardown.sh: tmux+treehouse command log differs old vs new"$'\n'"$(cat "$TMP_ROOT/teardown-diff.txt")"
  assert_contains "$(cat "$log_new")" "treehouse"$'\x1f''return'$'\x1f''--force'$'\x1f'"$wt" \
    "teardown did not call treehouse return --force <worktree>"
  assert_contains "$(cat "$log_new")" "tmux"$'\x1f''kill-window'$'\x1f''-t'$'\x1f'"broker:mx-$id" \
    "teardown did not call tmux kill-window -t <window>"

  pass "mx-teardown.sh: treehouse return + tmux kill-window command log stays byte-identical across the backlog backend replacement"
}

# --- backend selection loudly refuses an unknown backend --------------------

test_spawn_refuses_unknown_backend_flag() {
  local out status
  # bogus names a backend with no adapter at all.
  out=$(MX_ROOT_OVERRIDE='' MX_HOME='' MX_STATE_OVERRIDE='' MX_DATA_OVERRIDE='' \
    MX_PROJECTS_OVERRIDE='' MX_CONFIG_OVERRIDE='' MX_SPAWN_NO_GUARD=1 \
    "$ROOT/bin/mx-spawn.sh" nope-backend-z1 projects/none claude --backend bogus 2>&1)
  status=$?
  [ "$status" -ne 0 ] || fail "mx-spawn --backend bogus should refuse"
  assert_contains "$out" "unknown backend 'bogus'" "mx-spawn did not name the rejected backend"
  pass "mx-spawn.sh --backend bogus is refused loudly"
}

test_spawn_refuses_codex_app_backend_flag() {
  local out status
  out=$(MX_ROOT_OVERRIDE='' MX_HOME='' MX_STATE_OVERRIDE='' MX_DATA_OVERRIDE='' \
    MX_PROJECTS_OVERRIDE='' MX_CONFIG_OVERRIDE='' MX_SPAWN_NO_GUARD=1 \
    "$ROOT/bin/mx-spawn.sh" nope-codex-app-z1 projects/none claude --backend codex-app 2>&1)
  status=$?
  [ "$status" -ne 0 ] || fail "mx-spawn --backend codex-app should refuse"
  assert_contains "$out" "unknown backend 'codex-app'" "mx-spawn did not preserve the blocked codex-app contract"
  pass "mx-spawn.sh --backend codex-app is refused"
}

test_spawn_refuses_unknown_mx_backend_env() {
  local out status
  out=$(MX_ROOT_OVERRIDE='' MX_HOME='' MX_STATE_OVERRIDE='' MX_DATA_OVERRIDE='' \
    MX_PROJECTS_OVERRIDE='' MX_CONFIG_OVERRIDE='' MX_SPAWN_NO_GUARD=1 MX_BACKEND=bogus \
    "$ROOT/bin/mx-spawn.sh" nope-backend-z2 projects/none claude 2>&1)
  status=$?
  [ "$status" -ne 0 ] || fail "MX_BACKEND=bogus should refuse"
  assert_contains "$out" "unknown backend 'bogus'" "mx-spawn did not name the rejected MX_BACKEND"
  pass "mx-spawn.sh honors MX_BACKEND and refuses an unimplemented value loudly"
}

test_spawn_default_backend_writes_no_meta_field() {
  local proj wt data id state config out
  proj="$TMP_ROOT/nobackend-project"; wt="$TMP_ROOT/nobackend-wt"; data="$TMP_ROOT/nobackend-data"
  id="nobackendz3"
  mx_git_worktree "$proj" "$wt" "mx/$id"
  local fb
  fb=$(make_spawn_fakebin "$TMP_ROOT/nobackend-fake" "$wt")
  mkdir -p "$data/$id"; printf 'brief\n' > "$data/$id/brief.md"
  state="$TMP_ROOT/nobackend-state"; config="$TMP_ROOT/nobackend-config"
  mkdir -p "$state" "$config"

  out=$(PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$ROOT" \
    MX_STATE_OVERRIDE="$state" MX_DATA_OVERRIDE="$data" MX_CONFIG_OVERRIDE="$config" \
    MX_PROJECTS_OVERRIDE="$TMP_ROOT/unused-projects" MX_SPAWN_NO_GUARD=1 TMUX="fake,1,0" \
    MX_TMUX_LOG="$TMP_ROOT/nobackend.log" \
    "$ROOT/bin/mx-spawn.sh" "$id" "$proj" claude --backend tmux 2>&1)
  expect_code 0 $? "explicit --backend tmux should spawn successfully"$'\n'"$out"
  assert_no_grep 'backend=' "$state/$id.meta" \
    "an explicit --backend tmux (the default) must not write backend= to meta (P1 compatibility contract)"
  rm -rf "/tmp/mx-$id"
  pass "mx-spawn.sh: an explicit --backend tmux resolves silently and writes no backend= (missing means tmux)"
}

test_spawn_explicit_backend_flag_beats_autodetect_herdr_env() {
  local proj wt data id state config out fb
  proj="$TMP_ROOT/explicit-backend-project"; wt="$TMP_ROOT/explicit-backend-wt"; data="$TMP_ROOT/explicit-backend-data"
  id="explicitbackendz4"
  mx_git_worktree "$proj" "$wt" "mx/$id"
  fb=$(make_spawn_fakebin "$TMP_ROOT/explicit-backend-fake" "$wt")
  mkdir -p "$data/$id"; printf 'brief\n' > "$data/$id/brief.md"
  state="$TMP_ROOT/explicit-backend-state"; config="$TMP_ROOT/explicit-backend-config"
  mkdir -p "$state" "$config"

  # HERDR_ENV=1 is present (as if broker itself were running under herdr),
  # but an explicit --backend tmux flag must still win outright.
  out=$(PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$ROOT" \
    MX_STATE_OVERRIDE="$state" MX_DATA_OVERRIDE="$data" MX_CONFIG_OVERRIDE="$config" \
    MX_PROJECTS_OVERRIDE="$TMP_ROOT/unused-projects" MX_SPAWN_NO_GUARD=1 TMUX="fake,1,0" HERDR_ENV=1 \
    MX_TMUX_LOG="$TMP_ROOT/explicit-backend.log" \
    "$ROOT/bin/mx-spawn.sh" "$id" "$proj" claude --backend tmux 2>&1)
  expect_code 0 $? "explicit --backend tmux should spawn successfully even with HERDR_ENV=1 set"$'\n'"$out"
  assert_no_grep 'backend=' "$state/$id.meta" \
    "an explicit --backend tmux must win over an ambient HERDR_ENV=1 auto-detect marker"
  rm -rf "/tmp/mx-$id"
  pass "mx-spawn.sh: explicit --backend tmux wins over an ambient HERDR_ENV=1 auto-detect marker"
}

test_spawn_autodetect_nesting_resolves_tmux_silently() {
  local proj wt data id state config out fb
  proj="$TMP_ROOT/nest-project"; wt="$TMP_ROOT/nest-wt"; data="$TMP_ROOT/nest-data"
  id="nestbackendz5"
  mx_git_worktree "$proj" "$wt" "mx/$id"
  fb=$(make_spawn_fakebin "$TMP_ROOT/nest-fake" "$wt")
  mkdir -p "$data/$id"; printf 'brief\n' > "$data/$id/brief.md"
  state="$TMP_ROOT/nest-state"; config="$TMP_ROOT/nest-config"
  mkdir -p "$state" "$config"

  # No --backend, no MX_BACKEND, no config/backend: nothing is explicitly
  # configured, so auto-detect runs. $TMUX and HERDR_ENV=1 are both present
  # (tmux nested inside a herdr pane) - the full mx-spawn.sh pipeline, not just
  # mx_backend_name, must resolve this to tmux and stay completely silent about
  # it (today's default path, byte-identical).
  out=$(PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$ROOT" \
    MX_STATE_OVERRIDE="$state" MX_DATA_OVERRIDE="$data" MX_CONFIG_OVERRIDE="$config" \
    MX_PROJECTS_OVERRIDE="$TMP_ROOT/unused-projects" MX_SPAWN_NO_GUARD=1 TMUX="fake,1,0" HERDR_ENV=1 \
    MX_TMUX_LOG="$TMP_ROOT/nest.log" \
    "$ROOT/bin/mx-spawn.sh" "$id" "$proj" claude 2>&1)
  expect_code 0 $? "mx-spawn.sh should auto-detect tmux and spawn successfully for nested tmux-in-herdr"$'\n'"$out"
  assert_no_grep 'backend=' "$state/$id.meta" \
    "auto-detected nested tmux-in-herdr must resolve to tmux (missing backend= means tmux)"
  case "$out" in
    *NOTICE*) fail "auto-detecting tmux (even nested inside herdr) must stay silent, no NOTICE expected"$'\n'"$out" ;;
  esac
  rm -rf "/tmp/mx-$id"
  pass "mx-spawn.sh: auto-detect resolves nested tmux-in-herdr to tmux and stays silent end to end"
}

test_backend_name_precedence
test_backend_detect_precedence
test_backend_detect_cmux_fallback_bundle_id
test_backend_detect_cmux_fallback_requires_darwin
test_backend_detect_cmux_fallback_tmux_nested_false_positive
test_backend_detect_cmux_fallback_ancestry_pid_match
test_backend_detect_cmux_fallback_ancestry_comm_match
test_backend_detect_cmux_fallback_ancestry_stops_at_launchd
test_backend_name_cmux_fallback_notice
test_backend_name_autodetect_notice
test_backend_name_explicit_beats_detection
test_backend_validate_refuses_unknown
test_backend_source_shell_portable
test_backend_validate_spawn_accepts_known
test_meta_get_and_backend_of_meta
test_resolve_selector_three_forms
test_backend_of_selector_matches_explicit_target_meta
if git -C "$ROOT" cat-file -e "$BASE_REF:bin/mx-send.sh" 2>/dev/null; then
  test_send_conformance_old_vs_new
  test_peek_conformance_old_vs_new
else
  pass "pre-Multplx baseline byte conformance is not applicable across the atomic naming epoch"
fi
test_spawn_symlinked_project_prefix_avoids_false_refusal
if git -C "$ROOT" cat-file -e "$BASE_REF:bin/mx-teardown.sh" 2>/dev/null; then
  test_teardown_conformance_old_vs_new
fi
test_spawn_refuses_unknown_backend_flag
test_spawn_refuses_codex_app_backend_flag
test_spawn_refuses_unknown_mx_backend_env
test_spawn_default_backend_writes_no_meta_field
test_spawn_explicit_backend_flag_beats_autodetect_herdr_env
test_spawn_autodetect_nesting_resolves_tmux_silently
