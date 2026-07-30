#!/usr/bin/env bash
# Create, serve, inspect, and stop one-shot vplan review artifacts.
#
# Usage:
#   mx-vplan.sh new <file>
#   mx-vplan.sh review <file>
#   mx-vplan.sh comments <file>
#   mx-vplan.sh stop <file>
#   mx-vplan.sh --help
#
# `new` copies the vendored seed template and rewrites only its Mermaid asset
# path relative to the destination. `review` requires an artifact inside this
# Multplx root, starts the loopback-only Node server, records its PID identity
# under state/.vplan/, and prints the bound URL. The first attempted port is
# MX_VPLAN_PORT (default 4870), with 19 upward fallbacks. Confirm or the idle
# timeout (MX_VPLAN_IDLE_SECS, default 1800) removes the run record and exits.
# `comments` prints the persisted #vplan-comments array as formatted JSON.
# `stop` signals only a live process whose PID identity and review token still
# match the artifact's run record; stale records are cleaned without signaling.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
SERVER="$SCRIPT_DIR/mx-vplan-server.mjs"
ASSET_DIR="$ROOT/share/vplan"
TEMPLATE="$ASSET_DIR/template.html"
MANIFEST="$ASSET_DIR/manifest.json"

usage() {
  sed -n '2,/^set -u$/s/^# \{0,1\}//p' "$0"
}

die() {
  printf 'mx-vplan: %s\n' "$*" >&2
  exit 1
}

require_one_argument() {
  [ "$#" -eq 1 ] || die "expected exactly one artifact path (see --help)"
}

canonical_existing_file() {
  node -e '
    const fs = require("node:fs");
    const path = fs.realpathSync(process.argv[1]);
    if (!fs.statSync(path).isFile()) process.exit(2);
    process.stdout.write(path);
  ' "$1" 2>/dev/null
}

canonical_new_file() {
  local input=$1 directory base
  directory=$(dirname "$input")
  base=$(basename "$input")
  mkdir -p "$directory" || return 1
  directory=$(cd "$directory" 2>/dev/null && pwd -P) || return 1
  printf '%s/%s\n' "$directory" "$base"
}

assert_under_root() {
  node -e '
    const path = require("node:path");
    const relative = path.relative(process.argv[1], process.argv[2]);
    if (relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))) {
      process.exit(0);
    }
    process.exit(1);
  ' "$ROOT" "$1" || die "artifact must be inside the Multplx root: $1"
}

artifact_hash() {
  node -e 'process.stdout.write(require("node:crypto").createHash("sha256").update(process.argv[1]).digest("hex"))' "$1"
}

state_directory() {
  local home
  home=${MX_HOME:-$ROOT}
  printf '%s/.vplan\n' "${MX_STATE_OVERRIDE:-$home/state}"
}

record_value() {
  local record=$1 key=$2
  awk -v key="$key" '
    index($0, key "=") == 1 {
      print substr($0, length(key) + 2)
      exit
    }
  ' "$record" 2>/dev/null
}

remove_record_if_matches() {
  local record=$1 expected_pid=$2 expected_token=$3 current_pid current_token
  [ -f "$record" ] || return 0
  current_pid=$(record_value "$record" pid)
  current_token=$(record_value "$record" token)
  if [ "$current_pid" = "$expected_pid" ] && [ "$current_token" = "$expected_token" ]; then
    rm -f "$record"
  fi
}

load_pid_helpers() {
  # shellcheck source=bin/mx-wake-lib.sh disable=SC1091
  . "$SCRIPT_DIR/mx-wake-lib.sh"
}

hash_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    return 1
  fi
}

self_check() {
  local file expected actual
  for file in "$SERVER" "$TEMPLATE" "$MANIFEST" \
    "$ASSET_DIR/sdk.js" "$ASSET_DIR/sdk.css" "$ASSET_DIR/mermaid.min.js"; do
    [ -r "$file" ] || return 1
  done
  "$0" --help >/dev/null 2>&1 || return 1
  expected=$(sed -n 's/^[[:space:]]*"sha256":[[:space:]]*"\([a-f0-9][a-f0-9]*\)".*/\1/p' "$MANIFEST" | head -n 1)
  [ -n "$expected" ] || return 1
  actual=$(hash_file "$ASSET_DIR/mermaid.min.js") || return 1
  [ "$actual" = "$expected" ] || return 1
  grep -F 'mermaid.min.js' "$TEMPLATE" >/dev/null 2>&1 || return 1
  node --check "$SERVER" >/dev/null 2>&1 || return 1
}

