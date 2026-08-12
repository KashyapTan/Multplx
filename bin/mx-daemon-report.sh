#!/usr/bin/env bash
# mx-daemon-report.sh - optional helper to append a correlated parent report.
#
# A daemon answering a marked from-broker request must report on the
# parent status channel with the request's corr=<id> token. This helper makes
# that easy, but correctness must not depend on using it: a plain echo of a
# status line that includes the same corr token is equally valid
# (bin/mx-pending-reply-lib.sh).
#
# Usage:
#   mx-daemon-report.sh <status-file> <verb> <corr_id> <note...>
#   mx-daemon-report.sh --doc <status-file> <verb> <corr_id> <doc-path> <note...>
#
# Examples:
#   mx-daemon-report.sh "$STATUS" done abcdef0123456789 "audit clean"
#   mx-daemon-report.sh --doc "$STATUS" done abcdef0123456789 data/x/report.md "see report"
#
# The status file must be the absolute parent route from the daemon charter
# (state/<id>.status under the PARENT home), never a path relative to this
# daemon home. Writing under the wrong home is detected as supporting
# evidence by the parent pending-reply guard and does not acknowledge the
# request.
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
implementation=$(mx_lifecycle_implementation) || exit $?
if [ "$implementation" = rust ]; then
  MX_RUST_SOURCE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" daemon-report "$@"
fi
# shellcheck source=bin/mx-pending-reply-lib.sh
. "$SCRIPT_DIR/mx-pending-reply-lib.sh"

usage() {
  cat <<'EOF' >&2
Usage:
  mx-daemon-report.sh <status-file> <verb> <corr_id> <note...>
  mx-daemon-report.sh --doc <status-file> <verb> <corr_id> <doc-path> <note...>
EOF
  exit 2
}

DOC_MODE=0
if [ "${1:-}" = "--doc" ]; then
  DOC_MODE=1
  shift
fi

[ $# -ge 4 ] || usage
STATUS_FILE=$1
VERB=$2
CORR=$3
shift 3

case "$CORR" in
  corr=*) CORR=${CORR#corr=} ;;
esac
case "$CORR" in
  [a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9][a-fA-F0-9]) ;;
  *)
    echo "error: corr_id must be 16 hex characters (got '$CORR')" >&2
    exit 1
    ;;
esac

case "$STATUS_FILE" in
  '') usage ;;
esac
mkdir -p "$(dirname "$STATUS_FILE")" 2>/dev/null || true
if [ ! -d "$(dirname "$STATUS_FILE")" ]; then
  echo "error: cannot create parent directory for status file '$STATUS_FILE'" >&2
  exit 1
fi

token=$(mx_pending_reply_corr_token "$CORR")
if [ "$DOC_MODE" = 1 ]; then
  [ $# -ge 1 ] || usage
  DOC_PATH=$1
  shift
  NOTE=$*
  if [ -n "$NOTE" ]; then
    printf '%s [%s]: %s (%s via-helper)\n' "$VERB" "$token" "$NOTE" "$DOC_PATH" >> "$STATUS_FILE"
  else
    printf '%s [%s]: %s (via-helper)\n' "$VERB" "$token" "$DOC_PATH" >> "$STATUS_FILE"
  fi
else
  NOTE=$*
  if [ -n "$NOTE" ]; then
    printf '%s [%s]: %s (via-helper)\n' "$VERB" "$token" "$NOTE" >> "$STATUS_FILE"
  else
    printf '%s [%s]: (via-helper)\n' "$VERB" "$token" >> "$STATUS_FILE"
  fi
fi
