#!/usr/bin/env bash
# mx-send from-broker marker for daemon targets.
#
# A daemon is itself a broker, so a request relayed to it lands in its own
# chat - which the main broker never reads (the only channel back is the terse
# status file). mx-send therefore prepends a from-broker marker
# (bin/mx-marker-lib.sh) when, and only when, the resolved target is a task
# selector whose meta records kind=daemon, so the daemon can recognize
# the request and route its reply via the status path. These tests pin that
# behavior hermetically (stubbed tmux, no real agent):
#   1. Exact-id and stable-label kind=daemon selectors prepend the marker.
#   2. Exact-id and stable-label ordinary actor selectors stay unmarked.
#   3. Explicit endpoints stay unmarked, with or without matching local meta.
#   4. The --key path never carries the marker.
#   5. Direct maintainer text stays unmarked, and already-marked text is idempotent.
#   6. The marker is the label plus terminal-safe U+2063 INVISIBLE SEPARATOR.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
# shellcheck source=/dev/null
. "$ROOT/bin/mx-marker-lib.sh"

SEND="$ROOT/bin/mx-send.sh"

TMP_ROOT=$(mx_test_tmproot mx-send-marker)

# A fake tmux that (a) records the literal text of every `send-keys -l` to
# MX_SEND_LOG and (b) lets mx-send's submit path reach a clean "empty" verdict.
# display-message yields a numeric cursor_y; capture-pane returns an empty
# bordered composer so mx_tmux_composer_state reads "empty" (submit landed) on the
# first Enter. Only the literal (-l) text is logged; Enter retries and --key sends
# are not, so the log holds exactly what was typed into the composer.
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
    while [ $# -gt 0 ]; do
      case "$1" in
        -t) shift 2 ;;
        -l) literal=1; shift ;;
        *) break ;;
      esac
    done
    if [ "$literal" = 1 ]; then
      printf '%s' "${1:-}" >> "$MX_SEND_LOG"
    fi
    exit 0 ;;
  display-message)
    for a in "$@"; do case "$a" in *cursor_y*) printf '0\n'; exit 0 ;; esac; done
    printf 'fakepane\n'; exit 0 ;;
  capture-pane) printf '\xe2\x94\x82 \xe2\x94\x82\n'; exit 0 ;;
  list-windows) exit 0 ;;
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

# run_send <fakebin> <home> <send-log> -- <mx-send args...>
# Runs mx-send.sh with the stubs on PATH against the given home (which holds
# state/<id>.meta). MX_ROOT_OVERRIDE points at the same non-repo home so
# mx-guard's tangle check stays silent; guard noise goes to stderr (discarded).
# MX_SEND_SETTLE=0 keeps the run fast. Truncates the log first; returns mx-send's
# exit code.
run_send() {
  local fb=$1 home=$2 log=$3; shift 3
  : > "$log"
  env PATH="$fb:$PATH" \
    MX_ROOT_OVERRIDE="$home" MX_HOME="$home" MX_SEND_LOG="$log" MX_SEND_SETTLE=0 \
    "$SEND" "$@" 2>/dev/null
}

# setup_home <name> -> echoes a fresh home dir with an empty state/.
setup_home() {
  local home="$TMP_ROOT/$1-$RANDOM"
  mkdir -p "$home/state"
  printf '%s\n' "$home"
}

test_daemon_target_is_marked() {
  local dir fb log home rc got corr
  dir="$TMP_ROOT/sm"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); log="$dir/send.log"
  home=$(setup_home sm)
  mx_write_daemon_meta "$home/state/domain.meta" "$home" "sess:mx-domain"
  run_send "$fb" "$home" "$log" "mx-domain" "audit the build"; rc=$?
  expect_code 0 "$rc" "send to a daemon target should succeed"
  got=$(cat "$log")
  case "$got" in
    "$MX_FROM_BROKER_MARK"corr=[a-f0-9][a-f0-9]*) : ;;
    *) fail "daemon send: literal text should be marker+corr+text"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -c)" ;;
  esac
  case "$got" in
    *audit\ the\ build) : ;;
    *) fail "daemon send lost the request body"$'\n'"$got" ;;
  esac
  # shellcheck source=/dev/null
  . "$ROOT/bin/mx-pending-reply-lib.sh"
  corr=$(mx_pending_reply_extract_corr "$got")
  [ -f "$(mx_pending_reply_path "$home/state" "$corr")" ] \
    || fail "marked daemon send should create a parent pending-reply record"
  pass "mx-send: a kind=daemon target gets the from-broker marker and corr prepended"
}

