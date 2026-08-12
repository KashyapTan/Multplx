#!/usr/bin/env bash
# mx-afk-return.sh - deterministic away-mode return catch-up gate.
#
# Usage:
#   mx-afk-return.sh          Stop away mode, drain catch-up, and open/check gate.
#   mx-afk-return.sh begin    Same as the default command.
#   mx-afk-return.sh check    Re-drain and close the gate only after blockers resolve.
#   mx-afk-return.sh guard    Read-only refusal while away or catch-up is pending.
#
# `blocked:` is the actor protocol's broker-actionable verb. A live task's
# open blocked event must be remediated and closed with `resolved [key=...]`, or
# explicitly reclassified in the status stream with a durable reason, before an
# ordinary maintainer request may proceed. `needs-decision:` belongs to the
# configured approval authority and is deliberately not part of this blocker
# gate; normal reporting routes it through the AGENTS.md section 7 contract.
#
# The durable state/.afk-return-catchup file is written BEFORE daemon shutdown,
# so a crash between stopping, draining, and blocker handling fails closed. It
# retains the drained wake, buffered-escalation, and wedge-marker evidence until
# every live open blocker is closed and `check` succeeds. Repeated begin/check
# calls are idempotent. `guard` never mutates state and is suitable for ordinary
# read entrypoints such as mx-status-snapshot.sh.
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
    exec "$mx_supervision_adapter_bin" supervision mx-afk-return.sh "$@"
  fi
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
GATE="$STATE/.afk-return-catchup"
LOCK="$STATE/.afk-return-catchup.lock"

