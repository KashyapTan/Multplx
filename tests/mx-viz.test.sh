#!/usr/bin/env bash
# Behavior tests for the disposable read-only Multplx dashboard.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

CLI="$ROOT/bin/mx-viz.sh"
RUST_SERVICE="$ROOT/crates/multplx-services/src/local_services/viz.rs"
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mx-viz-tests.XXXXXX")
mkdir -p "$ROOT/data" || fail "could not create the dashboard artifact root"
ARTIFACT_DIR=$(mktemp -d "$ROOT/data/.mx-viz-test.XXXXXX") \
  || fail "could not create the dashboard artifact fixture"
PIDS=()

assert_selected_runtime() {
  local pid=$1 command
  command=$(ps -p "$pid" -o command= 2>/dev/null || true)
  printf '%s\n' "$command" | grep -F 'services viz-server' >/dev/null \
    || fail "Rust-selected dashboard PID is not the Rust service: $command"
  ! printf '%s\n' "$command" | grep -E '(^|[/ ])node([ /]|$)' >/dev/null \
    || fail "Rust-selected dashboard started Node: $command"
}

cleanup() {
  local pid
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    kill -TERM "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$ARTIFACT_DIR" "$TMP_ROOT"
}
trap cleanup EXIT

track_pid() {
  PIDS+=("$1")
}

record_value() {
  local record=$1 key=$2
  awk -v key="$key" 'index($0,key "=") == 1 {print substr($0,length(key)+2); exit}' "$record"
}

make_home() {
  local home=$1
  mkdir -p "$home/state" "$home/data" "$home/config" "$home/projects"
  printf '%s\n' '# Backlog' >"$home/data/backlog.md"
}

write_snapshot_fixture() {
  local file=$1 marker=$2 data_root=$3
  node "$ROOT/tests/fixtures/viz/portfolio.mjs" 0 | jq --arg marker "$marker" --arg data "$data_root" '
    . + {marker:$marker,roots:{data:$data},tasks:[],scout_reports:[],
     backlog:{records:[]},main_inventory:{valid:true},daemon_current:{records:[]},
     watcher:{alive:true,stale:false,identity_verified:true,afk:false,beacon_age_secs:1},
     wake_queue:{depth:0,oldest_age_secs:null},
     dispatch_queue:{depth:0,available:true,records:[]},
     headroom:{capacity:20,in_use:0,available:20,at_limit:false},headroom_reason:null,
     vplan_reviews:{records:[]},
     later_feeds:{gate_runs:{supported:true,available:false,records:[]},
       workflow_runs:{supported:true,available:false,records:[]},
       deliveries:{supported:true,available:false,records:[]},
       upstream_drift:{available:false},doctor:{available:true},timeline:{available:true}}}' >"$file"
}

make_readers() {
  local dir=$1
  mkdir -p "$dir"
cat >"$dir/snapshot.sh" <<'SH'
#!/usr/bin/env bash
count=0
[ ! -f "$MX_VIZ_COUNT_FILE" ] || count=$(cat "$MX_VIZ_COUNT_FILE")
printf '%s\n' "$((count + 1))" >"$MX_VIZ_COUNT_FILE"
[ ! -f "$MX_VIZ_DELAY_FILE" ] || sleep "$(cat "$MX_VIZ_DELAY_FILE")"
[ ! -f "$MX_VIZ_FAIL_FILE" ] || { printf '%s\n' 'fixture refresh failed' >&2; exit 9; }
cat "$MX_VIZ_FIXTURE"
SH
cat >"$dir/doctor.sh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' '{"schema":"mx-doctor.v1","worst_severity":"FAIL","exit_code":2,"summary":{"ok":0,"warn":0,"fail":1},"findings":[{"check":"fixture","severity":"FAIL"}],"fixes":[]}'
exit 2
SH
  cat >"$dir/timeline.sh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' '{"ts":"2026-07-31T12:00:00Z","source":"fixture","event":"started","detail":{"safe":true}}'
SH
  chmod +x "$dir/snapshot.sh" "$dir/doctor.sh" "$dir/timeline.sh"
}

make_real_snapshot_reader() {
  local dir=$1
  make_readers "$dir"
  cat >"$dir/snapshot.sh" <<SH
#!/usr/bin/env sh
exec "$ROOT/bin/mx-system-snapshot.sh" "\$@"
SH
  chmod +x "$dir/snapshot.sh"
}

start_viz() {
  local home=$1 port=$2 idle=${3:-60} refresh=${4:-0.2} readers=$5 timeout=${6:-2000}
  MX_HOME="$home" MX_VIZ_PORT="$port" MX_VIZ_IDLE_SECS="$idle" \
    MX_VIZ_POLL_MS=77 MX_VIZ_REFRESH_SECS="$refresh" \
    MX_VIZ_COMMAND_TIMEOUT_MS="$timeout" \
    MX_VIZ_SNAPSHOT_BIN="$readers/snapshot.sh" \
    MX_VIZ_DOCTOR_BIN="$readers/doctor.sh" \
    MX_VIZ_TIMELINE_BIN="$readers/timeline.sh" \
    MX_VIZ_FIXTURE="$home/snapshot.json" MX_VIZ_COUNT_FILE="$home/snapshot.count" \
    MX_VIZ_DELAY_FILE="$home/snapshot.delay" MX_VIZ_FAIL_FILE="$home/snapshot.fail" \
    "$CLI" serve
}

wait_http() {
  curl -fsS "$1" >/dev/null 2>&1
}

