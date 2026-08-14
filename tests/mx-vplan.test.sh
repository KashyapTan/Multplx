#!/usr/bin/env bash
# Behavior tests for the one-shot vplan review lifecycle and persistence contract.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

CLI="$ROOT/bin/mx-vplan.sh"
RUST_SERVICE="$ROOT/crates/multplx-services/src/local_services/vplan.rs"
WAKE_LIB="$ROOT/bin/mx-wake-lib.sh"
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mx-vplan-tests.XXXXXX")
mkdir -p "$ROOT/data"
ARTIFACT_ROOT=$(mktemp -d "$ROOT/data/.mx-vplan-test.XXXXXX")
STATE="$TMP_ROOT/state"
PIDS=()

assert_selected_runtime() {
  local pid=$1 command
  command=$(ps -p "$pid" -o command= 2>/dev/null || true)
  printf '%s\n' "$command" | grep -F 'services vplan-server' >/dev/null \
    || fail "Rust-selected review PID is not the Rust service: $command"
  ! printf '%s\n' "$command" | grep -E '(^|[/ ])node([ /]|$)' >/dev/null \
    || fail "Rust-selected review started Node: $command"
}

cleanup() {
  local pid
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    kill -TERM "$pid" 2>/dev/null || true
  done
  sleep 0.05
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    kill -KILL "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]:-}"; do
    [ -n "$pid" ] || continue
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$ARTIFACT_ROOT" "$TMP_ROOT"
}
trap cleanup EXIT

track_pid() {
  PIDS+=("$1")
}

write_artifact() {
  local file=$1
  mkdir -p "$(dirname "$file")"
  cat > "$file" <<'HTML'
<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><title>Fixture plan</title></head>
<body>
<main id="delivery-plan">
  <h1>Delivery</h1>
  <p id="target">push service owns PR-open</p>
</main>
</body>
</html>
HTML
}

artifact_record() {
  local file=$1 canonical hash
  canonical=$(node -e 'process.stdout.write(require("node:fs").realpathSync(process.argv[1]))' "$file")
  hash=$(node -e 'process.stdout.write(require("node:crypto").createHash("sha256").update(process.argv[1]).digest("hex"))' "$canonical")
  printf '%s/.vplan/%s.run\n' "$STATE" "$hash"
}

record_value() {
  local record=$1 key=$2
  awk -v key="$key" 'index($0, key "=") == 1 { print substr($0, length(key) + 2); exit }' "$record"
}

start_review() {
  local file=$1 port=${2:-4870} idle=${3:-60}
  MX_STATE_OVERRIDE="$STATE" MX_VPLAN_PORT="$port" MX_VPLAN_IDLE_SECS="$idle" \
    "$CLI" review "$file"
}

