#!/usr/bin/env bash
# Behavior tests for mx-report's durable-first watcher nudge.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

REPORT="$ROOT/bin/mx-report"
WATCH="$ROOT/bin/mx-watch.sh"
WAKE_LIB="$ROOT/bin/mx-wake-lib.sh"
TMP_ROOT=$(mx_test_tmproot mx-nudge)
PIDS=()
WATCHER_PID=

cleanup() {
  local pid
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    kill "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    wait "$pid" 2>/dev/null || true
  done
  mx_test_cleanup
}
trap cleanup EXIT

track_pid() {
  PIDS+=("$1")
}

start_watcher() {  # <home> <poll-seconds> <output>
  local home=$1 poll=$2 output=$3
  mkdir -p "$home/state"
  MX_HOME="$home" MX_ROOT_OVERRIDE="$ROOT" MX_POLL="$poll" \
    MX_SIGNAL_GRACE=0 MX_CHECK_INTERVAL=999999 MX_HEARTBEAT=999999 \
    "$WATCH" >"$output" 2>"$output.err" &
  WATCHER_PID=$!
  track_pid "$WATCHER_PID"
}

wait_for_watcher() {  # <home> <pid>
  local home=$1 pid=$2 i=0
  while [ "$i" -lt 100 ]; do
    if [ "$(cat "$home/state/.watch.lock/pid" 2>/dev/null || true)" = "$pid" ] \
      && [ -s "$home/state/.watch.lock/pid-identity" ] \
      && [ -e "$home/state/.last-watcher-beat" ]; then
      return 0
    fi
    kill -0 "$pid" 2>/dev/null || return 1
    sleep 0.05
    i=$((i + 1))
  done
  return 1
}

wait_for_queue() {  # <home> <tenths>
  local home=$1 limit=$2 i=0
  while [ "$i" -lt "$limit" ]; do
    [ -s "$home/state/.wake-queue" ] && return 0
    sleep 0.1
    i=$((i + 1))
  done
  return 1
}

report_bound() {  # <home> <id> <state> <message>
  local home=$1 id=$2 state=$3 message=$4
  MX_HOME="$home" MX_TASK_ID="$id" "$REPORT" \
    --id "$id" --state "$state" --message "$message"
}

test_live_watcher_wakes_before_poll() {
  local home="$TMP_ROOT/live" id=nudge-live-a1 output="$TMP_ROOT/live-watch" pid
  start_watcher "$home" 8 "$output"
  pid=$WATCHER_PID
  wait_for_watcher "$home" "$pid" || fail "live watcher did not publish a healthy lock"
  sleep 0.3

  report_bound "$home" "$id" blocked "needs maintainer input" \
    || fail "mx-report failed while nudging a live watcher"
  wait_for_queue "$home" 30 \
    || fail "nudge did not enqueue the actionable wake well before the 8-second poll"
  wait "$pid" || fail "nudged watcher did not exit cleanly after its actionable scan"

  [ "$(cat "$home/state/$id.status")" = "blocked: needs maintainer input" ] \
    || fail "live-watcher case did not preserve the durable status line"
  grep -F $'\tsignal\t' "$home/state/.wake-queue" >/dev/null \
    || fail "live-watcher nudge did not produce the ordinary signal queue record"
  pass "mx-report: a valid nudge wakes the real watcher well before its poll interval"
}

