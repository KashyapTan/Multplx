#!/usr/bin/env bash
# mx-send strict target resolution.
#
# A send that cannot be tied to a recorded task/lane or to an explicit
# well-formed backend target must fail loudly. These tests pin the historical
# silent-fallback failures: missing MX_HOME, unresolved selectors, prefixless
# herdr pane ids, dead explicit endpoints, and the healthy exact/mx-id paths.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SEND="$ROOT/bin/mx-send.sh"
TMP_ROOT=$(mx_test_tmproot mx-send-strict)

make_stubs() {  # <dir> -> echoes fakebin dir
  local dir=$1 fb="$1/fakebin"
  mkdir -p "$fb"
  cat > "$fb/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "${1:-}" in
  send-keys)
    shift
    literal=0
    target=
    while [ $# -gt 0 ]; do
      case "$1" in
        -t) target=$2; shift 2 ;;
        -l) literal=1; shift ;;
        *) break ;;
      esac
    done
    printf 'send-keys target=%s literal=%s arg=%s\n' "$target" "$literal" "${1:-}" >> "$MX_TMUX_LOG"
    exit 0 ;;
  display-message)
    target=
    while [ $# -gt 0 ]; do
      case "$1" in
        -t) target=$2; shift 2 ;;
        *) shift ;;
      esac
    done
    if [ -n "${MX_FAKE_TMUX_DEAD_TARGET:-}" ] && [ "$target" = "$MX_FAKE_TMUX_DEAD_TARGET" ]; then
      exit 1
    fi
    printf '%%1\n'
    exit 0 ;;
  capture-pane)
    printf '\xe2\x94\x82 \xe2\x94\x82\n'
    exit 0 ;;
  list-windows)
    printf 'foreign:%s\n' "${MX_FAKE_TMUX_WINDOW:-mx-lost}"
    exit 0 ;;
esac
exit 0
SH
  chmod +x "$fb/tmux"
  cat > "$fb/sleep" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fb/sleep"
  printf '%s\n' "$fb"
}

setup_home() {  # <name> -> echoes home dir
  local home="$TMP_ROOT/$1-$RANDOM"
  mkdir -p "$home/state"
  printf '%s\n' "$home"
}

test_exact_lane_id_send_still_works() {
  local dir fb home err log rc got
  dir="$TMP_ROOT/exact"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); home=$(setup_home exact); err="$dir/send.err"; log="$dir/tmux.log"; : > "$log"
  mx_write_meta "$home/state/mpf-lane-m8.meta" "window=sess:mx-mpf-lane-m8" "kind=delivery"

  PATH="$fb:$PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$home" MX_TMUX_LOG="$log" MX_SEND_SETTLE=0 \
    "$SEND" mpf-lane-m8 "lost dispatch" >/dev/null 2>"$err"; rc=$?
  expect_code 0 "$rc" "exact task id send should succeed when metadata exists"
  got=$(cat "$log")
  assert_contains "$got" "target=sess:mx-mpf-lane-m8 literal=1 arg=lost dispatch" "exact id should type literal text to the meta target"
  assert_contains "$got" "target=sess:mx-mpf-lane-m8 literal=0 arg=Enter" "exact id should submit with Enter"
  pass "mx-send strict: exact task/lane ids resolve through home metadata"
}

test_unset_mx_home_fails() {
  local dir fb err log rc
  dir="$TMP_ROOT/nohome"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); err="$dir/send.err"; log="$dir/tmux.log"; : > "$log"

  env -u MX_HOME PATH="$fb:$PATH" MX_ROOT_OVERRIDE="$dir" MX_TMUX_LOG="$log" MX_SEND_SETTLE=0 \
    "$SEND" sess:win "hello" >/dev/null 2>"$err"; rc=$?
  [ "$rc" -ne 0 ] || fail "unset MX_HOME should fail"
  assert_contains "$(cat "$err")" "MX_HOME is not set" "unset MX_HOME diagnostic should be explicit"
  [ ! -s "$log" ] || fail "unset MX_HOME still attempted a send"$'\n'"$(cat "$log")"
  pass "mx-send strict: unset MX_HOME fails before target resolution"
}