wait_http() {
  local url=$1
  curl -fsS "$url" >/dev/null 2>&1
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

post_payload() {
  local url=$1 token=$2 payload=$3 response=$4
  curl -sS -o "$response" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -H "X-Vplan-Token: $token" \
    --data-binary "@$payload" \
    "${url}confirm"
}

make_payload() {
  local file=$1
  cat > "$file" <<'JSON'
{
  "comments": [
    {
      "id": "c1",
      "selector": "#target",
      "anchor_text": "push service owns PR-open",
      "nearest_heading": "Delivery",
      "comment": "Split approval from PR-open.",
      "ts": "2026-07-29T18:00:00.000Z",
      "resolved": false
    }
  ]
}
JSON
}

assert_loopback_only() {
  local pid=$1 port=$2 output
  if command -v lsof >/dev/null 2>&1; then
    output=$(lsof -nP -a -p "$pid" -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)
    printf '%s\n' "$output" | grep -F "127.0.0.1:$port" >/dev/null \
      || fail "vplan socket was not listed on 127.0.0.1:$port: $output"
    printf '%s\n' "$output" | grep -F "*:$port" >/dev/null \
      && fail "vplan socket was exposed on all interfaces: $output"
    return
  fi
  if command -v ss >/dev/null 2>&1; then
    output=$(ss -ltnp 2>/dev/null | grep ":$port " || true)
    printf '%s\n' "$output" | grep -E "127\\.0\\.0\\.1:$port[[:space:]]" >/dev/null \
      || fail "vplan socket was not listed on loopback: $output"
    return
  fi
  grep -Fq 'bind_loopback(first_port)' "$RUST_SERVICE" \
    || fail "no socket inspection tool was available and the server lost its literal loopback bind"
}

test_round_trip_injection_shutdown_and_loopback() {
  local dir file original expected payload response url record pid token port mode status served comments
  dir="$ARTIFACT_ROOT/round-trip"
  file="$dir/plan.html"
  mkdir -p "$dir"
  write_artifact "$file"
  original="$dir/original.html"
  cp "$file" "$original"
  payload="$dir/payload.json"
  response="$dir/response.json"
  make_payload "$payload"

  port_available 4870 || fail "default review port 4870 was occupied before the test"

  url=$(start_review "$file") || fail "review did not start"
  [ "$url" = "http://127.0.0.1:4870/" ] || fail "default review URL mismatch: $url"
  record=$(artifact_record "$file")
  [ -f "$record" ] || fail "review did not publish a run record"
  pid=$(record_value "$record" pid)
  token=$(record_value "$record" token)
  port=$(record_value "$record" port)
  track_pid "$pid"
  assert_selected_runtime "$pid"
  [ "$(record_value "$record" artifact)" = "$(node -e 'process.stdout.write(require("node:fs").realpathSync(process.argv[1]))' "$file")" ] \
    || fail "run record did not preserve canonical artifact identity"
  if [ "$(uname)" = Darwin ]; then mode=$(stat -f %Lp "$record"); else mode=$(stat -c %a "$record"); fi
  [ "$mode" = 600 ] || fail "run record mode was $mode, expected 600"
  mx_test_wait_until 3000 "vplan HTTP readiness" wait_http "$url" || fail "review never became reachable"
  assert_loopback_only "$pid" "$port"

  served="$dir/served.html"
  curl -fsS "$url" > "$served" || fail "could not fetch served artifact"
  grep -F 'data-vplan-injected' "$served" >/dev/null || fail "served artifact lacks injected SDK markers"
  grep -F '/__vplan/sdk.js' "$served" >/dev/null || fail "served artifact lacks the SDK script"
  cmp -s "$file" "$original" || fail "serve-time injection changed the artifact on disk"

  status=$(post_payload "$url" "$token" "$payload" "$response")
  [ "$status" = 200 ] || fail "confirm returned HTTP $status: $(cat "$response")"
  mx_test_wait_until 4000 "confirm-triggered server exit" pid_dead "$pid" \
    || fail "server stayed alive after confirm"
  mx_test_wait_until 2000 "confirm-triggered run-record cleanup" path_absent "$record" \
    || fail "run record survived confirm"
  port_available "$port" || fail "port $port was not freed after confirm"

  expected="$dir/expected.html"
  node - "$original" "$expected" <<'NODE'
const fs = require("node:fs");
const [sourcePath, outputPath] = process.argv.slice(2);
const source = fs.readFileSync(sourcePath, "utf8");
const block = `<script type="application/json" id="vplan-comments">
[
  {
    "id": "c1",
    "selector": "#target",
    "anchor_text": "push service owns PR-open",
    "nearest_heading": "Delivery",
    "comment": "Split approval from PR-open.",
    "ts": "2026-07-29T18:00:00.000Z",
    "resolved": false
  }
]
</script>
`;
fs.writeFileSync(outputPath, source.replace("</body>", `${block}</body>`));
NODE
  cmp -s "$file" "$expected" || fail "confirmed artifact did not match the golden comment block"
  comments=$(MX_STATE_OVERRIDE="$STATE" "$CLI" comments "$file") || fail "comments command failed"
  node -e '
    const comments = JSON.parse(process.argv[1]);
    if (comments.length !== 1 || comments[0].id !== "c1" || comments[0].resolved !== false) process.exit(1);
  ' "$comments" || fail "comments command did not print the persisted array"
  pass "vplan round-trip preserves bytes, binds loopback, persists comments, exits, and frees the port"
}

start_decoy() {
  local first=$1 count=$2 ready=$3
  node - "$first" "$count" "$ready" <<'NODE' &
const fs = require("node:fs");
const net = require("node:net");
const [first, count, ready] = process.argv.slice(2);
const servers = [];
let pending = Number(count);
for (let offset = 0; offset < Number(count); offset += 1) {
  const server = net.createServer();
  servers.push(server);
  server.listen(Number(first) + offset, "127.0.0.1", () => {
    pending -= 1;
    if (pending === 0) fs.writeFileSync(ready, "ready\n");
  });
}
process.on("SIGTERM", () => {
  let left = servers.length;
  for (const server of servers) server.close(() => { if (--left === 0) process.exit(0); });
});
NODE
  DECOY_PID=$!
  track_pid "$DECOY_PID"
}

test_port_fallback_and_exhaustion() {
  local dir file ready decoy url record pid out rc full_ready full_decoy
  dir="$ARTIFACT_ROOT/ports"
  file="$dir/plan.html"
  mkdir -p "$dir"
  write_artifact "$file"

  ready="$dir/one.ready"
  start_decoy 4870 1 "$ready"
  decoy=$DECOY_PID
  mx_test_wait_until 2000 "single decoy listener" test -e "$ready" || fail "single decoy did not bind"
  url=$(start_review "$file") || fail "fallback review did not start"
  [ "$url" = "http://127.0.0.1:4871/" ] || fail "occupied 4870 did not fall back to 4871: $url"
  record=$(artifact_record "$file")
  pid=$(record_value "$record" pid)
  track_pid "$pid"
  MX_STATE_OVERRIDE="$STATE" "$CLI" stop "$file" >/dev/null || fail "fallback review did not stop"
  mx_test_wait_until 3000 "fallback port release" port_available 4871 || fail "fallback port did not release"
  kill -TERM "$decoy" 2>/dev/null || true
  mx_test_wait_until 3000 "single decoy exit" pid_dead "$decoy" || fail "single decoy did not stop"

  full_ready="$dir/full.ready"
  start_decoy 4870 20 "$full_ready"
  full_decoy=$DECOY_PID
  mx_test_wait_until 3000 "full-range decoy listeners" test -e "$full_ready" \
    || fail "full-range decoy did not bind"
  out=$(start_review "$file" 2>&1)
  rc=$?
  [ "$rc" -ne 0 ] || fail "review succeeded with the full port range occupied"
  printf '%s\n' "$out" | grep -F '4870-4889' >/dev/null \
    || fail "full-range failure did not name 4870-4889: $out"
  [ ! -e "$(artifact_record "$file")" ] || fail "range exhaustion left an orphan run record"
  kill -TERM "$full_decoy" 2>/dev/null || true
  mx_test_wait_until 3000 "full-range decoy exit" pid_dead "$full_decoy" \
    || fail "full-range decoy did not stop"
  pass "vplan walks the bounded port range and fails cleanly when it is exhausted"
}

test_merge_validation_and_malformed_existing_block() {
  local dir file url record pid token payload response status before comments malformed bad_payload
  dir="$ARTIFACT_ROOT/merge"
  file="$dir/plan.html"
  mkdir -p "$dir"
  write_artifact "$file"
  node - "$file" <<'NODE'
const fs = require("node:fs");
const file = process.argv[2];
const source = fs.readFileSync(file, "utf8");
const block = `<script type="application/json" id="vplan-comments">
[
  {
    "id": "c-old",
    "selector": "#target",
    "anchor_text": "push service owns PR-open",
    "nearest_heading": "Delivery",
    "comment": "Keep this history.",
    "ts": "2026-07-29T17:00:00.000Z",
    "resolved": true
  }
]
</script>
`;
fs.writeFileSync(file, source.replace("</body>", `${block}</body>`));
NODE
  payload="$dir/merge.json"
  cat > "$payload" <<'JSON'
{
  "comments": [
    {
      "id": "c-old",
      "selector": "#target",
      "anchor_text": "push service owns PR-open",
      "nearest_heading": "Delivery",
      "comment": "Keep this history.",
      "ts": "2026-07-29T17:00:00.000Z",
      "resolved": false
    },
    {
      "id": "c-new",
      "selector": "#delivery-plan",
      "anchor_text": "Delivery",
      "nearest_heading": "Delivery",
      "comment": "Add the approval stage.",
      "ts": "2026-07-29T19:00:00.000Z",
      "resolved": false
    }
  ]
}
JSON
  response="$dir/merge-response.json"
  url=$(start_review "$file") || fail "merge review did not start"
  record=$(artifact_record "$file")
  pid=$(record_value "$record" pid)
  token=$(record_value "$record" token)
  track_pid "$pid"
  status=$(post_payload "$url" "$token" "$payload" "$response")
  [ "$status" = 200 ] || fail "merge confirm returned HTTP $status: $(cat "$response")"
  mx_test_wait_until 4000 "merge server exit" pid_dead "$pid" || fail "merge server stayed alive"
  comments=$(MX_STATE_OVERRIDE="$STATE" "$CLI" comments "$file") || fail "merged comments could not be read"
  node -e '
    const comments = JSON.parse(process.argv[1]);
    if (comments.length !== 2) process.exit(1);
    if (comments.find((entry) => entry.id === "c-old")?.resolved !== true) process.exit(2);
    if (comments.find((entry) => entry.id === "c-new")?.resolved !== false) process.exit(3);
  ' "$comments" || fail "merge did not append the new comment while preserving resolved history"

  malformed="$dir/malformed.html"
  write_artifact "$malformed"
  node - "$malformed" <<'NODE'
const fs = require("node:fs");
const file = process.argv[2];
const source = fs.readFileSync(file, "utf8");
fs.writeFileSync(file, source.replace("</body>", '<script type="application/json" id="vplan-comments">{bad json</script>\n</body>'));
NODE
  cp "$malformed" "$dir/malformed.before"
  url=$(start_review "$malformed") || fail "malformed-block review did not start"
  record=$(artifact_record "$malformed")
  pid=$(record_value "$record" pid)
  token=$(record_value "$record" token)
  track_pid "$pid"
  status=$(post_payload "$url" "$token" "$payload" "$response")
  [ "$status" = 400 ] || fail "malformed existing block returned HTTP $status"
  cmp -s "$malformed" "$dir/malformed.before" || fail "malformed block refusal changed the artifact"
  kill -0 "$pid" 2>/dev/null || fail "malformed confirm ended the review instead of allowing correction"
  MX_STATE_OVERRIDE="$STATE" "$CLI" stop "$malformed" >/dev/null || fail "malformed review did not stop"

  file="$dir/invalid-payload.html"
  write_artifact "$file"
  before="$dir/invalid-payload.before"
  cp "$file" "$before"
  bad_payload="$dir/invalid.json"
  cat > "$bad_payload" <<'JSON'
{"comments":[{"id":"bad","selector":"#target","anchor_text":"x","nearest_heading":"Delivery","comment":"missing resolved","ts":"2026-07-29T19:00:00Z"}]}
JSON
  url=$(start_review "$file") || fail "invalid-payload review did not start"
  record=$(artifact_record "$file")
  pid=$(record_value "$record" pid)
  token=$(record_value "$record" token)
  track_pid "$pid"
  status=$(post_payload "$url" "$token" "$bad_payload" "$response")
  [ "$status" = 400 ] || fail "missing-field payload returned HTTP $status"
  cmp -s "$file" "$before" || fail "invalid payload changed the artifact"
  status=$(curl -sS -o "$response" -w '%{http_code}' -H 'Content-Type: application/json' \
    --data-binary "@$payload" "${url}confirm")
  [ "$status" = 403 ] || fail "confirm without the review token returned HTTP $status"
  MX_STATE_OVERRIDE="$STATE" "$CLI" stop "$file" >/dev/null || fail "invalid-payload review did not stop"
  pass "vplan merge preserves resolved history and validation failures never write"
}

test_seed_self_containment_and_idle_timeout() {
  local dir file output url record pid
  dir="$ARTIFACT_ROOT/seed"
  file="$dir/plan.html"
  mkdir -p "$dir"
  "$CLI" --self-check || fail "bundled vplan self-check failed"
  grep -F 'bind_loopback(first_port)' "$RUST_SERVICE" >/dev/null \
    || fail "Rust review server lost the bounded port-selection boundary"
  output=$("$CLI" new "$file") || fail "new did not create an artifact"
  [ "$output" = "$file" ] || fail "new printed an unexpected path: $output"
  [ -f "$file" ] || fail "new did not write the artifact"
  if grep -Eiq '(src|href)=["'\'']https?://' "$ROOT/share/vplan/template.html" "$file"; then
    fail "seed template or generated artifact contains an external asset reference"
  fi
  grep -F '../../share/vplan/mermaid.min.js' "$file" >/dev/null \
    || fail "generated task artifact did not receive a relative Mermaid path"
  if "$CLI" new "$file" >"$dir/overwrite.out" 2>"$dir/overwrite.err"; then
    fail "new overwrote an existing artifact"
  fi

  url=$(start_review "$file" 4870 1) || fail "idle-timeout review did not start"
  [ -n "$url" ] || fail "idle-timeout review printed no URL"
  record=$(artifact_record "$file")
  pid=$(record_value "$record" pid)
  track_pid "$pid"
  mx_test_wait_until 4000 "idle-timeout server exit" pid_dead "$pid" \
    || fail "untouched server did not exit after its idle timeout"
  mx_test_wait_until 2000 "idle-timeout run-record cleanup" path_absent "$record" \
    || fail "idle timeout left a run record"
  pass "vplan seed is offline-safe and an idle review cleans itself up"
}