pid_dead() {
  ! kill -0 "$1" 2>/dev/null
}

path_absent() {
  [ ! -e "$1" ]
}

port_available() {
  node -e '
    const net = require("node:net");
    const server = net.createServer();
    server.once("error", () => process.exit(1));
    server.listen(Number(process.argv[1]), "127.0.0.1", () => server.close(() => process.exit(0)));
  ' "$1"
}

select_test_port_base() {
  local candidates candidate offset available
  if [ -n "${MX_VIZ_TEST_PORT_BASE:-}" ]; then
    case "$MX_VIZ_TEST_PORT_BASE" in
      *[!0-9]*|'') fail "MX_VIZ_TEST_PORT_BASE must be an integer" ;;
    esac
    [ "$MX_VIZ_TEST_PORT_BASE" -ge 1024 ] && [ "$MX_VIZ_TEST_PORT_BASE" -le 65425 ] \
      || fail "MX_VIZ_TEST_PORT_BASE must be from 1024 through 65425"
    candidates=$MX_VIZ_TEST_PORT_BASE
  else
    candidates='52900 53900 54900 55900 56900 57900 58900 59900 60900 61900 62900 63900 64900'
  fi
  for candidate in $candidates; do
    available=true
    for offset in 0 30 60 61 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99; do
      if ! port_available "$((candidate + offset))"; then
        available=false
        break
      fi
    done
    [ "$available" = true ] || continue
    printf '%s\n' "$candidate"
    return 0
  done
  fail "could not find a free visualization test port range"
}

assert_loopback_only() {
  local pid=$1 port=$2 output
  if command -v lsof >/dev/null 2>&1; then
    output=$(lsof -nP -a -p "$pid" -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)
    printf '%s\n' "$output" | grep -F "127.0.0.1:$port" >/dev/null \
      || fail "dashboard socket was not listed on loopback: $output"
    printf '%s\n' "$output" | grep -F "*:$port" >/dev/null \
      && fail "dashboard socket was exposed on all interfaces: $output"
    return
  fi
  grep -Fq 'bind_loopback(first_port)' "$RUST_SERVICE" \
    || fail "no socket inspection tool was available and the literal loopback bind was lost"
}

state_digest() {
  local state=$1
  python3 - "$state" <<'PY'
import hashlib
import os
import sys
from pathlib import Path

root = Path(sys.argv[1])
digest = hashlib.sha256()
for current, directories, files in os.walk(root):
    relative = Path(current).relative_to(root)
    directories[:] = sorted(name for name in directories if not (relative == Path(".") and name == ".viz"))
    for name in sorted(files):
        path = Path(current) / name
        item = path.relative_to(root).as_posix()
        if item == ".viz" or item.startswith(".viz/"):
            continue
        digest.update(item.encode())
        digest.update(b"\0")
        if path.is_symlink():
            digest.update(os.readlink(path).encode())
        else:
            digest.update(path.read_bytes())
print(digest.hexdigest())
PY
}

start_decoy() {
  local first=$1 count=$2 ready=$3
  node - "$first" "$count" "$ready" <<'NODE' &
const fs = require("node:fs");
const net = require("node:net");
const first = Number(process.argv[2]);
const count = Number(process.argv[3]);
const ready = process.argv[4];
const servers = [];
for (let offset = 0; offset < count; offset += 1) {
  const server = net.createServer();
  servers.push(server);
  server.listen(first + offset, "127.0.0.1");
}
let listening = 0;
for (const server of servers) server.on("listening", () => {
  listening += 1;
  if (listening === servers.length) fs.writeFileSync(ready, "ready\n");
});
const stop = () => Promise.all(servers.map((server) => new Promise((resolve) => server.close(resolve)))).then(() => process.exit(0));
process.on("SIGTERM", stop);
NODE
  DECOY_PID=$!
  track_pid "$DECOY_PID"
  mx_test_wait_until 3000 "decoy listeners" test -f "$ready" \
    || fail "decoy listeners did not become ready"
}

etag_changed() {
  local url=$1 old=$2 headers=$3
  curl -fsS -D "$headers" -o /dev/null "$url" || return 1
  ! grep -F "ETag: $old" "$headers" >/dev/null
}

refresh_failed() {
  curl -fsS "$1" | jq -e '.refresh.state == "failed" and .metrics.refresh_failures == 1' >/dev/null
}

refresh_succeeded_twice() {
  curl -fsS "$1" | jq -e '.refresh.state == "idle" and .metrics.refresh_successes == 2' >/dev/null
}

