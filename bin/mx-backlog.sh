#!/usr/bin/env bash
# Operator CLI for the in-repo Multplx backlog library.
# Usage:
#   mx-backlog.sh list [--file <path>] [--limit <n>]
#   mx-backlog.sh show <id> [--file <path>] [--full]
#   mx-backlog.sh add <id> <title> [--file <path>] [options]
#   mx-backlog.sh done <id> [--file <path>] [--report p | --note s | --pr url]
#   mx-backlog.sh ready [--file <path>]
#   mx-backlog.sh hold <id> [--file <path>] --reason <text> --kind <kind>
#   mx-backlog.sh update <id> [--file <path>] (--body <text> | --body-file <path>) [--archive-body]
#   mx-backlog.sh block <id> [--file <path>] --by <blocker-id>
#   mx-backlog.sh unblock <id> [--file <path>] --by <blocker-id>
#   mx-backlog.sh mv <id>... --file <source> --to <destination>
#   mx-backlog.sh validate [--file <path>]
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
DEFAULT_FILE="$DATA/backlog.md"

# shellcheck source=bin/mx-backlog-lib.sh
. "$SCRIPT_DIR/mx-backlog-lib.sh"

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

command_name=${1:-}
case "$command_name" in
  list|show|add|done|ready|hold|update|block|unblock|mv|validate) shift ;;
  -h|--help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

file=$DEFAULT_FILE
destination=
limit=80
done_keep=
positionals=()
forward=()
want=
for argument in "$@"; do
  if [ -n "$want" ]; then
    case "$want" in
      file) file=$argument ;;
      to) destination=$argument ;;
      limit) limit=$argument ;;
      keep) done_keep=$argument ;;
      body-file)
        [ -f "$argument" ] && [ ! -L "$argument" ] || {
          echo "mx-backlog: body file must be a regular non-symlink file: $argument" >&2
          exit 1
        }
        forward+=(--body "$(cat "$argument")")
        ;;
      *) forward+=("--$want" "$argument") ;;
    esac
    want=
    continue
  fi
  case "$argument" in
    --file) want=file ;;
    --file=*) file=${argument#--file=} ;;
    --to) want=to ;;
    --to=*) destination=${argument#--to=} ;;
    --limit) want=limit ;;
    --limit=*) limit=${argument#--limit=} ;;
    --keep) want=keep ;;
    --keep=*) done_keep=${argument#--keep=} ;;
    --full) ;;
    --repo|--kind|--body|--body-file|--blocked-by|--report|--note|--pr|--reason|--by)
      want=${argument#--}
      ;;
    --archive-body|--start) forward+=("$argument") ;;
    --*) echo "mx-backlog: unknown option: $argument" >&2; exit 2 ;;
    *) positionals+=("$argument") ;;
  esac
done
[ -z "$want" ] || { echo "mx-backlog: --$want requires a value" >&2; exit 2; }

case "$command_name" in
  list)
    [ "${#positionals[@]}" -eq 0 ] || { usage >&2; exit 2; }
    mx_backlog_list "$file" "$limit" blocked_by,hold_kind,hold_reason
    ;;
  show)
    [ "${#positionals[@]}" -eq 1 ] || { usage >&2; exit 2; }
    mx_backlog_show "$file" "${positionals[0]}"
    ;;
  add)
    [ "${#positionals[@]}" -eq 2 ] || { usage >&2; exit 2; }
    mx_backlog_add "$file" "${positionals[0]}" "${positionals[1]}" "${forward[@]+"${forward[@]}"}"
    ;;
  done)
    [ "${#positionals[@]}" -eq 1 ] || { usage >&2; exit 2; }
    if [ -n "$done_keep" ]; then
      MX_BACKLOG_DONE_KEEP=$done_keep mx_backlog_done "$file" "${positionals[0]}" "${forward[@]+"${forward[@]}"}"
    else
      mx_backlog_done "$file" "${positionals[0]}" "${forward[@]+"${forward[@]}"}"
    fi
    ;;
  ready)
    [ "${#positionals[@]}" -eq 0 ] || { usage >&2; exit 2; }
    mx_backlog_ready "$file"
    ;;
  hold)
    [ "${#positionals[@]}" -eq 1 ] || { usage >&2; exit 2; }
    mx_backlog_hold "$file" "${positionals[0]}" "${forward[@]+"${forward[@]}"}"
    ;;
  update)
    [ "${#positionals[@]}" -eq 1 ] || { usage >&2; exit 2; }
    mx_backlog_update "$file" "${positionals[0]}" "${forward[@]+"${forward[@]}"}"
    ;;
  block)
    [ "${#positionals[@]}" -eq 1 ] || { usage >&2; exit 2; }
    mx_backlog_block "$file" "${positionals[0]}" "${forward[@]+"${forward[@]}"}"
    ;;
  unblock)
    [ "${#positionals[@]}" -eq 1 ] || { usage >&2; exit 2; }
    mx_backlog_unblock "$file" "${positionals[0]}" "${forward[@]+"${forward[@]}"}"
    ;;
  mv)
    [ "${#positionals[@]}" -gt 0 ] && [ -n "$destination" ] || { usage >&2; exit 2; }
    mx_backlog_mv "$file" "$destination" "${positionals[@]}"
    ;;
  validate)
    [ "${#positionals[@]}" -eq 0 ] || { usage >&2; exit 2; }
    mx_backlog_validate "$file"
    ;;
esac
