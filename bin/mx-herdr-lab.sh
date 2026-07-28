#!/usr/bin/env bash
# Provision and operate an isolated Herdr lab session without risking the live
# default session.
#
# Usage:
#   mx-herdr-lab.sh name <label>
#   mx-herdr-lab.sh prepare <session>
#   mx-herdr-lab.sh provision <session>
#   mx-herdr-lab.sh run <session> <herdr arguments...>
#   mx-herdr-lab.sh stop <session>
#   mx-herdr-lab.sh teardown <session>
#
# Session names must begin with "mx-lab-" and can never be "default".
# The name command sanitizes the label, caps it at 16 characters, and appends
# process/random suffixes to keep generated socket paths short.
# Every Herdr call made here carries a trailing --session <session>.
# The run command rejects caller-supplied --session flags, any leading option
# before the subcommand, all session lifecycle operations, and every server
# operation.
# Session stop is available only through guarded stop or teardown, and session
# delete is available only through teardown.
# Both paths perform a fresh refuse-default check immediately before each
# destructive call.
# Provision records the running default session as a system-state tripwire and
# teardown requires that record to be identical afterward.
set -u

mx_herdr_lab_error() {
  echo "mx-herdr-lab: $*" >&2
}

mx_herdr_lab_validate_name() { # <session>
  local name=${1:-}
  [[ "$name" =~ ^mx-lab-[a-zA-Z0-9][a-zA-Z0-9_-]*$ ]] && return 0
  case "$name" in
    default) mx_herdr_lab_error "refusing session name 'default'" ;;
    '') mx_herdr_lab_error "refusing an empty session name" ;;
    *) mx_herdr_lab_error "session name must start with 'mx-lab-' and contain only letters, digits, underscores, or dashes: $name" ;;
  esac
  return 1
}

mx_herdr_lab_state_dir() {
  printf '%s' "${MX_HERDR_LAB_STATE_DIR:-${TMPDIR:-/tmp}/mx-herdr-lab-${UID}}"
}

mx_herdr_lab_tripwire_path() { # <session>
  printf '%s/%s.system-state.json' "$(mx_herdr_lab_state_dir)" "$1"
}

mx_herdr_lab_raw() { # <session> <herdr arguments...>
  local name=$1
  shift
  HERDR_SESSION="$name" herdr "$@" --session "$name"
}

mx_herdr_lab_session_list() { # <session>
  mx_herdr_lab_raw "$1" session list --json
}

mx_herdr_lab_system_state() { # <session>
  local name=$1 sessions snapshot
  sessions=$(mx_herdr_lab_session_list "$name" 2>/dev/null) || {
    mx_herdr_lab_error "cannot read Herdr sessions for the system-state tripwire"
    return 1
  }
  snapshot=$(printf '%s' "$sessions" | jq -c '
    [.sessions[]? | select(.default == true)]
    | if length == 1 and .[0].name == "default" and .[0].running == true
      then .[0] | {name, default, running, socket_path}
      else empty
      end
  ' 2>/dev/null)
  [ -n "$snapshot" ] || {
    mx_herdr_lab_error "system-state tripwire requires exactly one running default session"
    return 1
  }
  printf '%s\n' "$snapshot"
}

mx_herdr_lab_prepare() { # <session>
  local name=$1 sessions state_dir tripwire
  mx_herdr_lab_validate_name "$name" || return 1
  command -v herdr >/dev/null 2>&1 || { mx_herdr_lab_error "herdr is required"; return 1; }
  command -v jq >/dev/null 2>&1 || { mx_herdr_lab_error "jq is required"; return 1; }

  sessions=$(mx_herdr_lab_session_list "$name" 2>/dev/null) || {
    mx_herdr_lab_error "cannot list Herdr sessions before provisioning '$name'"
    return 1
  }
  if printf '%s' "$sessions" | jq -e --arg name "$name" '.sessions[]? | select(.name == $name)' >/dev/null 2>&1; then
    mx_herdr_lab_error "session '$name' already exists; refusing to adopt or overwrite it"
    return 1
  fi

  state_dir=$(mx_herdr_lab_state_dir)
  tripwire=$(mx_herdr_lab_tripwire_path "$name")
  mkdir -p "$state_dir" || return 1
  [ ! -e "$tripwire" ] || {
    mx_herdr_lab_error "tripwire already exists for '$name'; refusing ambiguous ownership"
    return 1
  }
  mx_herdr_lab_system_state "$name" > "$tripwire" || {
    rm -f "$tripwire"
    return 1
  }
}

mx_herdr_lab_refuse_if_default() { # <session>
  local name=$1 info flag
  mx_herdr_lab_validate_name "$name" || return 1
  info=$(mx_herdr_lab_session_list "$name" 2>/dev/null) || {
    mx_herdr_lab_error "refusing destructive call because session list failed"
    return 1
  }
  flag=$(printf '%s' "$info" | jq -r --arg name "$name" \
    '.sessions[]? | select(.name == $name) | .default' 2>/dev/null)
  [ "$flag" = false ] && return 0
  mx_herdr_lab_error "refusing destructive call for '$name': session is absent or default (default=${flag:-<not found>})"
  return 1
}

mx_herdr_lab_cli() { # <session> <herdr arguments...>
  local name=$1 arg
  shift
  mx_herdr_lab_validate_name "$name" || return 1
  [ "$#" -gt 0 ] || { mx_herdr_lab_error "run requires Herdr arguments"; return 1; }
  case "$1" in
    -*)
      mx_herdr_lab_error "run forbids a leading option before the Herdr subcommand; it could shift a server or session lifecycle operation past the guard or subvert session isolation"
      return 1
      ;;
  esac
  for arg in "$@"; do
    case "$arg" in
      --session|--session=*)
        mx_herdr_lab_error "run forbids caller-supplied --session; the helper appends the lab session"
        return 1
        ;;
    esac
  done
  case "$1 ${2:-}" in
    "server "*)
      mx_herdr_lab_error "run forbids server operations; use provision for the named lab server"
      return 1
      ;;
    "session list") ;;
    "session "*)
      mx_herdr_lab_error "run forbids session lifecycle operations; use guarded teardown"
      return 1
      ;;
  esac
  mx_herdr_lab_raw "$name" "$@"
}

