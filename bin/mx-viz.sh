#!/usr/bin/env bash
# Start, inspect, and stop the disposable Multplx dashboard.
#
# Usage:
#   mx-viz.sh serve
#   mx-viz.sh status
#   mx-viz.sh stop
#   mx-viz.sh --help
#
# `serve` is singleton and idempotent per MX_HOME. It binds loopback only,
# tries MX_VIZ_PORT (default 4890) plus 19 upward ports, prints the URL only,
# and never opens a browser. The server exits after MX_VIZ_IDLE_SECS (default
# 1800) without a request. Snapshot polling defaults to MX_VIZ_POLL_MS=2500
# with a pull-through MX_VIZ_REFRESH_SECS=2 cache.
#
# Run record contract, state/.viz/server.run, mode 0600:
#   version=1
#   home=<canonical MX_HOME>
#   state=<state directory>
#   port=<bound loopback port>
#   pid=<server pid>
#   pid_identity=<portable process identity from mx-wake-lib.sh>
#   token=<random cleanup binding, never served>
#   started_at=<UTC ISO-8601 time>
# `stop` signals only a live identity-matched process. A dead or reused PID
# causes record cleanup without signaling. The Rust implementation owns these
# mechanics by default; MX_LOCAL_SERVICES_IMPLEMENTATION=legacy selects this
# retained rollback body before any state mutation.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
# Portion 12 Rust-default adapter.
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_local_services_implementation) || exit $?
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT=$ROOT; export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" services mx-viz.sh "$@"
fi
SERVER="$SCRIPT_DIR/mx-viz-server.mjs"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
VIZ_STATE="$STATE/.viz"
RUN_RECORD="$VIZ_STATE/server.run"
SERVE_LOCK="$VIZ_STATE/serve.lock"

usage() {
  sed -n '2,/^set -u$/s/^# \{0,1\}//p' "$0"
}

die() {
  printf 'mx-viz: %s\n' "$*" >&2
  exit 1
}

record_value() {
  local record=$1 key=$2
  awk -v key="$key" 'index($0, key "=") == 1 {print substr($0,length(key)+2); exit}' \
    "$record" 2>/dev/null
}

load_pid_helpers() {
  # shellcheck source=bin/mx-wake-lib.sh disable=SC1091
  . "$SCRIPT_DIR/mx-wake-lib.sh"
}

ensure_state() {
  mkdir -p "$STATE" "$VIZ_STATE" || die "could not create dashboard state directory"
  chmod 700 "$VIZ_STATE" 2>/dev/null || true
}

record_live() {
  local record=$1 pid identity actual home
  [ -f "$record" ] && [ ! -L "$record" ] || return 1
  pid=$(record_value "$record" pid)
  identity=$(record_value "$record" pid_identity)
  home=$(record_value "$record" home)
  [ "$home" = "$(cd "$MX_HOME" 2>/dev/null && pwd -P)" ] || return 1
  mx_pid_alive "$pid" || return 1
  [ -n "$identity" ] || return 1
  actual=$(mx_pid_identity "$pid" 2>/dev/null || true)
  [ "$actual" = "$identity" ]
}

remove_record_if_matches() {
  local record=$1 expected_pid=$2 expected_token=$3 pid token
  [ -f "$record" ] && [ ! -L "$record" ] || return 0
  pid=$(record_value "$record" pid)
  token=$(record_value "$record" token)
  if [ "$pid" = "$expected_pid" ] && [ "$token" = "$expected_token" ]; then
    rm -f "$record"
  fi
}

