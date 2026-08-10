#!/usr/bin/env bash
# Request, decide, consume, inspect, and audit exact maintainer exceptions.
#
# Usage:
#   mx-maintainer-override.sh registry [--json]
#   mx-maintainer-override.sh request --boundary <id> --task <id> --project <slug>
#     --operation <literal operation> --target <identity>
#     --expected-state <sha256> --consequence <one line> [--ttl <seconds>]
#   mx-maintainer-override.sh grant <request-id> --maintainer-words <literal words>
#   mx-maintainer-override.sh deny <request-id> --maintainer-words <literal words>
#   mx-maintainer-override.sh consume <request-id> --boundary <id> --task <id>
#     --project <slug> --operation <literal operation> --target <identity>
#     --expected-state <sha256>
#   mx-maintainer-override.sh result <request-id> --outcome succeeded|failed --detail <text>
#   mx-maintainer-override.sh inspect <request-id>
#   mx-maintainer-override.sh audit [--json]
#   mx-maintainer-override.sh digest <literal text>
#   mx-maintainer-override.sh argv [literal argv...]
#   mx-maintainer-override.sh handoff <request-id>
#
# grant and deny work only in the lock-owning primary broker session.
# Workers and other processes may request an exception, but cannot grant one.
# `handoff` prints an already-consumed operation for a capability that must be
# performed by the maintainer or a credentialed service.
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$SCRIPT_DIR/mx-maintainer-override-lib.sh"
trap mx_override_lock_release EXIT

usage() {
  awk 'NR == 1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "$0"
}

die_usage() {
  mx_override_error "$*"
  usage >&2
  exit 2
}

require_value() {
  [ "$#" -ge 2 ] && [ -n "$2" ] || die_usage "$1 requires a non-empty value"
}

command_registry() {
  case "${1:-}" in
    '') mx_override_registry ;;
    --json)
      mx_override_registry | jq -Rsc '
        split("\n") | map(select(length > 0) | split("\t")) |
        map({boundary_id:.[0],class:.[1],alternate:.[2]})
      '
      ;;
    *) die_usage "registry accepts only --json" ;;
  esac
}

command_request() {
  local boundary='' task='' project='' operation='' target='' state_digest='' consequence='' ttl=$MX_OVERRIDE_DEFAULT_TTL
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --boundary) require_value "$1" "${2:-}"; boundary=$2; shift 2 ;;
      --task) require_value "$1" "${2:-}"; task=$2; shift 2 ;;
      --project) require_value "$1" "${2:-}"; project=$2; shift 2 ;;
      --operation) require_value "$1" "${2:-}"; operation=$2; shift 2 ;;
      --target) require_value "$1" "${2:-}"; target=$2; shift 2 ;;
      --expected-state) require_value "$1" "${2:-}"; state_digest=$2; shift 2 ;;
      --consequence) require_value "$1" "${2:-}"; consequence=$2; shift 2 ;;
      --ttl) require_value "$1" "${2:-}"; ttl=$2; shift 2 ;;
      *) die_usage "unknown request argument: $1" ;;
    esac
  done
  [ -n "$boundary" ] && [ -n "$task" ] && [ -n "$project" ] \
    && [ -n "$operation" ] && [ -n "$target" ] && [ -n "$state_digest" ] \
    && [ -n "$consequence" ] || die_usage "request requires every binding field"
  mx_override_request "$boundary" "$task" "$project" "$operation" "$target" "$state_digest" "$consequence" "$ttl"
}

command_decide() {
  local decision=$1 request=${2:-} words=''
  [ -n "$request" ] || die_usage "$decision requires a request id"
  shift 2
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --maintainer-words) require_value "$1" "${2:-}"; words=$2; shift 2 ;;
      *) die_usage "unknown $decision argument: $1" ;;
    esac
  done
  [ -n "$words" ] || die_usage "$decision requires --maintainer-words"
  if [ "$decision" = grant ]; then
    mx_override_grant "$request" "$words"
  else
    mx_override_deny "$request" "$words"
  fi
}

command_consume() {
  local request=${1:-} boundary='' task='' project='' operation='' target='' state_digest=''
  [ -n "$request" ] || die_usage "consume requires a request id"
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --boundary) require_value "$1" "${2:-}"; boundary=$2; shift 2 ;;
      --task) require_value "$1" "${2:-}"; task=$2; shift 2 ;;
      --project) require_value "$1" "${2:-}"; project=$2; shift 2 ;;
      --operation) require_value "$1" "${2:-}"; operation=$2; shift 2 ;;
      --target) require_value "$1" "${2:-}"; target=$2; shift 2 ;;
      --expected-state) require_value "$1" "${2:-}"; state_digest=$2; shift 2 ;;
      *) die_usage "unknown consume argument: $1" ;;
    esac
  done
  [ -n "$boundary" ] && [ -n "$task" ] && [ -n "$project" ] \
    && [ -n "$operation" ] && [ -n "$target" ] && [ -n "$state_digest" ] \
    || die_usage "consume requires every binding field"
  mx_override_consume "$request" "$boundary" "$task" "$project" "$operation" "$target" "$state_digest"
}

