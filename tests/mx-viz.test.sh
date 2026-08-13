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
  [ "${MX_LOCAL_SERVICES_IMPLEMENTATION:-rust}" = rust ] || return 0
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
  jq -n --arg marker "$marker" --arg data "$data_root" '
    {schema:"mx-system-snapshot.v1",generated:"2026-07-31T12:00:00Z",
     marker:$marker,roots:{data:$data},tasks:[],scout_reports:[],
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

start_viz() {
  local home=$1 port=$2 idle=${3:-60} refresh=${4:-0.2} readers=$5
  MX_HOME="$home" MX_VIZ_PORT="$port" MX_VIZ_IDLE_SECS="$idle" \
    MX_VIZ_POLL_MS=77 MX_VIZ_REFRESH_SECS="$refresh" \
    MX_VIZ_SNAPSHOT_BIN="$readers/snapshot.sh" \
    MX_VIZ_DOCTOR_BIN="$readers/doctor.sh" \
    MX_VIZ_TIMELINE_BIN="$readers/timeline.sh" \
    MX_VIZ_FIXTURE="$home/snapshot.json" MX_VIZ_COUNT_FILE="$home/snapshot.count" \
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
  grep -F 'Maintainer → broker → workers' <(curl -fsS "$url") >/dev/null || fail "dashboard tree shell was not served"
  curl -fsS "${url}assets/app.js" | grep -F 'If-None-Match' >/dev/null || fail "polling client lacks conditional requests"
  grep -F '${headroom.in_use}/${headroom.capacity}' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard no longer renders the compact used/capacity headroom ratio"
  ! grep -F '${headroom.available} free' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard retained redundant free-headroom text"
  ! grep -F 'Fork watch' "$ROOT/share/viz/index.html" >/dev/null \
    || fail "dashboard retained the fork-watch panel"
  ! grep -REn 'https?://' "$ROOT/share/viz" >/dev/null || fail "dashboard assets contain an external dependency"
  ! grep -REn 'data-approve|btn-approve|Spawn actor|Raise a decision|Pause simulation' "$ROOT/share/viz" >/dev/null \
    || fail "dashboard assets retained demo or decision-write controls"
  grep -F 'Viewer only · respond through the ordinary Multplx workflow' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "decision drawer does not state its read-only boundary"
  grep -F 'formatLocalTime(record.ts, { seconds: true })' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "timeline timestamps are not localized for the browser"
  grep -F 'hour12: true' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "dashboard timestamps no longer force the requested 12-hour clock"
  ! grep -F 'JSON.stringify(record.detail)' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "timeline still renders raw JSON detail"
  grep -F 'function humanizeEvent(event)' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "timeline event labels are not humanized"
  grep -F 'function reconcileActors(tasks)' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "actor polling still lacks keyed DOM reconciliation"
  ! sed -n '/function reconcileActors(tasks)/,/^}/p' "$ROOT/share/viz/app.js" | grep -F 'clear(row)' >/dev/null \
    || fail "actor reconciliation still destroys the full row on every poll"
  grep -F 'if (!inside) dialog.close();' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "detail dialog does not close from a backdrop click"
  grep -F 'frame.setAttribute("sandbox", "")' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "rendered HTML artifacts are not isolated in a scriptless sandbox"
  grep -F 'function renderMarkdown(source)' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "Markdown artifacts do not have an in-page renderer"
  grep -F 'function renderGateRuns(target, records)' "$ROOT/share/viz/app.js" >/dev/null \
    || fail "deep-review gates still use the sparse generic renderer"

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

PORT_BASE=$(select_test_port_base) || fail "could not select visualization test ports"

test_lifecycle_cache_and_read_only_contract
test_artifact_boundary_and_get_only_server
test_port_walk_exhaustion_idle_and_stale_record_safety
test_self_containment_and_contract_headers