usage() {
  sed -n '2,7p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

clean_field() {
  LC_ALL=C tr '\t\r\n' '   '
}

append_evidence() {  # <kind> <text> <file>
  local kind=$1 text=$2 file=$3 clean record
  [ -n "$text" ] || return 0
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    clean=$(printf '%s' "$line" | clean_field)
    record=$(printf 'evidence\t%s\t%s' "$kind" "$clean")
    grep -Fqx "$record" "$file" 2>/dev/null || printf '%s\n' "$record" >> "$file"
  done <<EOF
$text
EOF
}

preserve_evidence() {  # <destination>
  local destination=$1
  [ -f "$GATE" ] || return 0
  grep '^evidence'"$(printf '\t')" "$GATE" >> "$destination" 2>/dev/null || true
}

scan_open_blockers() {  # -> tab-separated blocker rows
  local meta id status key verb summary clean_summary
  for meta in "$STATE"/*.meta; do
    [ -f "$meta" ] || continue
    id=$(basename "$meta")
    id=${id%.meta}
    status="$STATE/$id.status"
    [ -f "$status" ] || continue
    while IFS="$(printf '\t')" read -r key verb summary; do
      [ "$verb" = blocked ] || continue
      clean_summary=$(printf '%s' "$summary" | clean_field)
      printf 'blocker\t%s\t%s\t%s\n' "$id" "$key" "$clean_summary"
    done <<EOF
$(status_open_decisions "$status")
EOF
  done
}

write_pending_seed() {  # Fail-closed marker before any lifecycle mutation.
  local pending started
  mkdir -p "$STATE" || return 1
  started=$(awk -F '\t' '$1 == "started" { print $2; exit }' "$GATE" 2>/dev/null || true)
  [ -n "$started" ] || started=$(date +%s)
  pending=$(mktemp "$STATE/.afk-return-catchup.pending.XXXXXX") || return 1
  {
    printf 'schema\tmx-afk-return.v1\n'
    printf 'started\t%s\n' "$started"
    printf 'phase\tstopping-and-draining\n'
    preserve_evidence /dev/stdout
  } > "$pending" || { rm -f "$pending"; return 1; }
  mv "$pending" "$GATE"
}

write_gate() {  # <evidence-file> <blockers-file>
  local evidence=$1 blockers=$2 pending started
  pending=$(mktemp "$STATE/.afk-return-catchup.pending.XXXXXX") || return 1
  started=$(awk -F '\t' '$1 == "started" { print $2; exit }' "$GATE" 2>/dev/null || true)
  [ -n "$started" ] || started=$(date +%s)
  {
    printf 'schema\tmx-afk-return.v1\n'
    printf 'started\t%s\n' "$started"
    printf 'phase\tblocked\n'
    cat "$evidence" 2>/dev/null || true
    cat "$blockers" 2>/dev/null || true
  } > "$pending" || { rm -f "$pending"; return 1; }
  mv "$pending" "$GATE"
}

print_evidence() {  # <file>
  local file=$1 kind text
  while IFS="$(printf '\t')" read -r tag kind text; do
    [ "$tag" = evidence ] || continue
    printf 'catch-up %s: %s\n' "$kind" "$text"
  done < "$file"
}

print_blockers() {  # <file>
  local file=$1 tag id key summary
  while IFS="$(printf '\t')" read -r tag id key summary; do
    [ "$tag" = blocker ] || continue
    printf 'broker-actionable blocker: %s [key=%s] %s\n' "$id" "$key" "$summary"
  done < "$file"
}

clear_delivery_artifacts() {
  rm -f \
    "$STATE/.subsuper-escalations" \
    "$STATE/.subsuper-escalations.since" \
    "$STATE/.subsuper-inject-wedged"
}

return_guard() {
  if [ -e "$STATE/.afk" ]; then
    printf 'mx-afk-return: away mode is still active; run bin/mx-afk-return.sh before ordinary maintainer work\n' >&2
    return 3
  fi
  if [ -e "$GATE" ]; then
    printf 'mx-afk-return: return catch-up is pending; remediate or durably reclassify every listed blocker, then run bin/mx-afk-return.sh check\n' >&2
    print_blockers "$GATE" >&2
    return 3
  fi
  return 0
}

return_reconcile() {
  local evidence blockers drained wedge escalations lifecycle_ok=1
  evidence=$(mktemp "$STATE/.afk-return-evidence.XXXXXX") || return 1
  blockers=$(mktemp "$STATE/.afk-return-blockers.XXXXXX") || { rm -f "$evidence"; return 1; }
  preserve_evidence "$evidence"

  if [ -e "$STATE/.afk" ] || [ -e "$STATE/.afk-daemon-terminal" ]; then
    if ! "$SCRIPT_DIR/mx-afk-launch.sh" stop; then
      lifecycle_ok=0
      append_evidence lifecycle 'away-mode shutdown failed; lifecycle state preserved for retry' "$evidence"
    fi
  fi

  drained=$("$SCRIPT_DIR/mx-wake-drain.sh") || {
    append_evidence lifecycle 'durable wake drain failed; retry catch-up before ordinary work' "$evidence"
    lifecycle_ok=0
    drained=""
  }
  append_evidence wake "$drained" "$evidence"

  if [ -s "$STATE/.subsuper-inject-wedged" ]; then
    wedge=$(head -1 "$STATE/.subsuper-inject-wedged" 2>/dev/null || true)
    append_evidence wedge "$wedge" "$evidence"
  fi
  if [ -s "$STATE/.subsuper-escalations" ]; then
    escalations=$(cat "$STATE/.subsuper-escalations" 2>/dev/null || true)
    append_evidence escalation "$escalations" "$evidence"
  fi

  scan_open_blockers > "$blockers"
  if [ "$lifecycle_ok" -ne 1 ] || [ -s "$blockers" ]; then
    write_gate "$evidence" "$blockers" || { rm -f "$evidence" "$blockers"; return 1; }
    printf 'mx-afk-return: catch-up must finish before the maintainer request\n' >&2
    print_evidence "$GATE" >&2
    print_blockers "$GATE" >&2
    printf 'mx-afk-return: handle each blocker now, or close it with resolved [key=...] and append a durable reclassification reason, then run bin/mx-afk-return.sh check\n' >&2
    rm -f "$evidence" "$blockers"
    return 3
  fi

  print_evidence "$evidence"
  rm -f "$GATE"
  clear_delivery_artifacts
  rm -f "$evidence" "$blockers"
  printf 'mx-afk-return: catch-up clear; ordinary maintainer work may proceed\n'
  return 0
}

main() {
  local mode=${1:-begin} rc
  case "$mode" in
    begin|check) ;;
    guard) return_guard; return ;;
    -h|--help|help) usage; return 0 ;;
    *) usage >&2; return 2 ;;
  esac

  # The mutating begin/check paths need locks and the keyed status fold.
  # `guard` returned above without sourcing mx-wake-lib.sh, whose initialization
  # creates the state directory, so the advertised read-only guard is literal.
  # shellcheck source=bin/mx-wake-lib.sh
  . "$SCRIPT_DIR/mx-wake-lib.sh"
  # shellcheck source=bin/mx-classify-lib.sh
  . "$SCRIPT_DIR/mx-classify-lib.sh"

  mkdir -p "$STATE" || return 1
  mx_lock_acquire_wait "$LOCK"
  trap 'mx_lock_release "$LOCK"' EXIT
  write_pending_seed || { mx_lock_release "$LOCK"; trap - EXIT; return 1; }
  return_reconcile
  rc=$?
  mx_lock_release "$LOCK"
  trap - EXIT
  return "$rc"
}

main "$@"