test_no_listener_and_dead_lock_are_silent() {
  local home="$TMP_ROOT/no-listener" id=nudge-none-b2 out err rc dead_pid
  mkdir -p "$home/state"
  out="$TMP_ROOT/no-listener.out"
  err="$TMP_ROOT/no-listener.err"

  MX_HOME="$home" MX_TASK_ID="$id" "$REPORT" \
    --id "$id" --state working --message "durable without listener" \
    >"$out" 2>"$err"
  rc=$?
  [ "$rc" -eq 0 ] || fail "no-listener report exited $rc"
  [ ! -s "$out" ] && [ ! -s "$err" ] \
    || fail "no-listener nudge was not a silent no-op"

  sleep 0.01 &
  dead_pid=$!
  wait "$dead_pid"
  mkdir "$home/state/.watch.lock"
  printf '%s\n' "$dead_pid" > "$home/state/.watch.lock/pid"
  printf '%s\n' "stale watcher identity" > "$home/state/.watch.lock/pid-identity"
  : >"$out"
  : >"$err"
  MX_HOME="$home" MX_TASK_ID="$id" "$REPORT" \
    --id "$id" --state paused --message "durable with dead lock" \
    >"$out" 2>"$err"
  rc=$?
  [ "$rc" -eq 0 ] || fail "dead-lock report exited $rc"
  [ ! -s "$out" ] && [ ! -s "$err" ] \
    || fail "dead-lock nudge was not a silent no-op"

  [ "$(cat "$home/state/$id.status")" = $'working: durable without listener\npaused: durable with dead lock' ] \
    || fail "no-listener or dead-lock case changed the durable event grammar"
  pass "mx-report: no listener and a dead watcher lock silently retain durable-only behavior"
}

test_identity_mismatch_never_signals_decoy() {
  local home="$TMP_ROOT/mismatch" id=nudge-mismatch-c3 marker ready decoy identity
  marker="$home/decoy-signaled"
  ready="$home/decoy-ready"
  mkdir -p "$home/state/.watch.lock"

  bash -c '
    on_usr1() { printf "%s\n" delivered > "$1"; }
    trap on_usr1 USR1
    : > "$2"
    while :; do :; done
  ' _ "$marker" "$ready" &
  decoy=$!
  track_pid "$decoy"
  while [ ! -e "$ready" ]; do sleep 0.01; done

  identity=$(MX_STATE_OVERRIDE="$home/state" bash -c \
    '. "$1"; mx_pid_identity "$2"' _ "$WAKE_LIB" "$decoy") \
    || fail "could not identify live decoy process"
  printf '%s\n' "$decoy" > "$home/state/.watch.lock/pid"
  printf '%s\n' "not-$identity" > "$home/state/.watch.lock/pid-identity"

  report_bound "$home" "$id" done "durable mismatch" \
    || fail "identity-mismatch report failed"
  sleep 0.2
  [ ! -e "$marker" ] || fail "mx-report sent USR1 to a PID with mismatched identity"
  kill -0 "$decoy" 2>/dev/null || fail "identity-mismatch nudge killed the decoy process"
  [ "$(cat "$home/state/$id.status")" = "done: durable mismatch" ] \
    || fail "identity-mismatch case lost its durable status line"
  pass "mx-report: a live reused-PID decoy is never signaled when identity does not match"
}

test_opt_out_uses_natural_poll() {
  local home="$TMP_ROOT/disabled" id=nudge-disabled-d4 output="$TMP_ROOT/disabled-watch" pid
  start_watcher "$home" 3 "$output"
  pid=$WATCHER_PID
  wait_for_watcher "$home" "$pid" || fail "opt-out watcher did not publish a healthy lock"
  sleep 0.3

  MX_NUDGE=0 MX_HOME="$home" MX_TASK_ID="$id" "$REPORT" \
    --id "$id" --state blocked --message "poll fallback" \
    || fail "MX_NUDGE=0 changed mx-report success"
  sleep 0.4
  kill -0 "$pid" 2>/dev/null || fail "MX_NUDGE=0 woke the watcher before its natural poll"
  [ ! -s "$home/state/.wake-queue" ] \
    || fail "MX_NUDGE=0 enqueued a wake before the natural poll"

  wait_for_queue "$home" 50 || fail "natural poll did not pick up an opt-out status event"
  wait "$pid" || fail "natural-poll watcher did not exit cleanly"
  [ "$(cat "$home/state/$id.status")" = "blocked: poll fallback" ] \
    || fail "MX_NUDGE=0 changed the durable status line"
  pass "mx-report: MX_NUDGE=0 disables only the fast path and natural polling still surfaces the event"
}

test_live_watcher_wakes_before_poll
test_no_listener_and_dead_lock_are_silent
test_identity_mismatch_never_signals_decoy
test_opt_out_uses_natural_poll

echo "# mx-nudge.test.sh: all assertions passed"