test_unresolvable_target_does_not_tmux_fallback() {
  local dir fb home err log rc
  dir="$TMP_ROOT/unresolved"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); home=$(setup_home unresolved); err="$dir/send.err"; log="$dir/tmux.log"; : > "$log"

  PATH="$fb:$PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$home" MX_TMUX_LOG="$log" MX_FAKE_TMUX_WINDOW=lost-target MX_SEND_SETTLE=0 \
    "$SEND" lost-target "hello" >/dev/null 2>"$err"; rc=$?
  [ "$rc" -ne 0 ] || fail "unresolvable target should fail"
  assert_contains "$(cat "$err")" "not resolvable" "unresolvable diagnostic should be loud"
  assert_contains "$(cat "$err")" "metadata window/terminal lookup" "unresolvable diagnostic should name the attempted lookup"
  assert_contains "$(cat "$err")" "backend=none" "unresolvable diagnostic should name that no backend was assumed"
  [ ! -s "$log" ] || fail "unresolvable target fell through to tmux send"$'\n'"$(cat "$log")"
  pass "mx-send strict: unresolvable selectors do not fall back to tmux"
}

test_prefixless_herdr_pane_id_fails() {
  local dir fb home err log rc
  dir="$TMP_ROOT/herdr-pane"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); home=$(setup_home herdr); err="$dir/send.err"; log="$dir/tmux.log"; : > "$log"
  mx_write_meta "$home/state/nudge.meta" \
    "window=default:wB:p2" "backend=herdr" "herdr_session=default" "herdr_pane_id=wB:p2" "kind=delivery"

  PATH="$fb:$PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$home" MX_TMUX_LOG="$log" MX_SEND_SETTLE=0 \
    "$SEND" wB:p2 "nudge" >/dev/null 2>"$err"; rc=$?
  [ "$rc" -ne 0 ] || fail "prefixless herdr pane id should fail"
  assert_contains "$(cat "$err")" "matches herdr_pane_id" "herdr pane diagnostic should name the meta match"
  assert_contains "$(cat "$err")" "expected <herdr-session>:<pane-id>" "herdr pane diagnostic should show expected shape"
  assert_contains "$(cat "$err")" "default:wB:p2" "herdr pane diagnostic should show the canonical target"
  [ ! -s "$log" ] || fail "prefixless herdr pane id fell through to tmux send"$'\n'"$(cat "$log")"
  pass "mx-send strict: prefixless herdr pane ids are rejected before tmux fallback"
}

test_unmatched_single_colon_target_must_exist() {
  local dir fb home err log rc
  dir="$TMP_ROOT/dead-explicit"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); home=$(setup_home deadexplicit); err="$dir/send.err"; log="$dir/tmux.log"; : > "$log"

  PATH="$fb:$PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$home" MX_TMUX_LOG="$log" MX_FAKE_TMUX_DEAD_TARGET=sess:missing MX_SEND_SETTLE=0 \
    "$SEND" sess:missing "hello" >/dev/null 2>"$err"; rc=$?
  [ "$rc" -ne 0 ] || fail "dead explicit tmux-shaped target should fail"
  assert_contains "$(cat "$err")" "not a live tmux endpoint" "dead explicit target diagnostic should name the assumed backend"
  assert_contains "$(cat "$err")" "backend=tmux" "dead explicit target diagnostic should name the tried backend"
  [ ! -s "$log" ] || fail "dead explicit target still attempted a send"$'\n'"$(cat "$log")"
  pass "mx-send strict: unmatched single-colon explicit targets must verify live before sending"
}

test_healthy_mx_id_send_still_works() {
  local dir fb home err log rc got
  dir="$TMP_ROOT/healthy"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); home=$(setup_home healthy); err="$dir/send.err"; log="$dir/tmux.log"; : > "$log"
  mx_write_meta "$home/state/lane-ok.meta" "window=sess:mx-lane-ok" "kind=delivery" "harness=codex"

  PATH="$fb:$PATH" MX_HOME="$home" MX_ROOT_OVERRIDE="$home" MX_TMUX_LOG="$log" MX_SEND_SETTLE=0 \
    "$SEND" mx-lane-ok "hello maintainer" >/dev/null 2>"$err"; rc=$?
  expect_code 0 "$rc" "healthy mx-id send should succeed"
  got=$(cat "$log")
  assert_contains "$got" "target=sess:mx-lane-ok literal=1 arg=hello maintainer" "healthy send should type literal text to the meta target"
  assert_contains "$got" "target=sess:mx-lane-ok literal=0 arg=Enter" "healthy send should submit with Enter"
  assert_contains "$(cat "$err")" "requested message WILL still be sent" "mx-send guard banner should keep send-specific continuation wording"
  pass "mx-send strict: healthy mx-<id> sends still type once and submit"
}

test_exact_lane_id_send_still_works
test_unset_mx_home_fails
test_unresolvable_target_does_not_tmux_fallback
test_prefixless_herdr_pane_id_fails
test_unmatched_single_colon_target_must_exist
test_healthy_mx_id_send_still_works