new_artifact() {
  local destination asset_base
  require_one_argument "$@"
  destination=$(canonical_new_file "$1") || die "could not resolve destination: $1"
  assert_under_root "$destination"
  [ ! -e "$destination" ] || die "refusing to overwrite existing artifact: $destination"
  asset_base=$(node -e '
    const path = require("node:path");
    let relative = path.relative(path.dirname(process.argv[1]), process.argv[2]);
    if (!relative.startsWith(".")) relative = `./${relative}`;
    process.stdout.write(relative.split(path.sep).join("/"));
  ' "$destination" "$ASSET_DIR") || die "could not compute the relative asset path"
  node - "$TEMPLATE" "$destination" "$asset_base" <<'NODE' \
    || die "could not create artifact: $destination"
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const [template, destination, assetBase] = process.argv.slice(2);
const source = fs.readFileSync(template, "utf8");
const rendered = source.replaceAll("./mermaid.min.js", `${assetBase}/mermaid.min.js`);
const temporary = path.join(
  path.dirname(destination),
  `.${path.basename(destination)}.vplan-new-${process.pid}-${crypto.randomBytes(4).toString("hex")}.tmp`,
);
try {
  fs.writeFileSync(temporary, rendered, { encoding: "utf8", flag: "wx", mode: 0o644 });
  fs.renameSync(temporary, destination);
} catch (error) {
  try { fs.unlinkSync(temporary); } catch {}
  throw error;
}
NODE
  printf '%s\n' "$destination"
}

comments_artifact() {
  local artifact
  require_one_argument "$@"
  artifact=$(canonical_existing_file "$1") || die "artifact is not a readable file: $1"
  assert_under_root "$artifact"
  node "$SERVER" --comments "$artifact"
}

review_artifact() {
  local artifact state hash record pid identity current_identity token port started_at
  local ready_log error_log line counter rc temporary_record
  require_one_argument "$@"
  artifact=$(canonical_existing_file "$1") || die "artifact is not a readable file: $1"
  assert_under_root "$artifact"
  case "$artifact" in
    *'
'*) die "artifact paths may not contain newlines" ;;
  esac
  port=${MX_VPLAN_PORT:-4870}
  case "$port" in
    ''|*[!0-9]*) die "MX_VPLAN_PORT must be an integer from 1 through 65516" ;;
  esac
  [ "$port" -ge 1 ] && [ "$port" -le 65516 ] \
    || die "MX_VPLAN_PORT must be an integer from 1 through 65516"

  state=$(state_directory)
  mkdir -p "$state" || die "could not create vplan state directory: $state"
  hash=$(artifact_hash "$artifact") || die "could not derive artifact identity"
  record="$state/$hash.run"
  load_pid_helpers

  if [ -f "$record" ]; then
    pid=$(record_value "$record" pid)
    identity=$(record_value "$record" pid_identity)
    token=$(record_value "$record" token)
    if mx_pid_alive "$pid" && [ -n "$identity" ] && [ -n "$token" ]; then
      current_identity=$(mx_pid_identity "$pid" 2>/dev/null || true)
      if [ "$current_identity" = "$identity" ]; then
        port=$(record_value "$record" port)
        printf 'http://127.0.0.1:%s/\n' "$port"
        return 0
      fi
    fi
    remove_record_if_matches "$record" "$pid" "$token"
  fi

  token=$(node -e 'process.stdout.write(require("node:crypto").randomBytes(32).toString("hex"))') \
    || die "could not create review token"
  ready_log=$(mktemp "${TMPDIR:-/tmp}/mx-vplan-ready.XXXXXX") \
    || die "could not create readiness log"
  error_log=$(mktemp "${TMPDIR:-/tmp}/mx-vplan-error.XXXXXX") \
    || { rm -f "$ready_log"; die "could not create error log"; }

  nohup node "$SERVER" --serve "$artifact" "$ROOT" "$record" "$token" "$port" \
    >"$ready_log" 2>"$error_log" </dev/null &
  pid=$!
  line=
  counter=0
  while [ "$counter" -lt 200 ]; do
    line=$(sed -n '1p' "$ready_log" 2>/dev/null || true)
    [ -n "$line" ] && break
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.025
    counter=$((counter + 1))
  done

  case "$line" in
    "READY "[0-9]*)
      port=${line#READY }
      ;;
    *)
      if kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || true
      fi
      if wait "$pid"; then rc=0; else rc=$?; fi
      line=$(sed -n '1,4p' "$error_log" 2>/dev/null || true)
      rm -f "$ready_log" "$error_log" "$record"
      [ -n "$line" ] || line="server did not publish readiness (exit $rc)"
      die "$line"
      ;;
  esac

  identity=$(mx_pid_identity "$pid" 2>/dev/null || true)
  if [ -z "$identity" ]; then
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$ready_log" "$error_log" "$record"
    die "server started but its process identity could not be verified"
  fi
  started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  temporary_record="$record.tmp.$$"
  umask 077
  {
    printf 'version=1\n'
    printf 'artifact=%s\n' "$artifact"
    printf 'port=%s\n' "$port"
    printf 'pid=%s\n' "$pid"
    printf 'pid_identity=%s\n' "$identity"
    printf 'token=%s\n' "$token"
    printf 'started_at=%s\n' "$started_at"
  } >"$temporary_record" || {
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$temporary_record" "$ready_log" "$error_log"
    die "could not write run record"
  }
  mv "$temporary_record" "$record" || {
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$temporary_record" "$ready_log" "$error_log"
    die "could not publish run record"
  }
  rm -f "$ready_log" "$error_log"
  printf 'http://127.0.0.1:%s/\n' "$port"
}

