#!/usr/bin/env bash
# tests/wake-helpers.sh - shared fixtures and mocks for the wake-queue,
# watcher/lock, and supervise-daemon suites. The fake tmux surfaces here encode
# watcher/daemon/composer behavior, so they live here rather than in the generic
# tests/lib.sh. Generic reporters/assertions come from lib.sh, pulled in below.

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# mx-wake-drain.sh now calls mx-guard.sh to assert watcher liveness on every
# drain. mx-guard.sh's first check warns when the broker PRIMARY checkout
# (MX_ROOT) sits on a feature branch; with no override MX_ROOT resolves to the
# test runner's own checkout, which during validation is on a feature branch, so
# each drain would emit a spurious worktree-tangle banner. Point the tangle check
# at a fresh non-git dir to keep it inert across these suites - the same trick the
# direct mx-guard.sh tests use. A per-call MX_ROOT_OVERRIDE still wins where a
# suite sets its own (e.g. the watcher-lock guard-banner cases).
if [ -z "${MX_ROOT_OVERRIDE:-}" ]; then
  MX_ROOT_OVERRIDE="$(mx_test_tmproot mx-wake-tangle-root)"
  export MX_ROOT_OVERRIDE
fi

# Wedge-alarm notifier recorder (safety seam). The away-mode wedge alarm fires
# real notifiers when configured. Point its MX_WEDGE_ALARM_EXEC
# seam at a recorder for every
# daemon/wake suite, so no test - present or future - can post a real herdr
# or command: notification: it is impossible to forget, because sourcing this harness
# installs it. The recorder is an on-disk script (a real daemon a test spawns
# inherits the path and records too). It logs "<channel>\t<summary>" to
# $MX_WEDGE_ALARM_LOG, which a test sets to its own file to assert on; unset means
# /dev/null. MX_WEDGE_ALARM_FAIL=<channel> makes the recorder exit non-zero for
# that channel, to exercise graceful degradation. Suites that do not source this
# harness still cannot fire a real notification: the daemon defaults the seam to
# "discard" whenever it is sourced (its library-mode guard).
# Create the recorder dir with mktemp directly (not mx_test_tmproot, whose
# first call installs an EXIT trap that, invoked inside a command-substitution
# subshell, would delete the dir on subshell exit). Register it for the same
# cleanup and install the trap in THIS shell if it is the first registration.
_mx_wedge_rec_dir=$(mktemp -d "${TMPDIR:-/tmp}/mx-wedge-rec.XXXXXX")
if [ "${#MX_TEST_CLEANUP_DIRS[@]}" -eq 0 ]; then trap mx_test_cleanup EXIT; fi
MX_TEST_CLEANUP_DIRS+=("$_mx_wedge_rec_dir")
cat > "$_mx_wedge_rec_dir/rec" <<'REC'
#!/usr/bin/env bash
printf '%s\t%s\n' "${1:-}" "${2:-}" >> "${MX_WEDGE_ALARM_LOG:-/dev/null}"
case " ${MX_WEDGE_ALARM_FAIL:-} " in *" ${1:-} "*) exit 1 ;; esac
exit 0
REC
chmod +x "$_mx_wedge_rec_dir/rec"
export MX_WEDGE_ALARM_EXEC="$_mx_wedge_rec_dir/rec"

# append_wake <state> <kind> <key> <payload>: append a wake record to the durable
# queue in a subshell scoped to <state>, using the production wake library.
append_wake() {
  local state=$1 kind=$2 key=$3 payload=$4 lib="$ROOT/bin/mx-wake-lib.sh"
  MX_STATE_OVERRIDE="$state" bash -c '
    # shellcheck disable=SC1090,SC1091
    . "$1"
    mx_wake_append "$2" "$3" "$4"
  ' _ "$lib" "$kind" "$key" "$payload"
}

make_case() {
  local name=$1 dir fakebin
  dir="$TMP_ROOT/$name"
  fakebin="$dir/fakebin"
  mkdir -p "$dir/state" "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
if [ "${1:-}" = "list-windows" ]; then
  if [ -n "${MX_FAKE_TMUX_WINDOW:-}" ]; then
    printf '%s\n' "${MX_FAKE_TMUX_WINDOW#*:}"
  fi
  exit 0
fi
if [ "${1:-}" = "capture-pane" ]; then
  if [ -n "${MX_FAKE_TMUX_CAPTURE:-}" ]; then
    cat "$MX_FAKE_TMUX_CAPTURE"
  fi
  exit 0
fi
if [ "${1:-}" = "display-message" ]; then
  case "$*" in
    *pane_current_command*) printf '%s\n' "${MX_FAKE_TMUX_CURRENT_COMMAND:-}"; exit 0 ;;
  esac
