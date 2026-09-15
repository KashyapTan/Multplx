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
printf '%s\n' "${MX_QUEUED_MODEL:-}" >> "$MX_QUEUE_TEST_SPAWN_LOG.models"
[ "${MX_QUEUE_FAIL_TASK:-}" != "${1:-}" ]
SH
chmod +x "$FAKE_SPAWN"

# Sources and accepted briefs exist before parking; parking never allocates a
# task worktree or endpoint. Git is real, spawn transport below is a mock.
seed_request() {
  local home=$1 task=$2 project=$3
  mkdir -p "$project" "$home/data/$task"
  if [ ! -d "$project/.git" ]; then
    git -C "$project" init -q -b main || fail 'fixture Git init failed'
    git -C "$project" -c user.name=Fixture -c user.email=fixture@example.test commit --allow-empty -qm base || fail 'fixture base commit failed'
  fi
  printf 'Inspect %s in its recorded repository.\n' "$task" > "$home/data/$task/brief.md"
}
for task in later first keep cancel retry; do
  seed_request "$HOME_DIR" "$task" "$HOME_DIR/projects/$task"
done

queue_cmd() {
  MX_HOME="$HOME_DIR" MX_BACKEND=tmux \
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
  seed_request "$home" parked "$project"
  # Pin the asserted backend independently of the terminal running this test.
  out=$(MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" MX_CONFIG_OVERRIDE="$home/config" \
    MX_DATA_OVERRIDE="$home/data" MX_PROJECTS_OVERRIDE="$home/projects" \
    MX_SPAWN_NO_GUARD=1 MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 \
    MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 MX_HEADROOM_IN_USE=0 \
    MX_HEADROOM_API_CAPACITY=0 "$ROOT/bin/mx-spawn.sh" parked "$project" --harness codex --backend tmux --mode direct-PR --yolo on --resource gpu=2) \
    || fail "at-limit spawn boundary should return a queued outcome"
  assert_contains "$out" 'queued: parked parked until dispatch capacity is available' \
    "spawn boundary did not report the queued outcome"
  assert_grep 'harness=codex' "$home/state/.dispatch-queue/parked.request" \
    "spawn boundary did not preserve the requested harness"
  assert_grep 'backend=tmux' "$home/state/.dispatch-queue/parked.request" \
    "spawn boundary did not preserve the resolved backend"
  assert_absent "$home/state/parked.meta" "at-limit spawn published task metadata"
  [ "$(git -C "$project" worktree list --porcelain | grep -c '^worktree ')" -eq 1 ] || fail "at-limit spawn allocated a worktree"
  assert_grep 'mode=direct-PR' "$home/state/.dispatch-queue/parked.request" 'queue lost selected mode'
  assert_grep 'yolo=off' "$home/state/.dispatch-queue/parked.request" 'legacy yolo became active in queue'
  assert_grep '"gpu":2' "$home/state/.dispatch-queue/parked.request" 'queue lost requested custom resource units'
  MX_HOME="$home" MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 MX_HEADROOM_IN_USE=0 MX_HEADROOM_API_CAPACITY=4 MX_HEADROOM_SPAWN_BIN="$FAKE_SPAWN" MX_QUEUE_TEST_SPAWN_LOG="$home/spawn.log" "$HEADROOM" --queue-drain >/dev/null || fail 'mode queue drain failed'
  assert_grep '--mode direct-PR --yolo off' "$home/spawn.log" 'drain lost selected authority'

  pass "at-limit spawn parks intent before worktree or endpoint allocation"
}

test_queue_add_is_durable_and_visible() {
  local out
  out=$(queue_cmd --queue-add later projects/later --harness codex --model gpt-test --effort high)
  assert_contains "$out" 'queued: later parked' "queue add did not report the parked outcome"
  assert_grep 'task_id=later' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "queue record lost task identity"
  assert_grep "project=$(cd "$HOME_DIR/projects/later" && pwd -P)" "$HOME_DIR/state/.dispatch-queue/later.request" \
    "queue record lost project"
  assert_grep 'harness=codex' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "queue record lost requested profile"

  out=$(queue_cmd --queue)
  assert_contains "$out" $'\tlater\t'"$(cd "$HOME_DIR/projects/later" && pwd -P)"$'\tcodex\tgpt-test\thigh\ttmux\tdelivery' \
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
  assert_grep "first $(cd "$HOME_DIR/projects/first" && pwd -P) --harness claude" "$SPAWN_LOG" \
    "FIFO drain did not launch the oldest request with its profile"
  assert_grep '"role":"researcher"' "$SPAWN_LOG.models" "FIFO drain lost canonical researcher assignment"
  assert_absent "$first_record" "successful drain retained the oldest record"
  assert_grep 'task_id=later' "$HOME_DIR/state/.dispatch-queue/later.request" \
    "one-cycle drain removed a second request"

  queue_cmd --queue-drain >/dev/null || fail "second FIFO drain failed"
  [ "$(wc -l < "$SPAWN_LOG" | tr -d ' ')" -eq 2 ] || fail "second drain did not launch exactly one request"
  assert_grep "later $(cd "$HOME_DIR/projects/later" && pwd -P) --harness codex --model gpt-test --effort high" "$SPAWN_LOG" \
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
  queue_cmd --queue-cancel keep >/dev/null || fail "queue cleanup cancel failed"

  pass "queue cancellation removes exactly the named parked request"
}

test_failed_launch_retains_record_until_reconciled() {
  local record="$HOME_DIR/state/.dispatch-queue/retry.request" out rc=0
  queue_cmd --queue-add retry projects/retry --harness pi >/dev/null
  out=$(MX_QUEUE_FAIL_TASK=retry queue_cmd --queue-drain 2>&1) || rc=$?
  [ "$rc" -ne 0 ] || fail "failed queued launch reported success"
  assert_contains "$out" 'record retained' "failed launch did not explain retry preservation"
  assert_grep 'task_id=retry' "$record" "failed launch dropped its durable record"
  before=$(wc -l < "$SPAWN_LOG" | tr -d ' ')
  queue_cmd --queue-drain >/dev/null || fail "uncertain request made drain fail"
  [ "$(wc -l < "$SPAWN_LOG" | tr -d ' ')" -eq "$before" ] || fail "uncertain endpoint was retried without reconciliation"
  assert_grep 'state=dispatching' "$record" "uncertain request lost its dispatching fence"

  pass "failed queue launch retains an exact crash-recovery record until endpoint reconciliation"
}

test_spawn_boundary_parks_before_allocation
test_queue_add_is_durable_and_visible
test_at_limit_never_dispatches
test_fifo_one_per_cycle_and_exactly_once
test_cancel_removes_only_named_entry
test_failed_launch_retains_record_until_reconciled

echo "ALL TESTS PASSED"

# The capacity-parking path must retain the same delivery authority as a launch.
test_queued_registry_and_existing_task_authority_fail_closed() {
  local home="$TMP_ROOT/authority" project="$TMP_ROOT/authority/projects/app" out variant before
  mkdir -p "$home/state" "$home/config" "$home/data" "$project"
  for task in registry existing legacy; do seed_request "$home" "$task" "$project"; done
  printf '%s\n' '- app [direct-PR +yolo] - app' > "$home/data/projects.md"
  parked_spawn() {
    MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" MX_CONFIG_OVERRIDE="$home/config" \
      MX_DATA_OVERRIDE="$home/data" MX_PROJECTS_OVERRIDE="$home/projects" \
      MX_SPAWN_NO_GUARD=1 MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 \
      MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 MX_HEADROOM_IN_USE=0 \
      MX_HEADROOM_API_CAPACITY=0 "$ROOT/bin/mx-spawn.sh" "$@"
  }
  out=$(parked_spawn registry projects/app codex --scout --backend cmux --model pinned --effort high) || fail 'registered scout did not park'
  assert_grep 'kind=scout' "$home/state/.dispatch-queue/registry.request" 'queue lost scout kind'
  assert_grep 'mode=direct-PR' "$home/state/.dispatch-queue/registry.request" 'queue did not read registry mode'
  assert_grep 'yolo=off' "$home/state/.dispatch-queue/registry.request" 'legacy registry yolo became active'
  before=$(cat "$home/state/.dispatch-queue/registry.request")
  printf '%s\n' '- app [local-only] - app' > "$home/data/projects.md"
  MX_HOME="$home" MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 MX_HEADROOM_IN_USE=0 MX_HEADROOM_API_CAPACITY=4 MX_HEADROOM_SPAWN_BIN="$FAKE_SPAWN" MX_QUEUE_TEST_SPAWN_LOG="$home/spawn.log" "$HEADROOM" --queue-drain >/dev/null || fail 'registry queue drain failed'
  assert_grep '--mode direct-PR --yolo off' "$home/spawn.log" 'registry edit silently changed queued authority'
  assert_grep '--backend cmux' "$home/spawn.log" 'registry queue lost backend'
  assert_grep '"role":"researcher"' "$home/spawn.log.models" 'registry queue lost canonical researcher assignment'
  [ -n "$before" ] || fail 'queued authority evidence was absent'

  # New parked work pins identity; legacy task state remains unknown and cannot
  # be upgraded merely by resubmitting a task with the same name.
  parked_spawn existing projects/app --harness codex --mode direct-PR >/dev/null || fail 'canonical task did not park'
  before=$(cat "$home/state/.dispatch-queue/existing.request")
  for variant in mode role invalid-mode invalid-yolo missing-value; do
    case "$variant" in
      mode) set -- --mode local-only ;;
      role) set -- --mode direct-PR --role reviewer ;;
      invalid-mode) set -- --mode unknown ;;
      invalid-yolo) set -- --mode direct-PR --yolo maybe ;;
      missing-value) set -- --mode ;;
    esac
    if parked_spawn existing projects/app --harness codex "$@" >"$home/$variant.out" 2>"$home/$variant.err"; then fail "queue accepted $variant conflicting request"; fi
    [ "$(cat "$home/state/.dispatch-queue/existing.request")" = "$before" ] || fail 'refused queue request changed pending identity'
  done
  parked_spawn existing projects/app --harness codex --mode direct-PR --yolo on >/dev/null || fail 'inert legacy yolo alias refused'
  [ "$(cat "$home/state/.dispatch-queue/existing.request")" = "$before" ] || fail 'legacy yolo retargeted pending work'
  printf 'worktree=%s\nmode=direct-PR\nyolo=on\n' "$project" > "$home/state/legacy.meta"
  before=$(cat "$home/state/legacy.meta")
  if parked_spawn legacy projects/app --harness codex >"$home/legacy.out" 2>"$home/legacy.err"; then fail 'legacy unknown task was silently resumed'; fi
  [ "$(cat "$home/state/legacy.meta")" = "$before" ] || fail 'legacy refusal mutated old metadata'
  assert_absent "$home/state/.dispatch-queue/legacy.request" 'legacy refusal created canonical queued task'
  pass 'queued launch pins canonical repository/assignment identity, keeps yolo inert and rejects conflicting or legacy-unknown relaunches'
}
test_queued_registry_and_existing_task_authority_fail_closed
