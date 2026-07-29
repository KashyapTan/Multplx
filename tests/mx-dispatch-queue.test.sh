#!/usr/bin/env bash
# Durable parked dispatch queue: FIFO, restart, cancellation, and limit safety.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

HEADROOM="$ROOT/bin/mx-headroom.sh"
TMP_ROOT=$(mx_test_tmproot mx-dispatch-queue)
HOME_DIR="$TMP_ROOT/home"
SPAWN_LOG="$TMP_ROOT/spawn.log"
FAKE_SPAWN="$TMP_ROOT/fake-spawn"
unset MX_HEADROOM_SKIP_QUEUE
mkdir -p "$HOME_DIR/state"

cat > "$FAKE_SPAWN" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$MX_QUEUE_TEST_SPAWN_LOG"
SH
chmod +x "$FAKE_SPAWN"

queue_cmd() {
  MX_HOME="$HOME_DIR" \
  MX_HEADROOM_CPU_COUNT="${MX_QUEUE_CPU_COUNT:-8}" \
  MX_HEADROOM_LOAD1="${MX_QUEUE_LOAD1:-0}" \
  MX_HEADROOM_MEM_AVAILABLE_BYTES="${MX_QUEUE_MEM:-17179869184}" \
  MX_HEADROOM_IN_USE="${MX_QUEUE_IN_USE:-0}" \
  MX_HEADROOM_API_CAPACITY="${MX_QUEUE_API_CAPACITY:-4}" \
  MX_HEADROOM_SPAWN_BIN="$FAKE_SPAWN" \
  MX_QUEUE_TEST_SPAWN_LOG="$SPAWN_LOG" \
    "$HEADROOM" "$@"
}