test_stale_run_record_never_signals_reused_pid() {
  local dir file record decoy identity marker
  dir="$ARTIFACT_ROOT/stale"
  file="$dir/plan.html"
  mkdir -p "$dir" "$STATE/.vplan"
  write_artifact "$file"
  marker="$dir/decoy-signal"
  bash -c '
    trap "printf signaled > \"$1\"; exit 0" TERM
    while :; do sleep 1; done
  ' _ "$marker" &
  decoy=$!
  track_pid "$decoy"
  identity=$(MX_STATE_OVERRIDE="$STATE" bash -c '. "$1"; mx_pid_identity "$2"' _ "$WAKE_LIB" "$decoy") \
    || fail "could not identify stale-record decoy"
  record=$(artifact_record "$file")
  cat > "$record" <<EOF
version=1
artifact=$(node -e 'process.stdout.write(require("node:fs").realpathSync(process.argv[1]))' "$file")
port=4870
pid=$decoy
pid_identity=not-$identity
token=0123456789abcdef0123456789abcdef
started_at=2026-07-29T18:00:00Z
EOF
  MX_STATE_OVERRIDE="$STATE" "$CLI" stop "$file" >"$dir/stop.out" \
    || fail "stop refused stale record cleanup"
  kill -0 "$decoy" 2>/dev/null || fail "stop signaled a live PID whose identity did not match"
  [ ! -e "$marker" ] || fail "identity-mismatched decoy received TERM"
  [ ! -e "$record" ] || fail "stale run record was not removed"
  pass "vplan stop cleans stale records without signaling a reused PID"
}

test_round_trip_injection_shutdown_and_loopback
test_port_fallback_and_exhaustion
test_merge_validation_and_malformed_existing_block
test_seed_self_containment_and_idle_timeout
test_stale_run_record_never_signals_reused_pid