serve_dashboard() {
  local canonical_home canonical_state port token ready_log error_log pid line counter rc identity started temporary
  ensure_state
  load_pid_helpers
  mx_lock_acquire_wait "$SERVE_LOCK"
  trap 'mx_lock_release "$SERVE_LOCK"' EXIT
  canonical_home=$(cd "$MX_HOME" 2>/dev/null && pwd -P) || die "MX_HOME is unavailable: $MX_HOME"
  canonical_state=$(cd "$STATE" 2>/dev/null && pwd -P) || die "state directory is unavailable: $STATE"

  if [ -e "$RUN_RECORD" ] || [ -L "$RUN_RECORD" ]; then
    if record_live "$RUN_RECORD"; then
      port=$(record_value "$RUN_RECORD" port)
      printf 'http://127.0.0.1:%s/\n' "$port"
      mx_lock_release "$SERVE_LOCK"
      trap - EXIT
      return 0
    fi
    [ -f "$RUN_RECORD" ] && [ ! -L "$RUN_RECORD" ] \
      || die "unsafe dashboard run record: $RUN_RECORD"
    rm -f "$RUN_RECORD"
  fi

  port=${MX_VIZ_PORT:-4890}
  case "$port" in ''|*[!0-9]*) die "MX_VIZ_PORT must be an integer from 1 through 65516" ;; esac
  [ "$port" -ge 1 ] && [ "$port" -le 65516 ] \
    || die "MX_VIZ_PORT must be an integer from 1 through 65516"
  token=$(node -e 'process.stdout.write(require("node:crypto").randomBytes(32).toString("hex"))') \
    || die "could not create dashboard token"
  ready_log=$(mktemp "${TMPDIR:-/tmp}/mx-viz-ready.XXXXXX") \
    || die "could not create readiness log"
  error_log=$(mktemp "${TMPDIR:-/tmp}/mx-viz-error.XXXXXX") \
    || { rm -f "$ready_log"; die "could not create error log"; }

  nohup node "$SERVER" --serve "$ROOT" "$canonical_home" "$canonical_state" "$RUN_RECORD" "$token" "$port" \
    >"$ready_log" 2>"$error_log" </dev/null &
  pid=$!
  line=
  counter=0
  while [ "$counter" -lt 200 ]; do
    line=$(sed -n '1p' "$ready_log" 2>/dev/null || true)
    [ -n "$line" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.025
    counter=$((counter + 1))
  done
  case "$line" in
    "READY "[0-9]*) port=${line#READY } ;;
    *)
      kill -TERM "$pid" 2>/dev/null || true
      if wait "$pid"; then rc=0; else rc=$?; fi
      line=$(sed -n '1,4p' "$error_log" 2>/dev/null || true)
      rm -f "$ready_log" "$error_log" "$RUN_RECORD"
      [ -n "$line" ] || line="server did not publish readiness (exit $rc)"
      die "$line"
      ;;
  esac

  identity=$(mx_pid_identity "$pid" 2>/dev/null || true)
  if [ -z "$identity" ]; then
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$ready_log" "$error_log" "$RUN_RECORD"
    die "server started but its process identity could not be verified"
  fi
  started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  temporary="$RUN_RECORD.tmp.$$"
  umask 077
  {
    printf 'version=1\n'
    printf 'home=%s\n' "$canonical_home"
    printf 'state=%s\n' "$canonical_state"
    printf 'port=%s\n' "$port"
    printf 'pid=%s\n' "$pid"
    printf 'pid_identity=%s\n' "$identity"
    printf 'token=%s\n' "$token"
    printf 'started_at=%s\n' "$started"
  } >"$temporary" || {
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$temporary" "$ready_log" "$error_log"
    die "could not write dashboard run record"
  }
  mv "$temporary" "$RUN_RECORD" || {
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$temporary" "$ready_log" "$error_log"
    die "could not publish dashboard run record"
  }
  rm -f "$ready_log" "$error_log"
  mx_lock_release "$SERVE_LOCK"
  trap - EXIT
  printf 'http://127.0.0.1:%s/\n' "$port"
}

status_dashboard() {
  local port pid started meta last_poll
  ensure_state
  load_pid_helpers
  if ! record_live "$RUN_RECORD"; then
    if [ -f "$RUN_RECORD" ] && [ ! -L "$RUN_RECORD" ]; then rm -f "$RUN_RECORD"; fi
    printf 'stopped\n'
    return 1
  fi
  port=$(record_value "$RUN_RECORD" port)
  pid=$(record_value "$RUN_RECORD" pid)
  started=$(record_value "$RUN_RECORD" started_at)
  last_poll=never
  if command -v curl >/dev/null 2>&1; then
    meta=$(curl -fsS "http://127.0.0.1:$port/api/meta" 2>/dev/null || true)
    if [ -n "$meta" ] && command -v jq >/dev/null 2>&1; then
      last_poll=$(printf '%s' "$meta" | jq -r '.last_poll_at // "never"' 2>/dev/null || printf never)
    fi
  fi
  printf 'running: http://127.0.0.1:%s/ pid=%s started=%s last_poll=%s\n' \
    "$port" "$pid" "$started" "$last_poll"
}

stop_dashboard() {
  local pid identity token actual counter
  ensure_state
  load_pid_helpers
  if [ ! -e "$RUN_RECORD" ] && [ ! -L "$RUN_RECORD" ]; then
    printf 'dashboard is not running\n'
    return 0
  fi
  [ -f "$RUN_RECORD" ] && [ ! -L "$RUN_RECORD" ] \
    || die "unsafe dashboard run record: $RUN_RECORD"
  pid=$(record_value "$RUN_RECORD" pid)
  identity=$(record_value "$RUN_RECORD" pid_identity)
  token=$(record_value "$RUN_RECORD" token)
  actual=$(mx_pid_identity "$pid" 2>/dev/null || true)
  if ! mx_pid_alive "$pid" || [ -z "$identity" ] || [ "$actual" != "$identity" ]; then
    remove_record_if_matches "$RUN_RECORD" "$pid" "$token"
    printf 'removed stale dashboard record\n'
    return 0
  fi
  kill -TERM "$pid" 2>/dev/null || die "could not stop dashboard process $pid"
  counter=0
  while [ "$counter" -lt 100 ] && kill -0 "$pid" 2>/dev/null; do
    sleep 0.05
    counter=$((counter + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    actual=$(mx_pid_identity "$pid" 2>/dev/null || true)
    [ "$actual" != "$identity" ] || die "dashboard process $pid did not stop after 5 seconds"
  fi
  remove_record_if_matches "$RUN_RECORD" "$pid" "$token"
  printf 'stopped dashboard\n'
}

case "${1:-}" in
  serve) [ "$#" -eq 1 ] || { usage >&2; exit 2; }; serve_dashboard ;;
  status) [ "$#" -eq 1 ] || { usage >&2; exit 2; }; status_dashboard ;;
  stop) [ "$#" -eq 1 ] || { usage >&2; exit 2; }; stop_dashboard ;;
  -h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