mx_herdr_lab_cancel_provision() { # <pid>
  local pid=$1 attempt=0
  if kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    while kill -0 "$pid" 2>/dev/null && [ "$attempt" -lt 10 ]; do
      sleep 0.1
      attempt=$((attempt + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
  fi
  wait "$pid" 2>/dev/null || true
}

mx_herdr_lab_provision() { # <session>
  local name=$1 sessions tripwire running attempt server_pid max_attempts timeout_seconds
  mx_herdr_lab_validate_name "$name" || return 1
  command -v herdr >/dev/null 2>&1 || { mx_herdr_lab_error "herdr is required"; return 1; }
  command -v jq >/dev/null 2>&1 || { mx_herdr_lab_error "jq is required"; return 1; }

  sessions=$(mx_herdr_lab_session_list "$name" 2>/dev/null) || {
    mx_herdr_lab_error "cannot list Herdr sessions before provisioning '$name'"
    return 1
  }
  if printf '%s' "$sessions" | jq -e --arg name "$name" '.sessions[]? | select(.name == $name)' >/dev/null 2>&1; then
    tripwire=$(mx_herdr_lab_tripwire_path "$name")
    [ -f "$tripwire" ] || {
      mx_herdr_lab_error "missing system-state tripwire for existing session '$name'; refusing to adopt it"
      return 1
    }
    mx_herdr_lab_refuse_if_default "$name" || return 1
    running=$(printf '%s' "$sessions" | jq -r --arg name "$name" \
      '.sessions[]? | select(.name == $name) | .running' 2>/dev/null)
    [ "$running" = false ] || {
      mx_herdr_lab_error "session '$name' is not stopped; refusing to re-provision it"
      return 1
    }
    mx_herdr_lab_check_tripwire "$name" || return 1
  else
    mx_herdr_lab_prepare "$name" || return 1
  fi
  mx_herdr_lab_raw "$name" server >/dev/null 2>&1 &
  server_pid=$!
  attempt=0
  max_attempts=300
  timeout_seconds=60
  while [ "$attempt" -lt "$max_attempts" ]; do
    running=$(mx_herdr_lab_cli "$name" status --json 2>/dev/null | jq -r '.server.running // false' 2>/dev/null) || running=false
    if [ "$running" = true ]; then
      mx_herdr_lab_refuse_if_default "$name" || {
        mx_herdr_lab_cancel_provision "$server_pid"
        return 1
      }
      return 0
    fi
    sleep 0.2
    attempt=$((attempt + 1))
  done
  mx_herdr_lab_cancel_provision "$server_pid"
  mx_herdr_lab_error "lab session '$name' did not report running within $timeout_seconds seconds"
  return 1
}

mx_herdr_lab_check_tripwire() { # <session>
  local name=$1 tripwire before after
  tripwire=$(mx_herdr_lab_tripwire_path "$name")
  [ -f "$tripwire" ] || {
    mx_herdr_lab_error "missing system-state tripwire for '$name'; refusing unverified teardown"
    return 1
  }
  before=$(cat "$tripwire")
  after=$(mx_herdr_lab_system_state "$name") || return 1
  [ "$before" = "$after" ] || {
    mx_herdr_lab_error "SYSTEM-STATE TRIPWIRE FAILED: default session changed during lab work"
    mx_herdr_lab_error "before: $before"
    mx_herdr_lab_error "after:  $after"
    return 1
  }
}

mx_herdr_lab_verify_tripwire() { # <session>
  local name=$1 tripwire
  mx_herdr_lab_check_tripwire "$name" || return 1
  tripwire=$(mx_herdr_lab_tripwire_path "$name")
  rm -f "$tripwire"
}

mx_herdr_lab_stop() { # <session>
  local name=$1 tripwire
  mx_herdr_lab_validate_name "$name" || return 1
  tripwire=$(mx_herdr_lab_tripwire_path "$name")
  [ -f "$tripwire" ] || {
    mx_herdr_lab_error "missing system-state tripwire for '$name'; refusing stop"
    return 1
  }
  mx_herdr_lab_refuse_if_default "$name" || return 1
  mx_herdr_lab_raw "$name" session stop "$name" --json
}

mx_herdr_lab_teardown() { # <session>
  local name=$1 tripwire sessions delete_status=0
  mx_herdr_lab_validate_name "$name" || return 1
  tripwire=$(mx_herdr_lab_tripwire_path "$name")
  [ -f "$tripwire" ] || {
    mx_herdr_lab_error "missing system-state tripwire for '$name'; refusing destructive calls"
    return 1
  }
  sessions=$(mx_herdr_lab_session_list "$name" 2>/dev/null) || {
    mx_herdr_lab_error "cannot list Herdr sessions before teardown"
    return 1
  }
  if ! printf '%s' "$sessions" | jq -e --arg name "$name" '.sessions[]? | select(.name == $name)' >/dev/null 2>&1; then
    mx_herdr_lab_verify_tripwire "$name"
    return
  fi
  mx_herdr_lab_stop "$name" >/dev/null 2>&1 || true
  sleep 0.5
  mx_herdr_lab_refuse_if_default "$name" || return 1
  mx_herdr_lab_raw "$name" session delete "$name" --json >/dev/null 2>&1 || delete_status=$?
  sessions=$(mx_herdr_lab_session_list "$name" 2>/dev/null) || {
    mx_herdr_lab_error "cannot confirm removal of lab session '$name' after teardown"
    return 1
  }
  if printf '%s' "$sessions" | jq -e --arg name "$name" '.sessions[]? | select(.name == $name)' >/dev/null 2>&1; then
    if [ "$delete_status" -ne 0 ]; then
      mx_herdr_lab_error "session delete failed for '$name' and the lab session remains"
    else
      mx_herdr_lab_error "lab session '$name' remains after teardown"
    fi
    return 1
  fi
  mx_herdr_lab_verify_tripwire "$name"
}

mx_herdr_lab_name() { # <label>
  local label=${1:-lab}
  label=$(printf '%s' "$label" | tr -cd 'a-zA-Z0-9_-' | sed 's/^[^a-zA-Z0-9]*//; s/-*$//')
  [ -n "$label" ] || label=lab
  label=${label:0:16}
  label=${label%-}
  [ -n "$label" ] || label=lab
  printf 'mx-lab-%s-%s-%s\n' "$label" "$$" "$RANDOM"
}

mx_herdr_lab_usage() {
  sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

mx_herdr_lab_main() {
  local command=${1:-}
  case "$command" in
    name)
      [ "$#" -eq 2 ] || { mx_herdr_lab_usage >&2; return 2; }
      mx_herdr_lab_name "$2"
      ;;
    prepare)
      [ "$#" -eq 2 ] || { mx_herdr_lab_usage >&2; return 2; }
      mx_herdr_lab_prepare "$2"
      ;;
    provision)
      [ "$#" -eq 2 ] || { mx_herdr_lab_usage >&2; return 2; }
      mx_herdr_lab_provision "$2"
      ;;
    run)
      [ "$#" -ge 3 ] || { mx_herdr_lab_usage >&2; return 2; }
      shift
      mx_herdr_lab_cli "$@"
      ;;
    stop)
      [ "$#" -eq 2 ] || { mx_herdr_lab_usage >&2; return 2; }
      mx_herdr_lab_stop "$2"
      ;;
    teardown)
      [ "$#" -eq 2 ] || { mx_herdr_lab_usage >&2; return 2; }
      mx_herdr_lab_teardown "$2"
      ;;
    -h|--help|help)
      mx_herdr_lab_usage
      ;;
    *)
      mx_herdr_lab_usage >&2
      return 2
      ;;
  esac
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -e
  mx_herdr_lab_main "$@"
fi
