#!/usr/bin/env bash
# Push declared inherited local material to live daemon homes.
# Usage: mx-config-push.sh [--help]
#
# Mid-session convergence for inherited local material such as
# config/actor-dispatch.json edits or data/maintainer-shared.md updates. This
# discovers live daemon homes from state/*.meta, backfills
# home= from data/daemons.md for older meta records, and reuses the same
# propagation machinery as bootstrap, but deliberately does not
# fast-forward tracked files.
# After a successful per-home propagation that changes any allowlisted config/*
# item, writes a generation-specific literal-content reread instruction and
# sends its pointer to that live daemon via mx-config-inherit-lib.sh
# (mx_config_send_reread_nudge).
# Unchanged config and data/maintainer-shared.md-only updates send no reread
# message unless a previous send failure is pending for that home.
# Warnings-only skips exit 0; real propagation or reread-send errors exit non-zero.
set -u

usage() {
  cat <<'EOF'
Usage: mx-config-push.sh [--help]

Push the primary Multplx home's declared inherited local material into each
live daemon home.

This is local-material-only:
  - does not fast-forward tracked files
  - after successful config/* changes, writes a generation-specific
    literal-content reread instruction and sends its pointer to that live daemon
    (no message when config is unchanged unless a previous send failure is pending)
  - reports each live home and each inheritable item as pushed, unchanged,
    skipped, or error
  - exits non-zero for real propagation errors or reread-send failures

Live homes come from state/*.meta records with kind=daemon.
data/daemons.md is only a fallback for missing home= fields in older or
incomplete meta records.

Environment overrides follow the rest of broker:
  MX_HOME            active Multplx home
  MX_ROOT_OVERRIDE  Multplx repo root
  MX_STATE_OVERRIDE state dir
  MX_DATA_OVERRIDE  data dir
  MX_CONFIG_OVERRIDE config dir
EOF
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
  "")
    ;;
  *)
    echo "usage: mx-config-push.sh [--help]" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
CONFIG="${MX_CONFIG_OVERRIDE:-$MX_HOME/config}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
DAEMONS_MD="$DATA/daemons.md"

"$SCRIPT_DIR/mx-guard.sh" || true

# shellcheck source=bin/mx-ff-lib.sh
. "$SCRIPT_DIR/mx-ff-lib.sh"
# shellcheck source=bin/mx-wake-lib.sh
. "$SCRIPT_DIR/mx-wake-lib.sh"
# shellcheck source=bin/mx-config-inherit-lib.sh
. "$SCRIPT_DIR/mx-config-inherit-lib.sh"

print_item_report() {
  local report=$1 item status reason
  while IFS=$'\t' read -r item status reason; do
    [ -n "$item" ] || continue
    if [ -n "$reason" ]; then
      printf '  %s: %s - %s\n' "$item" "$status" "$reason"
    else
      printf '  %s: %s\n' "$item" "$status"
    fi
  done < "$report"
}

records=$(mktemp "${TMPDIR:-/tmp}/mx-config-push-records.XXXXXX" 2>/dev/null) || exit 1
reports=""
# shellcheck disable=SC2317,SC2329 # Invoked by trap handlers below.
cleanup() {
  local report_file
  rm -f "$records"
  for report_file in $reports; do
    rm -f "$report_file"
  done
}
trap cleanup EXIT

live_daemon_meta_records "$STATE" "$DAEMONS_MD" > "$records"
if [ ! -s "$records" ]; then
  echo "config-push: no live daemon homes found"
  exit 0
fi

echo "config-push: $MX_HOME -> live daemon homes"

seen_homes=""
errors=0
while IFS='|' read -r id home _window meta; do
  [ -n "$id" ] || continue
  if [ -z "$home" ]; then
    printf 'daemon %s: skipped - no home= in %s and no registry home\n' "$id" "$meta"
    continue
  fi
  if ! validate_daemon_home "$id" "$home"; then
    printf 'daemon %s (%s): skipped - unsafe home: %s\n' "$id" "$home" "$VALIDATION_ERROR"
    continue
  fi
  home_real="$VALIDATED_HOME"
  case " $seen_homes " in
    *" $home_real "*)
      printf 'daemon %s (%s): skipped - already processed for another live meta\n' "$id" "$home_real"
      continue
      ;;
  esac
  seen_homes="$seen_homes $home_real"

  printf 'daemon %s (%s):\n' "$id" "$home_real"
  dirty=$(dirty_status "$home_real" yes || true)
  if [ -n "$dirty" ]; then
    echo "  home: dirty working tree - local-material push continuing"
  fi

  mkdir -p "$home_real/state" || {
    echo "  config-reread: error - could not create state directory"
    errors=1
    continue
  }
  home_lock=$(mx_config_inherit_lock_path "$home_real") || {
    echo "  config-reread: error - could not resolve per-home lock"
    errors=1
    continue
  }
  mx_lock_acquire_wait "$home_lock" || {
    echo "  config-reread: error - could not acquire per-home lock"
    errors=1
    continue
  }
  if mx_config_reread_retry_queue_is_full "$MX_HOME" "$id"; then
    mx_config_reread_retry_pending "$id" "$home_real" || true
    if mx_config_reread_retry_queue_is_full "$MX_HOME" "$id"; then
      echo "  config-reread: error - retry instruction queue is full"
      errors=1
      mx_lock_release "$home_lock" || true
      continue
    fi
  fi

  report=$(mktemp "${TMPDIR:-/tmp}/mx-config-push-report.XXXXXX" 2>/dev/null) || {
    echo "  home: error - could not create report file"
    errors=1
    mx_lock_release "$home_lock" || true
    continue
  }
  reports="$reports $report"
  if MX_CONFIG_INHERIT_REPORT="$report" propagate_daemon_inheritance "$MX_HOME" "$home_real" "$CONFIG" "$DATA"; then
    :
  else
    errors=1
  fi
  print_item_report "$report"
  reread_pending=0
  if mx_config_reread_has_pending "$home_real" || mx_config_reread_has_staged "$MX_HOME" "$id"; then
    reread_pending=1
  fi
  if reread_out=$(MX_HOME="$MX_HOME" MX_ROOT_OVERRIDE="$MX_ROOT" \
    MX_STATE_OVERRIDE="$STATE" \
    mx_config_send_reread_nudge "$id" "$home_real" "$report" 2>&1); then
    if [ -n "$(mx_config_reread_changed_items "$report")" ] || [ "$reread_pending" -eq 1 ]; then
      printf '  config-reread: sent\n'
    fi
    [ -z "$reread_out" ] || printf '%s\n' "$reread_out"
  else
    errors=1
    if [ -n "$reread_out" ]; then
      printf '%s\n' "$reread_out"
    else
      printf '  config-reread: send failed\n'
    fi
  fi
  mx_lock_release "$home_lock" || true
done < "$records"

[ "$errors" -eq 0 ] || exit 1
exit 0