test_spawn_boundary_parks_before_allocation() {
  local home="$TMP_ROOT/spawn-boundary" project="$TMP_ROOT/not-allocated" out
  mkdir -p "$home/state" "$home/config"
  out=$(MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" MX_CONFIG_OVERRIDE="$home/config" \
    MX_DATA_OVERRIDE="$home/data" MX_PROJECTS_OVERRIDE="$home/projects" \
    MX_SPAWN_NO_GUARD=1 MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 \
    MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 MX_HEADROOM_IN_USE=0 \
    MX_HEADROOM_API_CAPACITY=0 "$ROOT/bin/mx-spawn.sh" parked "$project" --harness codex) \
    || fail "at-limit spawn boundary should return a queued outcome"
  assert_contains "$out" 'queued: parked parked until dispatch capacity is available' \
    "spawn boundary did not report the queued outcome"
  assert_grep 'harness=codex' "$home/state/.dispatch-queue/parked.request" \
    "spawn boundary did not preserve the requested harness"
  assert_grep 'backend=tmux' "$home/state/.dispatch-queue/parked.request" \
    "spawn boundary did not preserve the resolved backend"
  assert_absent "$home/state/parked.meta" "at-limit spawn published task metadata"
  assert_absent "$project" "at-limit spawn allocated a worktree"

  pass "at-limit spawn parks intent before worktree or endpoint allocation"
}

test_queue_add_is_durable_and_visible() {
  local out
  out=$(queue_cmd --queue-add later projects/later --harness codex --model gpt-test --effort high)
  assert_contains "$out" 'queued: later parked' "queue add did not report the parked outcome"
  assert_grep 'task_id=later' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "queue record lost task identity"
  assert_grep 'project=projects/later' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "queue record lost project"
  assert_grep 'harness=codex' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "queue record lost requested profile"

  out=$(queue_cmd --queue)
  assert_contains "$out" $'\tlater\tprojects/later\tcodex\tgpt-test\thigh\t-\tdelivery' \
    "fresh process could not reconstruct and inspect the queued request"

  pass "at-limit-compatible queue records are durable and restart-reconstructable"
}

test_at_limit_never_dispatches() {
  local before
  before=$(cat "$HOME_DIR/state/.dispatch-queue/later.request")
  MX_QUEUE_API_CAPACITY=0 queue_cmd --queue-drain >/dev/null \
    || fail "at-limit drain should remain a silent no-op"
  MX_QUEUE_API_CAPACITY=0 queue_cmd --queue-drain >/dev/null \
    || fail "repeated at-limit drain should remain a silent no-op"
  [ ! -e "$SPAWN_LOG" ] || [ ! -s "$SPAWN_LOG" ] \
    || fail "at-limit drain invoked spawn"
  [ "$before" = "$(cat "$HOME_DIR/state/.dispatch-queue/later.request")" ] \
    || fail "at-limit drain changed the queued record"

  pass "repeated at-limit drains never dispatch or drop intent"
}

test_fifo_one_per_cycle_and_exactly_once() {
  local first_record="$HOME_DIR/state/.dispatch-queue/first.request"
  : > "$SPAWN_LOG"
  queue_cmd --queue-add first projects/first --harness claude --scout >/dev/null
  # Make ordering deterministic without a real-time sleep.
  sed 's/^enqueued_at=.*/enqueued_at=1/' "$first_record" > "$first_record.next"
  mv "$first_record.next" "$first_record"
  chmod 0600 "$first_record"
  sed 's/^enqueued_at=.*/enqueued_at=2/' "$HOME_DIR/state/.dispatch-queue/later.request" \
    > "$HOME_DIR/state/.dispatch-queue/later.request.next"
  mv "$HOME_DIR/state/.dispatch-queue/later.request.next" "$HOME_DIR/state/.dispatch-queue/later.request"
  chmod 0600 "$HOME_DIR/state/.dispatch-queue/later.request"

  queue_cmd --queue-drain >/dev/null || fail "first FIFO drain failed"
  [ "$(wc -l < "$SPAWN_LOG" | tr -d ' ')" -eq 1 ] || fail "one drain launched more than one request"
  assert_grep 'first projects/first --harness claude --scout' "$SPAWN_LOG" \
    "FIFO drain did not launch the oldest request with its profile"
  assert_absent "$first_record" "successful drain retained the oldest record"
  assert_grep 'task_id=later' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "one-cycle drain removed a second request"

  queue_cmd --queue-drain >/dev/null || fail "second FIFO drain failed"
  [ "$(wc -l < "$SPAWN_LOG" | tr -d ' ')" -eq 2 ] || fail "second drain did not launch exactly one request"
  assert_grep 'later projects/later --harness codex --model gpt-test --effort high' "$SPAWN_LOG" \
    "second drain lost the stored profile"
  assert_absent "$HOME_DIR/state/.dispatch-queue/later.request" "successful second drain retained its record"
  queue_cmd --queue-drain >/dev/null || fail "empty drain failed"
  [ "$(wc -l < "$SPAWN_LOG" | tr -d ' ')" -eq 2 ] || fail "empty drain re-dispatched a removed record"

  pass "queue drains FIFO, at most one per cycle, exactly once"
}

test_cancel_removes_only_named_entry() {
  queue_cmd --queue-add keep projects/keep >/dev/null
  queue_cmd --queue-add cancel projects/cancel >/dev/null
  queue_cmd --queue-cancel cancel >/dev/null || fail "queue cancel failed"
  assert_absent "$HOME_DIR/state/.dispatch-queue/cancel.request" "cancel retained the named entry"
  assert_grep 'task_id=keep' "$HOME_DIR/state/.dispatch-queue/keep.request" \
    "cancel removed a different entry"

  pass "queue cancellation removes exactly the named parked request"
}

test_spawn_boundary_parks_before_allocation
test_queue_add_is_durable_and_visible
test_at_limit_never_dispatches
test_fifo_one_per_cycle_and_exactly_once
test_cancel_removes_only_named_entry

echo "ALL TESTS PASSED"