test_lifecycle_cache_and_read_only_contract() {
  local home readers url record pid port before after body headers etag status raw_hash expected_hash mode url2 test_port
  home="$TMP_ROOT/lifecycle"
  readers="$TMP_ROOT/readers-lifecycle"
  make_home "$home"
  make_readers "$readers"
  write_snapshot_fixture "$home/snapshot.json" alpha "$home/data"
  before=$(state_digest "$home/state")

  test_port=$PORT_BASE
  url=$(start_viz "$home" "$test_port" 60 0.2 "$readers") || fail "dashboard did not start"
  [ "$url" = "http://127.0.0.1:$test_port/" ] || fail "dashboard URL mismatch: $url"
  record="$home/state/.viz/server.run"
  [ -f "$record" ] || fail "dashboard did not publish its run record"
  pid=$(record_value "$record" pid)
  port=$(record_value "$record" port)
  track_pid "$pid"
  assert_selected_runtime "$pid"
  if [ "$(uname)" = Darwin ]; then mode=$(stat -f %Lp "$record"); else mode=$(stat -c %a "$record"); fi
  [ "$mode" = 600 ] || fail "dashboard run record mode was $mode, expected 600"
  assert_loopback_only "$pid" "$port"
  mx_test_wait_until 3000 "dashboard HTTP readiness" wait_http "$url" || fail "dashboard was unreachable"

  grep -F 'content="77"' <(curl -fsS "$url") >/dev/null || fail "serve did not inject the configured poll interval"
  grep -F 'Task portfolio' <(curl -fsS "$url") >/dev/null || fail "dashboard portfolio shell was not served"
  curl -fsS "${url}assets/app.js" | grep -F 'If-None-Match' >/dev/null || fail "polling client lacks conditional requests"
  grep -F 'pollInFlight' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard lost non-overlapping client polling"
  grep -F 'document.hidden ? hiddenPollMs : pollMs' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard lost hidden-tab polling backoff"
  grep -F 'task.key || task.id' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard deep links do not prefer home-qualified task identity"
  grep -F 'ArrowDown' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard lost keyboard task navigation"
  grep -F 'Tasks are counted once' "$ROOT/share/viz/index.html" >/dev/null \
    || fail "dashboard no longer distinguishes tasks from sessions and attempts"
  ! grep -REn 'https?://' "$ROOT/share/viz" >/dev/null || fail "dashboard assets contain an external dependency"
  ! grep -REn 'data-approve|btn-approve|Spawn actor|Raise a decision|Pause simulation' "$ROOT/share/viz" >/dev/null \
    || fail "dashboard assets retained demo or decision-write controls"
  grep -F 'Viewer only · respond through the ordinary Multplx workflow' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "decision drawer does not state its read-only boundary"
  grep -F 'hour12: true' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard timestamps no longer force the requested 12-hour clock"
  grep -F 'if (!inside) dialog.close();' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "detail dialog does not close from a backdrop click"
  grep -F 'frame.setAttribute("sandbox", "")' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "rendered HTML artifacts are not isolated in a scriptless sandbox"
  grep -F 'function renderMarkdown(source)' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "Markdown artifacts do not have an in-page renderer"
  grep -F 'artifact.source_path === source' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "canonical evidence pointers are not joined to exact artifact routes"
  grep -F 'X-Multplx-Observation-Age-Ms' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard does not keep age separate from meaningful content changes"

  headers="$home/headers"
  body="$home/body.json"
  curl -fsS -D "$headers" -o "$body" "${url}api/state" || fail "state endpoint failed"
  jq -e '.snapshot.later_feeds.gate_runs.available == false
    and .snapshot.later_feeds.workflow_runs.available == false
    and .snapshot.later_feeds.deliveries.available == false
    and ([.artifacts[]? | select(.root == "plans")] | length) == 0' "$body" >/dev/null \
    || fail "state envelope confused absent feeds or retained obsolete port-plan artifacts"
  [ "$(cat "$home/snapshot.count")" = 1 ] || fail "first state request did not run one snapshot"
  curl -fsS "${url}api/state" >/dev/null || fail "cached state request failed"
  [ "$(cat "$home/snapshot.count")" = 1 ] || fail "fresh cache reran the snapshot"
  etag=$(awk 'tolower($1) == "etag:" {gsub("\r", "", $2); print $2}' "$headers")
  status=$(curl -sS -o /dev/null -w '%{http_code}' -H "If-None-Match: $etag" "${url}api/state")
  [ "$status" = 304 ] || fail "matching ETag returned HTTP $status"
  raw_hash=$(awk 'tolower($1) == "x-multplx-snapshot-hash:" {gsub("\r", "", $2); print $2}' "$headers")
  expected_hash=$(node -e 'const fs=require("node:fs"),c=require("node:crypto");process.stdout.write(c.createHash("sha256").update(fs.readFileSync(process.argv[1],"utf8").trim()).digest("hex"))' "$home/snapshot.json")
  [ "$raw_hash" = "$expected_hash" ] || fail "state endpoint changed canonical snapshot bytes"
  node - "$body" "$home/snapshot.json" <<'NODE' || fail "state envelope did not embed canonical snapshot bytes unchanged"
const fs = require("node:fs");
const body = fs.readFileSync(process.argv[2], "utf8");
const expected = fs.readFileSync(process.argv[3], "utf8").trim();
const marker = '"snapshot":';
const raw = body.slice(body.indexOf(marker) + marker.length, -2);
if (raw !== expected) process.exit(1);
NODE

  write_snapshot_fixture "$home/snapshot.json" beta "$home/data"
  mx_test_wait_until 3000 "snapshot cache expiry" etag_changed "${url}api/state" "$etag" "$home/new-headers" \
    || fail "state hash did not change after canonical snapshot bytes changed"
  [ "$(cat "$home/snapshot.count")" -ge 2 ] || fail "expired cache did not refresh the snapshot"

  curl -fsS "${url}api/doctor" | jq -e '.exit_code == 2 and .findings[0].severity == "FAIL"' >/dev/null \
    || fail "explicit doctor endpoint discarded a diagnostic nonzero report"
  curl -fsS "${url}api/timeline/task-1" | jq -e '.records[0].event == "started"' >/dev/null || fail "timeline endpoint bypassed or lost the reader result"
  status=$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$url")
  [ "$status" = 405 ] || fail "non-GET request returned HTTP $status"
  status=$(curl -sS -o /dev/null -w '%{http_code}' -X PUT "$url")
  [ "$status" = 405 ] || fail "PUT request returned HTTP $status"

  url2=$(start_viz "$home" "$test_port" 60 0.2 "$readers") || fail "idempotent serve failed"
  [ "$url2" = "$url" ] || fail "singleton serve returned a different URL"
  [ "$(record_value "$record" pid)" = "$pid" ] || fail "singleton serve replaced the live server"
  MX_HOME="$home" "$CLI" status | grep -F "running: $url" >/dev/null || fail "status did not report the live dashboard"
  MX_HOME="$home" "$CLI" stop >/dev/null || fail "dashboard stop failed"
  mx_test_wait_until 3000 "dashboard exit" pid_dead "$pid" || fail "dashboard process survived stop"
  mx_test_wait_until 1000 "dashboard record cleanup" path_absent "$record" || fail "dashboard record survived stop"
  port_available "$port" || fail "dashboard port was not released"
  after=$(state_digest "$home/state")
  [ "$after" = "$before" ] || fail "serve, poll, doctor, timeline, or stop mutated operational state"
  pass "viz lifecycle is singleton, loopback-only, cached, conditional, byte-preserving, and operationally read-only"
}

