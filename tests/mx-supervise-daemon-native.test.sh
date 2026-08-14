#!/usr/bin/env bash
# Black-box native away-supervisor lifecycle, restart, escalation, and submit contract.
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mx_test_tmproot mx-supervise-native)
BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}
case_dir="$TMP_ROOT/case"
home="$case_dir/home"
state="$home/state"
fakebin="$case_dir/fakebin"
mkdir -p "$state" "$fakebin"
printf '1\n' > "$state/.afk"

cat > "$case_dir/watcher" <<'SH'
#!/usr/bin/env bash
set -eu
state=${MX_STATE_OVERRIDE:?}
count=0
[ ! -f "$state/watch-count" ] || count=$(cat "$state/watch-count")
count=$((count + 1))
printf '%s\n' "$count" > "$state/watch-count"
if [ "$count" -eq 1 ]; then
  exit 7
fi
printf 'signal: %s/task-z1.status\n' "$state"
SH
chmod 700 "$case_dir/watcher"

cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -eu
case "$*" in
  *'#{pane_id}'*) printf '%%1\n'; exit 0 ;;
  *'#{pane_current_path}'*) printf '%s\n' "${MX_FAKE_CWD:-/tmp}"; exit 0 ;;
esac
case "${1:-}" in
  capture-pane) printf '› \n'; exit 0 ;;
  send-keys)
    printf '%s\n' "$*" >> "${MX_FAKE_SEND_LOG:?}"
    exit 0
    ;;
  display-message) printf '0\n'; exit 0 ;;
esac
exit 0
SH
chmod 700 "$fakebin/tmux"

send_log="$case_dir/send.log"
: > "$send_log"
PATH="$fakebin:$PATH" MX_HOME="$home" MX_STATE_OVERRIDE="$state" \
  MX_SUPERVISE_WATCH_EXEC="$case_dir/watcher" MX_SUPERVISOR_BACKEND=tmux \
  MX_SUPERVISOR_TARGET='broker:0' MX_FAKE_SEND_LOG="$send_log" \
  MX_INJECT_CONFIRM_SLEEP=0 MX_INJECT_CONFIRM_RETRIES=2 \
  "$BIN" supervise-daemon >"$case_dir/out" 2>"$case_dir/err" &
pid=$!

for _ in $(seq 1 100); do
  [ -f "$state/.supervise-daemon.pid" ] && [ -f "$state/watch-count" ] \
    && [ "$(cat "$state/watch-count")" -ge 2 ] && break
  sleep 0.1
done
[ -f "$state/.supervise-daemon.pid" ] || fail "native supervisor did not publish its pid"
[ "$(cat "$state/watch-count")" -ge 2 ] || fail "native supervisor did not restart its failed watcher"

if PATH="$fakebin:$PATH" MX_HOME="$home" MX_STATE_OVERRIDE="$state" \
    MX_SUPERVISE_WATCH_EXEC="$case_dir/watcher" MX_FAKE_SEND_LOG="$send_log" \
    "$BIN" supervise-daemon >"$case_dir/dup.out" 2>"$case_dir/dup.err"; then
  fail "native supervisor admitted a concurrent second owner"
fi
assert_grep 'another mx-supervise-daemon is already running' "$case_dir/dup.err" \
  "native supervisor duplicate refusal was not explicit"

for _ in $(seq 1 100); do
  grep -F 'MULTPLX_OP: v1 away-supervisor' "$send_log" >/dev/null 2>&1 && break
  sleep 0.1
done
assert_grep 'MULTPLX_OP: v1 away-supervisor' "$send_log" \
  "native supervisor did not submit a typed away-supervisor escalation"
assert_grep 'Supervisor escalate' "$send_log" \
  "native supervisor escalation omitted the digest"

kill -TERM "$pid"
for _ in $(seq 1 50); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.1
done
wait "$pid" || fail "native supervisor did not shut down cleanly"
[ ! -e "$state/.supervise-daemon.pid" ] || fail "native supervisor retained its pidfile"
[ ! -e "$state/.supervise-daemon.lock" ] || fail "native supervisor retained its ownership lock"
pass "native supervisor restarts watcher, serializes ownership, submits typed escalation, and cleans shutdown state"