test_exact_daemon_task_id_is_marked() {
  local dir fb log home rc got already_marked corr
  dir="$TMP_ROOT/sm-exact"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); log="$dir/send.log"
  home=$(setup_home sm-exact)
  mx_write_daemon_meta "$home/state/domain.meta" "$home" "sess:mx-domain"
  run_send "$fb" "$home" "$log" "domain" "audit the build"; rc=$?
  expect_code 0 "$rc" "send to an exact daemon task id should succeed"
  got=$(cat "$log")
  case "$got" in
    "$MX_FROM_BROKER_MARK"corr=[a-f0-9]*) : ;;
    *) fail "exact daemon send: literal text should be marker+corr+text"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -c)" ;;
  esac
  # shellcheck source=/dev/null
  . "$ROOT/bin/mx-pending-reply-lib.sh"
  corr=$(mx_pending_reply_extract_corr "$got")
  # Resend with the same corr already present: embed is idempotent for that corr.
  already_marked="${MX_FROM_BROKER_MARK}corr=${corr} already routed"
  run_send "$fb" "$home" "$log" "domain" "$already_marked"; rc=$?
  expect_code 0 "$rc" "send of already-marked exact-id content should succeed"
  got=$(cat "$log")
  case "$got" in
    "${MX_FROM_BROKER_MARK}corr=${corr} already routed") : ;;
    *) fail "exact daemon send altered already-correlated content"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -tx1)" ;;
  esac
  pass "mx-send: an exact kind=daemon task id is marked with corr exactly once"
}

test_actor_target_is_not_marked() {
  local dir fb log home rc got
  dir="$TMP_ROOT/actors"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); log="$dir/send.log"
  home=$(setup_home actors)
  mx_write_meta "$home/state/build.meta" \
    "window=sess:mx-build" "worktree=$home/wt" "project=$home/p" \
    "harness=echo" "kind=delivery" "mode=deep-review" "yolo=off"
  run_send "$fb" "$home" "$log" "mx-build" "fix the test"; rc=$?
  expect_code 0 "$rc" "send to a stable-label actor target should succeed"
  got=$(cat "$log")
  [ "$got" = "fix the test" ] \
    || fail "stable-label actor send: expected bare text, got marker or other"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -c)"
  run_send "$fb" "$home" "$log" "build" "fix the exact test"; rc=$?
  expect_code 0 "$rc" "send to an exact-id actor target should succeed"
  got=$(cat "$log")
  [ "$got" = "fix the exact test" ] \
    || fail "exact-id actor send: expected bare text, got marker or other"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -c)"
  pass "mx-send: exact-id and stable-label kind=delivery selectors are sent unmarked"
}

test_explicit_window_is_not_marked() {
  local dir fb log home rc got
  dir="$TMP_ROOT/explicit"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); log="$dir/send.log"
  home=$(setup_home explicit)
  # An explicit endpoint is not a task selector, so even matching daemon
  # metadata must not make mx-send guess the caller's intent and mark it.
  mx_write_daemon_meta "$home/state/win.meta" "$home" "other:win"
  run_send "$fb" "$home" "$log" "other:win" "ping"; rc=$?
  expect_code 0 "$rc" "send to an explicit window with matching meta should succeed"
  got=$(cat "$log")
  [ "$got" = "ping" ] \
    || fail "explicit session:window send with meta: expected bare text, got marker"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -c)"

  home=$(setup_home explicit-no-meta)
  run_send "$fb" "$home" "$log" "outside:window" "outside ping"; rc=$?
  expect_code 0 "$rc" "send to an explicit window with no local meta should succeed"
  got=$(cat "$log")
  [ "$got" = "outside ping" ] \
    || fail "explicit session:window send without meta: expected bare text, got marker"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$got" | od -An -c)"
  pass "mx-send: explicit endpoints stay unmarked with or without local metadata"
}