test_single_flight_stale_service_and_bounded_refresh() {
  local home readers url pid test_port header output meta i etag failed_headers failed_body status
  local -a callers=()
  home="$TMP_ROOT/single-flight"
  readers="$TMP_ROOT/readers-single-flight"
  make_home "$home"
  make_readers "$readers"
  write_snapshot_fixture "$home/snapshot.json" alpha "$home/data"
  test_port=$((PORT_BASE + 20))
  url=$(start_viz "$home" "$test_port" 60 0.2 "$readers" 500) \
    || fail "single-flight dashboard did not start"
  pid=$(record_value "$home/state/.viz/server.run" pid)
  track_pid "$pid"
  curl -fsS -D "$home/initial.headers" -o "$home/initial.json" "${url}api/state" \
    || fail "initial snapshot refresh failed: $(cat "$home/initial.json" 2>/dev/null)"
  etag=$(awk 'tolower($1) == "etag:" {sub(/\r$/, "", $2); print $2}' "$home/initial.headers")
  [ -n "$etag" ] || fail "initial snapshot response omitted ETag"
  [ "$(cat "$home/snapshot.count")" = 1 ] || fail "initial cache launched more than one reader"

  sleep 0.25
  printf '%s\n' 1 >"$home/snapshot.delay"
  for i in $(seq 1 12); do
    header="$home/stale-$i.headers"
    output="$home/stale-$i.json"
    curl --max-time 0.5 -fsS -D "$header" -o "$output" "${url}api/state" &
    callers+=("$!")
    PIDS+=("$!")
  done
  for i in $(seq 1 12); do
    wait "${callers[$((i - 1))]}" \
      || fail "stale caller $i blocked on the stalled snapshot reader"
    grep -Fi 'X-Multplx-Cache: stale' "$home/stale-$i.headers" >/dev/null \
      || fail "stale caller $i did not receive stale cache metadata"
    jq -e '.snapshot.marker == "alpha"' "$home/stale-$i.json" >/dev/null \
      || fail "stale caller $i lost the last good snapshot"
  done
  mx_test_wait_until 2000 "bounded refresh failure" refresh_failed "${url}api/meta" \
    || fail "stalled snapshot reader did not fail within its deadline"
  [ "$(cat "$home/snapshot.count")" = 2 ] \
    || fail "multiple viewers launched independent snapshot readers"
  meta=$(curl -fsS "${url}api/meta") || fail "metrics endpoint failed after stalled refresh"
  printf '%s\n' "$meta" | jq -e '
    .metrics.refresh_attempts == 2 and
    .metrics.refresh_successes == 1 and
    .metrics.refresh_failures == 1 and
    .metrics.stale_serves >= 12 and
    .metrics.max_refresh_ms >= 400 and
    .metrics.max_refresh_ms < 900' >/dev/null \
    || fail "refresh metrics did not account for cache sharing and the bounded failure: $meta"
  failed_headers="$home/stale-failed.headers"
  failed_body="$home/stale-failed.json"
  status=$(curl -sS -D "$failed_headers" -o "$failed_body" -H "If-None-Match: $etag" \
    -w '%{http_code}' "${url}api/state")
  [ "$status" = 304 ] || fail "failed stale conditional request returned HTTP $status"
  grep -Fi 'X-Multplx-Cache: stale' "$failed_headers" >/dev/null \
    || fail "failed stale response lost stale cache metadata"
  grep -Fi 'X-Multplx-Refresh: failed' "$failed_headers" >/dev/null \
    || fail "failed stale response lost refresh state"
  awk '
    BEGIN { found = 0 }
    tolower($0) ~ /^x-multplx-refresh-error:/ {
      sub(/\r$/, "")
      value = $0
      sub(/^[^:]*:[[:space:]]*/, "", value)
      if (value != "" && value !~ /[[:cntrl:]]/) found = 1
    }
    END { exit !found }
  ' "$failed_headers" \
    || fail "failed stale 304 omitted its bounded single-line refresh error"

  rm "$home/snapshot.delay"
  sleep 0.25
  curl -fsS "${url}api/state" >/dev/null || fail "stale cache was unavailable during retry"
  mx_test_wait_until 2000 "refresh recovery" refresh_succeeded_twice "${url}api/meta" \
    || fail "snapshot refresh did not recover after the stalled provider cleared"
  [ "$(cat "$home/snapshot.count")" = 3 ] || fail "refresh recovery launched duplicate readers"
  MX_HOME="$home" "$CLI" stop >/dev/null || fail "single-flight dashboard did not stop"
  pass "viz shares one bounded refresh across viewers and serves stale last-good state during provider failure"
}