fi
exit 1
SH
  chmod +x "$fakebin/tmux"
  make_fake_actor_state "$fakebin" >/dev/null
  printf '%s\n' "$dir"
}

# Install a hermetic fake mx-actor-state.sh into <fakebin> and echo its path. The
# watcher's absorb-only-when-provably-working triage calls this (via
# MX_ACTOR_STATE_BIN) to read an actor's current state on no-verb signal and stale
# paths; the fake returns a canned "state: <s> · source: <src> · <detail>"
# verdict line so a test can fix the provably-working decision without a real
# worktree or no-mistakes.
# A per-id override MX_FAKE_ACTOR_STATE_<sanitized-id> wins; otherwise the shared
# MX_FAKE_ACTOR_STATE; otherwise an unknown verdict (NOT provably working), the
# safe default so a test that forgets to set one surfaces rather than absorbs.
make_fake_actor_state() {  # <fakebin>
  local fakebin=$1
  cat > "$fakebin/mx-actor-state.sh" <<'SH'
#!/usr/bin/env bash
set -u
id=${1:-}
key=$(printf '%s' "$id" | tr -c 'A-Za-z0-9' '_')
var="MX_FAKE_ACTOR_STATE_$key"
val=${!var:-${MX_FAKE_ACTOR_STATE:-}}
printf '%s\n' "${val:-state: unknown · source: none · fake default}"
exit 0
SH
  chmod +x "$fakebin/mx-actor-state.sh"
  printf '%s\n' "$fakebin/mx-actor-state.sh"
}

make_supercase() {
  local name=$1 dir fakebin
  dir="$TMP_ROOT/$name"
  fakebin="$dir/fakebin"
  mkdir -p "$dir/state" "$fakebin"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "${1:-}" in
  display-message)
    [ "${MX_FAKE_TMUX_PANE_ALIVE:-1}" = "1" ] || exit 1
    _print=0
    # Return cursor_y when the format asks for it (pane_input_pending).
    for _a in "$@"; do
      case "$_a" in *cursor_y*) printf '%s\n' "${MX_FAKE_TMUX_CURSOR_Y:-0}"; exit 0 ;; esac
      [ "$_a" = "-p" ] && _print=1
    done
    [ "$_print" = 1 ] && printf 'fakepane\n'
    exit 0 ;;
  list-windows)
    [ -n "${MX_FAKE_TMUX_WINDOW:-}" ] && printf '%s\n' "$MX_FAKE_TMUX_WINDOW"
    exit 0 ;;
  capture-pane)
    # Honor a single-line band capture (-S N -E M, both non-negative) the way the
    # composer reader now bounds its capture to the cursor row; otherwise (e.g.
    # mx_pane_is_busy's "-S -40" tail) return the whole capture. -e is accepted and
    # ignored: this fake emits plain text, which the dim-stripper passes through.
    _S=""; _E=""; shift
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -S) _S="${2:-}"; shift 2; continue ;;
        -E) _E="${2:-}"; shift 2; continue ;;
        *) shift ;;
      esac
    done
    [ -n "${MX_FAKE_TMUX_CAPTURE:-}" ] || exit 0
    if [ -n "$_S" ] && [ -n "$_E" ]; then
      case "$_S$_E" in
        *[!0-9]*) cat "$MX_FAKE_TMUX_CAPTURE" 2>/dev/null ;;
        *) sed -n "$((_S + 1)),$((_E + 1))p" "$MX_FAKE_TMUX_CAPTURE" 2>/dev/null ;;
      esac
    else
      cat "$MX_FAKE_TMUX_CAPTURE" 2>/dev/null
    fi
    exit 0 ;;
  send-keys)
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -l) shift; [ "$#" -gt 0 ] && {
          printf '%s\n' "$1" >> "${MX_FAKE_TMUX_SENT:-/dev/null}"
          # Reflect sent text into capture so pane_input_pending sees it as
          # pending input (text in the composer).
          [ -n "${MX_FAKE_TMUX_CAPTURE:-}" ] && printf '%s\n' "$1" >> "$MX_FAKE_TMUX_CAPTURE"
        } ;;
        Enter)
          # Optionally swallow Enter (file-based flag) to test the retry path.
          if [ -n "${MX_FAKE_TMUX_SWALLOW_FILE:-}" ] && [ -f "$MX_FAKE_TMUX_SWALLOW_FILE" ]; then
            rm -f "$MX_FAKE_TMUX_SWALLOW_FILE"
          else
            printf '[ENTER]\n' >> "${MX_FAKE_TMUX_SENT:-/dev/null}"
            # Enter submits: clear the last line (the typed text) from the
            # capture, simulating the composer being cleared on submit.
            if [ -n "${MX_FAKE_TMUX_CAPTURE:-}" ] && [ -s "$MX_FAKE_TMUX_CAPTURE" ]; then
              _tmp=$(mktemp 2>/dev/null) || _tmp="${MX_FAKE_TMUX_CAPTURE}.tmp"
              sed '$d' "$MX_FAKE_TMUX_CAPTURE" > "$_tmp" 2>/dev/null && mv -f "$_tmp" "$MX_FAKE_TMUX_CAPTURE"
              rm -f "$_tmp" 2>/dev/null
            fi
          fi
          ;;
      esac
      shift
    done
    exit 0 ;;