test_key_path_is_not_marked() {
  local dir fb log home rc
  dir="$TMP_ROOT/key"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); log="$dir/send.log"
  home=$(setup_home key)
  mx_write_daemon_meta "$home/state/domain.meta" "$home" "sess:mx-domain"
  run_send "$fb" "$home" "$log" "mx-domain" --key Escape; rc=$?
  expect_code 0 "$rc" "--key send to a daemon should succeed"
  [ ! -s "$log" ] \
    || fail "--key path logged a literal send (marker leaked into a keypress)"$'\n'"--- bytes ---"$'\n'"$(od -An -c "$log")"
  pass "mx-send: the --key path carries no marker (no literal text is typed)"
}

test_marker_is_label_plus_invisible_separator() {
  local separator hex
  separator=$(printf '\342\201\243')
  [ "$MX_FROM_BROKER_MARK" = "[mx-from-broker]$separator" ] \
    || fail "marker is not the expected label + U+2063 sequence"$'\n'"--- bytes ---"$'\n'"$(printf '%s' "$MX_FROM_BROKER_MARK" | od -An -tx1)"
  hex=$(printf '%s' "$MX_FROM_BROKER_MARK" | od -An -tx1 | tr -d ' \n')
  case "$hex" in
    *e281a3) : ;;
    *) fail "marker does not end in UTF-8 U+2063 bytes e2 81 a3; bytes were: $hex" ;;
  esac
  mx_message_from_broker "${MX_FROM_BROKER_MARK}do the work" \
    || fail "detector should recognize a marked message"
  mx_message_from_broker "do the work" \
    && fail "direct maintainer input must remain unmarked"
  mx_message_from_broker "[mx-from-broker]do the work" \
    && fail "detector must reject the label without U+2063"
  pass "mx-send: the marker is '[mx-from-broker]' + terminal-safe U+2063, while direct maintainer text stays unmarked"
}

test_marker_transformation_is_idempotent() {
  local once twice
  mx_message_mark_from_broker "do the work" once
  mx_message_mark_from_broker "$once" twice
  [ "$once" = "$twice" ] \
    || fail "already-marked content was double-prefixed"$'\n'"--- once ---"$'\n'"$(printf '%s' "$once" | od -An -tx1)"$'\n'"--- twice ---"$'\n'"$(printf '%s' "$twice" | od -An -tx1)"
  [ "$once" = "${MX_FROM_BROKER_MARK}do the work" ] \
    || fail "marker transformation did not prefix bare content exactly once"
  pass "mx-marker: from-broker transformation is idempotent"
}

test_marked_send_preserves_trailing_newlines() {
  local dir fb log home rc payload got_hex body_hex corr
  dir="$TMP_ROOT/sm-trailing-newlines"; mkdir -p "$dir"
  fb=$(make_stubs "$dir"); log="$dir/send.log"
  home=$(setup_home sm-trailing-newlines)
  mx_write_daemon_meta "$home/state/domain.meta" "$home" "sess:mx-domain"
  payload=$'audit the build\n\n'
  run_send "$fb" "$home" "$log" "domain" "$payload"; rc=$?
  expect_code 0 "$rc" "marked send with trailing newlines should succeed"
  # shellcheck source=/dev/null
  . "$ROOT/bin/mx-pending-reply-lib.sh"
  corr=$(mx_pending_reply_extract_corr "$(cat "$log")")
  [ -n "$corr" ] || fail "marked send should embed a corr id"
  # Body after marker+corr+space must preserve the original trailing newlines.
  body_hex=$(printf '%s' "$payload" | od -An -tx1 | tr -d ' \n')
  got_hex=$(od -An -tx1 "$log" | tr -d ' \n')
  case "$got_hex" in
    *"$body_hex") : ;;
    *) fail "marked send lost trailing newline body bytes: got $got_hex expected to end with $body_hex" ;;
  esac
  pass "mx-send: marked daemon payload preserves trailing newline bytes"
}

test_daemon_target_is_marked
test_exact_daemon_task_id_is_marked
test_actor_target_is_not_marked
test_explicit_window_is_not_marked
test_key_path_is_not_marked
test_marker_is_label_plus_invisible_separator
test_marker_transformation_is_idempotent
test_marked_send_preserves_trailing_newlines