test_real_child_brief_artifact_projection() {
  local home child child_real nested nested_real readers url pid test_port brief model coordinator_model body artifact_url served head branch
  home="$TMP_ROOT/real-artifact-parent"
  child="$TMP_ROOT/real-artifact-child"
  nested="$TMP_ROOT/real-artifact-nested"
  readers="$TMP_ROOT/readers-real-artifact"
  make_home "$home"
  mkdir -p "$child/state/brief-revisions" "$child/data/work" "$child/config" "$child/projects" "$child/bin"
  printf '%s\n' '# Isolated fixture contract' >"$child/AGENTS.md"
  printf '%s\n' domain >"$child/.mx-daemon-home"
  child_real=$(cd "$child" && pwd -P)
  mkdir -p "$nested/state/brief-revisions" "$nested/data/work" "$nested/config" "$nested/projects" "$nested/bin"
  printf '%s\n' '# Isolated nested fixture contract' >"$nested/AGENTS.md"
  printf '%s\n' nested >"$nested/.mx-daemon-home"
  nested_real=$(cd "$nested" && pwd -P)
  printf '%s\n' '# Backlog' '## In flight' '- [ ] nested - Coordinate nested fixture (repo: sample, kind: coordination)' >"$child/data/backlog.md"
  printf -- '- nested - nested fixture domain (home: %s; scope: fixture; projects: sample; added 2026-09-17)\n' \
    "$nested" >"$child/data/daemons.md"
  printf '%s\n' '# Backlog' '## In flight' '- [ ] work - Implement fixture (repo: sample, kind: delivery)' >"$nested/data/backlog.md"
  git -C "$nested" init -q
  git -C "$nested" config user.email fixture@example.invalid
  git -C "$nested" config user.name Fixture
  printf '%s\n' fixture >"$nested/.fixture"
  git -C "$nested" add .fixture
  git -C "$nested" commit -qm fixture
  head=$(git -C "$nested" rev-parse HEAD)
  branch=$(git -C "$nested" symbolic-ref --short HEAD)
  printf -- '- domain - fixture domain (home: %s; scope: fixture; projects: sample; added 2026-09-17)\n' \
    "$child" >"$home/data/daemons.md"
  mx_write_daemon_meta "$home/state/domain.meta" "$child" 'fixture:mx-domain' sample
  printf '%s\n' 'working: fixture domain' >"$home/state/domain.status"
  brief="$nested/state/brief-revisions/work-1.md"
  printf '%s\n' '# Accepted brief' '' 'Render this exact accepted child-home brief.' >"$brief"
  printf '%s\n' '# Report' >"$nested/data/work/report.md"
  coordinator_model=$(jq -nc --arg owner "$child" --arg runtime "$nested" '
    {schema_version:2,task_id:"nested",role:"sub-orchestrator",artifact:"coordination",persistent:false,private_home:false,
     parent_id:"domain",root_id:"domain",owner_home:$owner,owner_state:($owner+"/state"),parent_state:($owner+"/state"),parent_home:$owner,
     persistent_home:$runtime,home_allocation:null,accepted_brief_digest:"fixture-nested",accepted_brief_path:null,
     runtime:{provider:"tmux",session_id:null,endpoint:null},attempt:{id:"attempt-nested-1",generation:1,brief_revision:1},
     accepted_brief_revision:1,briefs:[{revision:1,scope:"Coordinate nested fixture",acceptance_criteria:["nested work is visible"],source_artifacts:[],reason:"fixture"}],
     prior_attempts:[],retained_executions:[],native_observations:[],assignments:[{generation:1,role:"sub-orchestrator",brief_revision:1,reason:"fixture"}],
     schedule:{priority:1,dependencies:[],state:"running",waiting_condition:null,decisions:[]},project:null,allocation:null,
     domain:{domain_id:"nested",coordinator_id:"nested",scope_revision:1,assignment_generation:1,projects:["sample"],idea_id:null,scope:"fixture"},
     owning_coordinator:"domain",transfers:[],delivery:{current_commit:null,history:[]},legacy_unknown:false}'
  ) || fail "could not create canonical nested coordinator fixture"
  model=$(jq -nc --arg owner "$home" --arg runtime "$child" '
    {schema_version:2,task_id:"domain",role:"sub-orchestrator",artifact:"coordination",persistent:true,private_home:false,
     parent_id:"root",root_id:"root",owner_home:$owner,owner_state:($owner+"/state"),parent_state:($owner+"/state"),parent_home:$owner,
     persistent_home:$runtime,home_allocation:null,accepted_brief_digest:"fixture-domain",accepted_brief_path:null,
     runtime:{provider:"tmux",session_id:null,endpoint:"fixture:mx-domain"},attempt:{id:"attempt-domain-1",generation:1,brief_revision:1},
     accepted_brief_revision:1,briefs:[{revision:1,scope:"Coordinate fixture",acceptance_criteria:["nested domain is visible"],source_artifacts:[],reason:"fixture"}],
     prior_attempts:[],retained_executions:[],native_observations:[],assignments:[{generation:1,role:"sub-orchestrator",brief_revision:1,reason:"fixture"}],
     schedule:{priority:1,dependencies:[],state:"running",waiting_condition:null,decisions:[]},project:null,allocation:null,
     domain:{domain_id:"domain",coordinator_id:"domain",scope_revision:1,assignment_generation:1,projects:["sample"],idea_id:null,scope:"fixture"},
     owning_coordinator:"root",transfers:[],delivery:{current_commit:null,history:[]},legacy_unknown:false}'
  ) || fail "could not create canonical direct coordinator fixture"
  printf '%s\n' 'schema_version=2' >>"$home/state/domain.meta"
  printf 'canonical_model=%s\n' "$model" >>"$home/state/domain.meta"
  {
    printf '%s\n' 'backend=tmux' 'project=sample' 'kind=delivery' 'mode=coordination' 'schema_version=2'
    printf 'worktree=%s\n' "$nested"
    printf 'canonical_model=%s\n' "$coordinator_model"
  } >"$child/state/nested.meta"
  printf '%s\n' 'working: nested fixture coordinator' >"$child/state/nested.status"
  mkdir -p "$child/state/nested.gate"
  jq -nc --arg worktree "$nested_real" --arg branch "$branch" --arg head "$head" \
    '{version:1,task:"nested",worktree:$worktree,branch:$branch,approved_head:$head,status:"running",step:"test",round:1}' \
    >"$child/state/nested.gate/run.json"
  model=$(jq -nc --arg home "$nested" --arg brief "$brief" '
    {schema_version:2,task_id:"work",role:"implementer",artifact:"implementation",persistent:false,private_home:false,
     parent_id:"nested",root_id:"domain",owner_home:$home,owner_state:($home+"/state"),parent_state:($home+"/state"),parent_home:$home,
     persistent_home:null,home_allocation:null,accepted_brief_digest:"fixture",accepted_brief_path:$brief,
     runtime:{provider:"tmux",session_id:null,endpoint:null},attempt:{id:"attempt-work-1",generation:1,brief_revision:1},
     accepted_brief_revision:1,briefs:[{revision:1,scope:"Implement fixture",acceptance_criteria:["brief is visible"],source_artifacts:[],reason:"fixture"}],
     prior_attempts:[],retained_executions:[],native_observations:[],assignments:[{generation:1,role:"implementer",brief_revision:1,reason:"fixture"}],
     schedule:{priority:1,dependencies:[],state:"running",waiting_condition:null,decisions:[]},project:null,allocation:null,domain:null,
     owning_coordinator:"nested",transfers:[],delivery:{current_commit:null,history:[]},legacy_unknown:false}'
  ) || fail "could not create canonical child task fixture"
  {
    printf '%s\n' 'backend=tmux' 'project=sample' 'kind=delivery' 'mode=delivery' 'schema_version=2'
    printf 'worktree=%s\n' "$nested"
    printf 'canonical_model=%s\n' "$model"
  } >"$nested/state/work.meta"
  printf '%s\n' 'working: fixture task' >"$nested/state/work.status"
  mkdir -p "$nested/state/work.gate"
  jq -nc --arg worktree "$nested_real" --arg branch "$branch" --arg head "$head" \
    '{version:1,task:"work",worktree:$worktree,branch:$branch,approved_head:$head,status:"running",step:"test",round:1}' \
    >"$nested/state/work.gate/run.json"
  make_real_snapshot_reader "$readers"
  test_port=$((PORT_BASE + 30))
  url=$(start_viz "$home" "$test_port" 60 2 "$readers" 2000) \
    || fail "real artifact dashboard did not start"
  pid=$(record_value "$home/state/.viz/server.run" pid)
  track_pid "$pid"
  body="$home/real-state.json"
  curl -fsS "${url}api/state" >"$body" || fail "real canonical snapshot did not load"
  jq -e --arg direct "$child_real" --arg home "$nested" --arg validated "$nested_real" --arg brief "$brief" '
    (.snapshot.daemon_current.records | map(select(.home == $direct and .valid == true and .provenance.selected == "structured-home")) | length == 1)
    and (.snapshot.daemon_current.records | map(select(.home == $home)) | length == 0)
    and (.snapshot.portfolio.tasks[] | select(.key == ($home + "#task:work"))
      | .owner.home == $home and .brief.path == $brief)
    and (.snapshot.domains.records[] | select(.coordinator.id == "nested")
      | .coordinator.validated_home == $validated
        and (.portfolio.tasks[] | select(.id == "work") | .owner.home == $home and .brief.path == $brief))
  ' "$body" >/dev/null || {
    jq '{tasks:.snapshot.tasks,portfolio:.snapshot.portfolio,daemon_current:.snapshot.daemon_current.records,domains:.snapshot.domains.records}' "$body" >&2
    fail "real child portfolio lost canonical owner or accepted brief"
  }
  artifact_url=$(jq -r --arg brief "$brief" '.artifacts[] | select(.source_path == $brief) | .url' "$body")
  case "$artifact_url" in
    /artifact/ref/*) : ;;
    *) fail "accepted brief did not receive an opaque artifact URL: $artifact_url" ;;
  esac
  served="$home/served-brief.md"
  curl -fsS "${url%/}$artifact_url" >"$served" || fail "accepted brief artifact URL was not readable"
  cmp -s "$brief" "$served" || fail "accepted brief artifact route changed file bytes"
  MX_HOME="$home" "$CLI" stop >/dev/null || fail "real artifact dashboard did not stop"
  pass "viz projects a real validated nested owner and serves its exact accepted brief through an opaque route"
}

test_artifact_boundary_and_get_only_server() {
  local home readers url pid status outside link headers html test_port
  home="$TMP_ROOT/artifacts"
  readers="$TMP_ROOT/readers-artifacts"
  make_home "$home"
  make_readers "$readers"
  write_snapshot_fixture "$home/snapshot.json" artifacts "$home/data"
  outside="$TMP_ROOT/outside.txt"
  printf '%s\n' secret >"$outside"
  link="$ARTIFACT_DIR/escape.txt"
  ln -s "$outside" "$link"
  html="$ARTIFACT_DIR/rendered.html"
  printf '%s\n' '<!doctype html><style>body{color:green}</style><script>window.parent.document.body.textContent="unsafe"</script><h1>Rendered artifact</h1>' >"$html"
  test_port=$((PORT_BASE + 30))
  url=$(start_viz "$home" "$test_port" 60 0.2 "$readers") || fail "artifact dashboard did not start"
  pid=$(record_value "$home/state/.viz/server.run" pid)
  track_pid "$pid"

  status=$(curl -sS -o /dev/null -w '%{http_code}' "${url}artifact/plans/15-viz.html")
  [ "$status" = 403 ] || fail "obsolete port-plan artifact returned HTTP $status"
  status=$(curl --path-as-is -sS -o /dev/null -w '%{http_code}' "${url}artifact/plans/%2e%2e/CLAUDE.md")
  [ "$status" = 403 ] || fail "encoded traversal returned HTTP $status"
  status=$(curl --path-as-is -sS -o /dev/null -w '%{http_code}' "${url}artifact/data//etc/passwd")
  [ "$status" = 403 ] || fail "absolute-path shape returned HTTP $status"
  status=$(curl --path-as-is -sS -o /dev/null -w '%{http_code}' "${url}artifact/data/$(basename "$ARTIFACT_DIR")/escape.txt")
  [ "$status" = 403 ] || fail "symlink escape returned HTTP $status"
  status=$(curl -sS -o /dev/null -w '%{http_code}' "${url}artifact/not-allowed/README.md")
  [ "$status" = 403 ] || fail "non-allowlisted root returned HTTP $status"
  status=$(curl -sS -o /dev/null -w '%{http_code}' "${url}artifact/docs/not-present.md")
  [ "$status" = 404 ] || fail "missing allowlisted artifact returned HTTP $status"
  headers="$home/artifact-headers"
  curl -fsS -D "$headers" -o "$home/rendered.html" \
    "${url}artifact/data/$(basename "$ARTIFACT_DIR")/rendered.html" \
    || fail "allowlisted HTML artifact was not served"
  grep -Fi 'X-Frame-Options: SAMEORIGIN' "$headers" >/dev/null \
    || fail "HTML artifacts cannot be framed safely inside the same-origin viewer"
  grep -Fi "Content-Security-Policy: default-src 'none'" "$headers" >/dev/null \
    || fail "HTML artifact frame lacks a deny-by-default content policy"
  grep -Fi "style-src 'self' 'unsafe-inline'" "$headers" >/dev/null \
    || fail "HTML artifact frame cannot render its authored inline styles"
  grep -Fi "script-src 'none'" "$headers" >/dev/null \
    || fail "HTML artifact frame does not block scripts explicitly"
  grep -F '<h1>Rendered artifact</h1>' "$home/rendered.html" >/dev/null \
    || fail "HTML artifact bytes were not preserved for browser rendering"
  status=$(curl -sS -o /dev/null -w '%{http_code}' -X DELETE "${url}api/state")
  [ "$status" = 405 ] || fail "DELETE state returned HTTP $status"
  MX_HOME="$home" "$CLI" stop >/dev/null || fail "artifact dashboard did not stop"
  pass "artifact serving rejects traversal, symlink escape, non-allowlisted roots, and every non-GET method"
}

test_port_walk_exhaustion_idle_and_stale_record_safety() {
  local home readers ready url pid port output stale_home decoy record first_port exhausted_port
  home="$TMP_ROOT/ports"
  readers="$TMP_ROOT/readers-ports"
  make_home "$home"
  make_readers "$readers"
  write_snapshot_fixture "$home/snapshot.json" ports "$home/data"

  ready="$home/one.ready"
  first_port=$((PORT_BASE + 60))
  start_decoy "$first_port" 1 "$ready"
  url=$(start_viz "$home" "$first_port" 1 0.2 "$readers") || fail "dashboard did not walk past a busy port"
  [ "$url" = "http://127.0.0.1:$((first_port + 1))/" ] || fail "dashboard selected the wrong fallback port: $url"
  pid=$(record_value "$home/state/.viz/server.run" pid)
  port=$(record_value "$home/state/.viz/server.run" port)
  track_pid "$pid"
  [ "$port" = "$((first_port + 1))" ] || fail "run record lost fallback port"
  mx_test_wait_until 3000 "dashboard idle exit" pid_dead "$pid" || fail "idle dashboard did not exit"
  mx_test_wait_until 1000 "idle record cleanup" path_absent "$home/state/.viz/server.run" || fail "idle exit left a run record"
  [ ! -e "$home/snapshot.count" ] || fail "an unpolled dashboard executed the snapshot"

  ready="$home/all.ready"
  exhausted_port=$((PORT_BASE + 80))
  start_decoy "$exhausted_port" 20 "$ready"
  if output=$(start_viz "$home" "$exhausted_port" 60 0.2 "$readers" 2>&1); then
    fail "dashboard started with all 20 candidate ports occupied"
  fi
  printf '%s\n' "$output" | grep -F 'no loopback port available' >/dev/null \
    || fail "port exhaustion lacked a precise error: $output"
  [ ! -e "$home/state/.viz/server.run" ] || fail "port exhaustion published a run record"

  stale_home="$TMP_ROOT/stale"
  make_home "$stale_home"
  mkdir -p "$stale_home/state/.viz"
  sleep 30 &
  decoy=$!
  track_pid "$decoy"
  record="$stale_home/state/.viz/server.run"
  cat >"$record" <<EOF
version=1
home=$stale_home
state=$stale_home/state
port=$((PORT_BASE + 110))
pid=$decoy
pid_identity=definitely-not-the-decoy
token=0123456789abcdef0123456789abcdef
started_at=2026-07-31T12:00:00Z
EOF
  chmod 600 "$record"
  MX_HOME="$stale_home" "$CLI" stop | grep -F 'removed stale dashboard record' >/dev/null \
    || fail "stop did not classify a mismatched process identity as stale"
  kill -0 "$decoy" 2>/dev/null || fail "stale record cleanup signaled an unrelated process"
  [ ! -e "$record" ] || fail "stale run record survived cleanup"
  pass "viz walks bounded ports, fails closed on exhaustion, idles out, and never signals a reused PID"
}

test_self_containment_and_contract_headers() {
  local help
  help=$($CLI --help)
  printf '%s\n' "$help" | grep -F 'MX_VIZ_PORT (default 4890) plus 19 upward ports' >/dev/null \
    || fail "CLI header lost the bounded default port contract"
  printf '%s\n' "$help" | grep -F 'MX_VIZ_IDLE_SECS (default' >/dev/null \
    || fail "CLI header lost the idle contract"
  printf '%s\n' "$help" | grep -F 'state/.viz/server.run' >/dev/null \
    || fail "CLI header lost the run-record contract"
  grep -F 'TcpListener::bind(("127.0.0.1", port))' "$ROOT/crates/multplx-services/src/local_services/mod.rs" >/dev/null \
    || fail "Rust server lost the literal loopback bind"
  grep -F 'bind_loopback(first_port)' "$RUST_SERVICE" >/dev/null \
    || fail "Rust server lost the bounded port-selection boundary"
  local legacy_reference
  legacy_reference=$(printf '%s%s/' fir stmate)
  ! grep -REn "$legacy_reference" "$CLI" "$RUST_SERVICE" "$ROOT/share/viz" >/dev/null \
    || fail "production viz implementation depends on the read-only upstream reference tree"
  pass "viz is self-contained and keeps its public contract in executable headers"
}

test_portfolio_scale_fixtures_and_ui_contract() {
  local count fixture
  for count in 0 1 5 10 20; do
    fixture="$TMP_ROOT/portfolio-$count.json"
    node "$ROOT/tests/fixtures/viz/portfolio.mjs" "$count" >"$fixture" \
      || fail "could not generate $count-task portfolio fixture"
    jq -e --argjson count "$count" '
      .portfolio.schema == "mx-portfolio.v1"
      and .portfolio.counts.tasks == $count
      and (.portfolio.tasks | length) == (.portfolio.counts.tasks + .portfolio.counts.coordinators)
      and ([.portfolio.tasks[] | select((.key | length) == 0 or (.project.display_name | length) == 0)] | length) == 0
      and ([.portfolio.tasks[] | select((.attempt.generation | type) != "number" or (.sessions | type) != "array" or (.prior_attempts | type) != "array")] | length) == 0
    ' "$fixture" >/dev/null || fail "$count-task fixture does not match the canonical task-first projection"
  done
  jq -e '
    .portfolio.counts.tasks == 20
    and .portfolio.counts.sessions == .portfolio.counts.records
    and .portfolio.counts.attempts > .portfolio.counts.tasks
    and ([.portfolio.tasks[].role] | unique | length) == 4
    and ([.portfolio.tasks[] | select((.children | length) > 0)] | length) > 0
    and ([.portfolio.tasks[] | select((.decisions | length) > 0)] | length) > 0
    and ([.portfolio.tasks[] | select(.allocation.observation.state == "retained")] | length) > 0
    and .portfolio.freshness.partial == true
    and .domains.complete == false
  ' "$TMP_ROOT/portfolio-20.json" >/dev/null || fail "20-task fixture does not exercise nested roles, retries, decisions, retained allocations, and partial state"
  node --check "$ROOT/share/viz/app.js" || fail "dashboard client has invalid JavaScript syntax"
  ! grep -REn 'data-approve|data-merge|data-spawn|method="post"' "$ROOT/share/viz" >/dev/null \
    || fail "dashboard UI crossed its read-only boundary"
  pass "viz has exact 0/1/5/10/20 task fixtures and task-first read-only interaction coverage"
}

PORT_BASE=$(select_test_port_base) || fail "could not select visualization test ports"

test_lifecycle_cache_and_read_only_contract
test_single_flight_stale_service_and_bounded_refresh
test_real_child_brief_artifact_projection
test_artifact_boundary_and_get_only_server
test_port_walk_exhaustion_idle_and_stale_record_safety
test_self_containment_and_contract_headers
test_portfolio_scale_fixtures_and_ui_contract