command_result() {
  local request=${1:-} outcome='' detail=''
  [ -n "$request" ] || die_usage "result requires a request id"
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --outcome) require_value "$1" "${2:-}"; outcome=$2; shift 2 ;;
      --detail) require_value "$1" "${2:-}"; detail=$2; shift 2 ;;
      *) die_usage "unknown result argument: $1" ;;
    esac
  done
  [ -n "$outcome" ] && [ -n "$detail" ] || die_usage "result requires --outcome and --detail"
  mx_override_result "$request" "$outcome" "$detail"
}

command_inspect() {
  [ "$#" -eq 1 ] || die_usage "inspect requires one request id"
  local file
  file=$(mx_override_find_record "$1") || {
    mx_override_error "request not found or invalid: $1"
    exit 1
  }
  jq -S . "$file"
}

command_audit() {
  local json=0 root state file invalid=0 output
  case "${1:-}" in '') ;; --json) json=1 ;; *) die_usage "audit accepts only --json" ;; esac
  root=$(mx_override_state_root)
  [ -d "$root" ] && [ ! -L "$root" ] || { [ "$json" -eq 0 ] && exit 0; printf '[]\n'; exit 0; }
  if [ "$json" -eq 1 ]; then
    output=$(mktemp "${TMPDIR:-/tmp}/mx-override-audit.XXXXXX") || exit 1
    trap 'rm -f "$output"; mx_override_lock_release' EXIT
    for state in pending granted denied consumed stale; do
      for file in "$root/$state"/*.json; do
        [ -e "$file" ] || continue
        if mx_override_record_validate "$file" "$state"; then
          jq -c --arg record_state "$state" '. + {record_state:$record_state}' "$file" >>"$output"
        else
          invalid=1
        fi
      done
    done
    jq -sc 'sort_by(.requested_at,.request_id)' "$output"
    rm -f "$output"
    trap mx_override_lock_release EXIT
  else
    for state in pending granted denied consumed stale; do
      for file in "$root/$state"/*.json; do
        [ -e "$file" ] || continue
        if ! mx_override_record_validate "$file" "$state"; then
          printf 'invalid\t%s\n' "$file"
          invalid=1
          continue
        fi
        jq -r --arg state "$state" '[.request_id,$state,.boundary_id,.task_id,.project,.target_identity,.outcome] | @tsv' "$file"
      done
    done
  fi
  [ "$invalid" -eq 0 ]
}

command_handoff() {
  [ "$#" -eq 1 ] || die_usage "handoff requires one request id"
  local file boundary decision
  file=$(mx_override_find_record "$1") || { mx_override_error "request not found or invalid: $1"; exit 1; }
  decision=$(jq -r '.decision' "$file")
  [ "$decision" = consumed ] || { mx_override_error "handoff requires an atomically consumed request"; exit 1; }
  [ "$(jq -r '.outcome' "$file")" = not-run ] || { mx_override_error "handoff request already has an outcome"; exit 1; }
  boundary=$(jq -r '.boundary_id' "$file")
  case "$boundary" in authentication.login|delivery.credentialed-action) ;;
    *) mx_override_error "boundary does not use operator handoff: $boundary"; exit 1 ;;
  esac
  jq -r '"request=" + .request_id + "\nboundary=" + .boundary_id + "\ntarget=" + .target_identity + "\noperation=" + .action_argv_or_operation + "\nconsequence=" + .consequence' "$file"
}

case "${1:-}" in
  registry) shift; command_registry "$@" ;;
  request) shift; command_request "$@" ;;
  grant|deny) decision=$1; shift; command_decide "$decision" "$@" ;;
  consume) shift; command_consume "$@" ;;
  result) shift; command_result "$@" ;;
  inspect) shift; command_inspect "$@" ;;
  audit) shift; command_audit "$@" ;;
  digest) shift; [ "$#" -eq 1 ] || die_usage "digest requires one literal argument"; mx_override_sha256_text "$1" ;;
  argv) shift; jq -cn '$ARGS.positional' --args -- "$@" ;;
  handoff) shift; command_handoff "$@" ;;
  -h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