esac
exit 1
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$dir"
}

make_bordered_case() {
  local name=$1 dir fakebin
  dir="$TMP_ROOT/$name"; fakebin="$dir/fakebin"
  mkdir -p "$dir/state" "$fakebin"
  printf '│ > │\n' > "$dir/composer"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
COMPOSER="${MX_FAKE_COMPOSER:?MX_FAKE_COMPOSER unset}"
case "${1:-}" in
  display-message)
    print=0
    for a in "$@"; do case "$a" in *cursor_y*) printf '0\n'; exit 0 ;; esac; done
    for a in "$@"; do [ "$a" = "-p" ] && print=1; done
    [ "$print" = 1 ] && printf 'fakepane\n'
    exit 0 ;;
  capture-pane) cat "$COMPOSER" 2>/dev/null; exit 0 ;;
  list-windows) exit 0 ;;
  send-keys)
    shift
    text=""; is_enter=0; lit=0
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -t) shift ;;
        -l) lit=1 ;;
        Enter) is_enter=1 ;;
        *) [ "$lit" = 1 ] && text="$1" ;;
      esac
      shift
    done
    if [ "$is_enter" = 1 ]; then
      if [ -n "${MX_FAKE_SWALLOW:-}" ] && [ -f "$MX_FAKE_SWALLOW" ]; then
        [ "${MX_FAKE_PERSIST_SWALLOW:-0}" = 1 ] || rm -f "$MX_FAKE_SWALLOW"
      else
        [ -n "${MX_FAKE_SENT:-}" ] && printf '[ENTER]\n' >> "$MX_FAKE_SENT"
        printf '│ > │\n' > "$COMPOSER"
      fi
    elif [ "$lit" = 1 ]; then
      [ "${MX_FAKE_SEND_FAIL:-0}" = 1 ] && exit 1
      [ -n "${MX_FAKE_SENT:-}" ] && printf '%s\n' "$text" >> "$MX_FAKE_SENT"
      printf '│ > %s │\n' "$text" > "$COMPOSER"
    fi
    exit 0 ;;
esac
exit 1
SH
  chmod +x "$fakebin/tmux"
  printf '%s\n' "$dir"
}

wait_for_exit() {
  local pid=$1 limit=${2:-50} i=0
  while [ "$i" -lt "$limit" ]; do
    if ! is_live_non_zombie "$pid"; then
      wait "$pid"
      return "$?"
    fi
    sleep 0.1
    i=$((i + 1))
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  return 124
}

is_live_non_zombie() {
  local pid=$1 stat
  kill -0 "$pid" 2>/dev/null || return 1
  stat=$(ps -p "$pid" -o stat= 2>/dev/null || true)
  case "$stat" in
    Z*) return 1 ;;
  esac
  return 0
}

hash_text() {
  if command -v md5 >/dev/null 2>&1; then
    printf '%s' "$1" | md5 -q
  else
    printf '%s' "$1" | md5sum | cut -d' ' -f1
  fi
}

dead_pid() {
  local p=999999
  while kill -0 "$p" 2>/dev/null; do
    p=$((p + 1))
  done
  printf '%s\n' "$p"
}