stop_artifact() {
  local artifact state hash record pid identity token current_identity counter
  require_one_argument "$@"
  artifact=$(canonical_existing_file "$1") || die "artifact is not a readable file: $1"
  assert_under_root "$artifact"
  state=$(state_directory)
  hash=$(artifact_hash "$artifact") || die "could not derive artifact identity"
  record="$state/$hash.run"
  if [ ! -f "$record" ]; then
    printf 'no active review for %s\n' "$artifact"
    return 0
  fi
  load_pid_helpers
  pid=$(record_value "$record" pid)
  identity=$(record_value "$record" pid_identity)
  token=$(record_value "$record" token)
  current_identity=$(mx_pid_identity "$pid" 2>/dev/null || true)
  if ! mx_pid_alive "$pid" || [ -z "$identity" ] || [ "$current_identity" != "$identity" ]; then
    remove_record_if_matches "$record" "$pid" "$token"
    printf 'removed stale review record for %s\n' "$artifact"
    return 0
  fi
  kill -TERM "$pid" 2>/dev/null || die "could not stop review process $pid"
  counter=0
  while [ "$counter" -lt 100 ] && kill -0 "$pid" 2>/dev/null; do
    sleep 0.05
    counter=$((counter + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    current_identity=$(mx_pid_identity "$pid" 2>/dev/null || true)
    [ "$current_identity" != "$identity" ] \
      || die "review process $pid did not stop after 5 seconds"
  fi
  remove_record_if_matches "$record" "$pid" "$token"
  printf 'stopped review for %s\n' "$artifact"
}

case "${1:-}" in
  -h|--help)
    usage
    ;;
  --self-check)
    [ "$#" -eq 1 ] || die "--self-check accepts no arguments"
    self_check || die "bundled vplan self-check failed"
    ;;
  new)
    shift
    new_artifact "$@"
    ;;
  review)
    shift
    review_artifact "$@"
    ;;
  comments)
    shift
    comments_artifact "$@"
    ;;
  stop)
    shift
    stop_artifact "$@"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
