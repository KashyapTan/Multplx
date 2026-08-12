#!/usr/bin/env bash
# Run one bounded foreground watcher checkpoint for harnesses that should not
# rely on background-task completion to wake the model.
set -u

# Portion 08 Rust-default adapter. Keep the body below as the explicit bounded
# rollback path and as the sourced-function ABI where this file is sourceable.
MX_SUPERVISION_ADAPTER_DIR=${BASH_SOURCE[0]%/*}
[ "$MX_SUPERVISION_ADAPTER_DIR" != "${BASH_SOURCE[0]}" ] || MX_SUPERVISION_ADAPTER_DIR=.
MX_SUPERVISION_ADAPTER_DIR="$(CDPATH='' cd -- "$MX_SUPERVISION_ADAPTER_DIR" 2>/dev/null && pwd)" || exit 1
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  # shellcheck source=bin/mx-rust-runtime.sh
  . "$MX_SUPERVISION_ADAPTER_DIR/mx-rust-runtime.sh"
  mx_supervision_adapter_implementation=$(mx_supervision_implementation) || exit $?
  if [ "$mx_supervision_adapter_implementation" = rust ]; then
    MX_RUST_SOURCE_ROOT="$(cd "$MX_SUPERVISION_ADAPTER_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
    mx_supervision_adapter_bin=$(mx_rust_runtime_bin) || exit $?
    exec "$mx_supervision_adapter_bin" supervision mx-watch-checkpoint.sh "$@"
  fi
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SECONDS_ARG=${MX_CODEX_WATCH_CHECKPOINT:-180}

usage() {
  cat <<'EOF'
Usage: mx-watch-checkpoint.sh [--seconds <n>]

Run bin/mx-watch.sh in the foreground for a bounded checkpoint.
On an actionable watcher wake, pass through the watcher output and exit 0.
On a quiet checkpoint, print "checkpoint: no actionable wake within <n>s" and exit 124.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --seconds)
      [ "$#" -gt 1 ] || { echo "error: --seconds requires a value" >&2; exit 2; }
      SECONDS_ARG=$2
      shift 2
      ;;
    --seconds=*)
      SECONDS_ARG=${1#--seconds=}
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$SECONDS_ARG" in
  ''|*[!0-9]*) echo "error: --seconds must be a positive integer" >&2; exit 2 ;;
  0) echo "error: --seconds must be greater than zero" >&2; exit 2 ;;
esac

OUT=$(mktemp "${TMPDIR:-/tmp}/mx-watch-checkpoint.out.XXXXXX") || exit 1
ERR=$(mktemp "${TMPDIR:-/tmp}/mx-watch-checkpoint.err.XXXXXX") || {
  rm -f "$OUT"
  exit 1
}
trap 'rm -f "$OUT" "$ERR"' EXIT

reconcile_timed_out_watch_lock() {
  local state lock pid reclaimable i
  state=${MX_STATE_OVERRIDE:-${MX_HOME:-$(cd "$SCRIPT_DIR/.." && pwd)}/state}
  lock="$state/.watch.lock"
  i=0

  while [ "$i" -lt 60 ]; do
    if [ ! -e "$lock" ] && [ ! -L "$lock" ]; then
      return 0
    fi

    pid=$(cat "$lock/pid" 2>/dev/null || true)
    reclaimable=0
    case "$pid" in
      ''|*[!0-9]*) reclaimable=1 ;;
      *) kill -0 "$pid" 2>/dev/null || reclaimable=1 ;;
    esac
    if [ "$reclaimable" -eq 1 ]; then
      (
        MX_STATE_OVERRIDE="$state"
        # shellcheck source=bin/mx-wake-lib.sh
        . "$SCRIPT_DIR/mx-wake-lib.sh"
        if mx_lock_try_acquire "$lock"; then
          mx_lock_release "$lock"
        fi
      )
    fi

    if [ ! -e "$lock" ] && [ ! -L "$lock" ]; then
      return 0
    fi
    sleep 0.05
    i=$((i + 1))
  done

  pid=$(cat "$lock/pid" 2>/dev/null || true)
  echo "checkpoint: timed-out watcher lock did not clean up (pid=${pid:-unknown})" >&2
  return 1
}

run_with_perl_timeout() {
  perl -e '
    my $seconds = shift;
    my $pid = fork;
    die "fork failed\n" unless defined $pid;
    if (!$pid) {
      setpgrp(0, 0);
      exec @ARGV;
      die "exec failed: $!\n";
    }
    local $SIG{ALRM} = sub {
      kill "TERM", -$pid;
      select undef, undef, undef, 0.2;
      kill "KILL", -$pid;
      exit 124;
    };
    alarm $seconds;
    waitpid $pid, 0;
    exit($? >> 8);
  ' "$SECONDS_ARG" "$SCRIPT_DIR/mx-watch.sh"
}

set +e
if command -v timeout >/dev/null 2>&1; then
  timeout "$SECONDS_ARG" "$SCRIPT_DIR/mx-watch.sh" >"$OUT" 2>"$ERR"
  RC=$?
elif command -v gtimeout >/dev/null 2>&1; then
  gtimeout "$SECONDS_ARG" "$SCRIPT_DIR/mx-watch.sh" >"$OUT" 2>"$ERR"
  RC=$?
else
  run_with_perl_timeout >"$OUT" 2>"$ERR"
  RC=$?
fi
set -e

if grep -E '^(signal:|stale:|check:|heartbeat($|:))' "$OUT" >/dev/null 2>&1; then
  cat "$OUT"
  [ ! -s "$ERR" ] || cat "$ERR" >&2
  exit 0
fi

if grep -E '^watcher: already running' "$OUT" "$ERR" >/dev/null 2>&1; then
  [ ! -s "$OUT" ] || cat "$OUT"
  [ ! -s "$ERR" ] || cat "$ERR" >&2
  echo "checkpoint: watcher is already running outside this foreground checkpoint" >&2
  exit 1
fi

if [ "$RC" -eq 124 ]; then
  reconcile_timed_out_watch_lock || exit 1
  printf 'checkpoint: no actionable wake within %ss\n' "$SECONDS_ARG"
  exit 124
fi

[ ! -s "$OUT" ] || cat "$OUT"
[ ! -s "$ERR" ] || cat "$ERR" >&2
exit "$RC"
