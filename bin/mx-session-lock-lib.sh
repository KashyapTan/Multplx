#!/usr/bin/env bash
# Shared session-lock harness identity.
#
# ONE owner of the "which verified-harness process holds this home's session
# lock, and does the current process descend from that same harness?" decision.
# bin/mx-lock.sh uses it to acquire and inspect state/.lock;
# bin/mx-claude-stop-autoarm.sh uses it to prove a Stop hook fires inside the
# lock-owning primary session before it may arm or rewake.
# This file is sourced by scripts and has no side effects on source.

# Known harness command names; extend when a new adapter is verified.
MX_HARNESS_RE='^(claude|codex|codex-cli|cursor-agent|pi)(\.(js|py))?$'

# Native Claude installs execute a version-named binary at this exact layout.
# Do not match arbitrary version basenames or a directory merely named claude.
mx_native_claude_path_matches() {
  local path=$1 version=${1##*/}
  case "$path" in */./*|*/../*|*/.|*/..|*/) return 1 ;; esac
  case "$path" in /*/.local/share/claude/versions/*) ;; *) return 1 ;; esac
  case "${path%/*}" in */.local/share/claude/versions) ;; *) return 1 ;; esac
  printf '%s' "$version" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'
}

# Match a command or an interpreter's script basename, not directory names
# or arbitrary tool payloads. Keep this sourced ABI aligned with the Rust owner.
mx_harness_command_matches() {
  local comm=$1 args=$2 script word interpreter executable=$1 argv0 known_re='^(claude|codex|codex-cli|cursor-agent|pi)(\.(js|py))?$'
  local -a mx_harness_words
  comm=${comm##*/}
  if printf '%s' "$comm" | grep -qE "$known_re" && printf '%s' "$comm" | grep -qE "$MX_HARNESS_RE"; then return 0; fi
  if printf '%s' claude | grep -qE "$MX_HARNESS_RE"; then
    if mx_native_claude_path_matches "$executable"; then return 0; fi
    argv0=${args%% *}
    if [ "$executable" = "$comm" ] && [ "${argv0##*/}" = "$comm" ] && mx_native_claude_path_matches "$argv0"; then return 0; fi
  fi
  interpreter=$(printf '%s' "$comm" | tr '[:upper:]' '[:lower:]')
  case "$interpreter" in
    node|nodejs|sh|bash|zsh|dash|ksh) ;;
    *) printf '%s' "$interpreter" | grep -qE '^python([23](\.[0-9]+)*)?$' || return 1 ;;
  esac
  read -r -a mx_harness_words <<< "$args"
  script=
  for word in "${mx_harness_words[@]:1}"; do
    case "$interpreter:$word" in sh:-*|bash:-*|zsh:-*|dash:-*|ksh:-*) return 1 ;; esac
    case "$interpreter" in
      node|nodejs) case "$word" in --no-warnings|--enable-source-maps) continue ;; esac ;;
      python*) case "$word" in -u|-B|-E|-I|-O|-OO) continue ;; esac ;;
    esac
    case "$word" in
      -c|-m|-e|--eval|--module|-r|--require|--import) return 1 ;;
      -*) return 1 ;;
      *) script=$word; break ;;
    esac
  done
  [ -n "$script" ] || return 1
  case "$interpreter" in
    sh|bash|zsh|dash|ksh)
      case "${script##*/}" in codex|codex-cli|claude|cursor-agent|pi) ;; *) return 1 ;; esac ;;
  esac
  if printf '%s' "${script##*/}" | grep -qE "$known_re" && printf '%s' "${script##*/}" | grep -qE "$MX_HARNESS_RE"; then return 0; fi
  case "$script" in
    */@anthropic-ai/claude-code/cli.js) printf '%s' claude | grep -qE "$MX_HARNESS_RE" ;;
    */@mariozechner/pi-coding-agent/dist/cli.js) printf '%s' pi | grep -qE "$MX_HARNESS_RE" ;;
    *) return 1 ;;
  esac
}

# Walk the current process ancestry (up to 8 hops) and print the first pid whose
# command looks like a verified harness. The harness pid lives as long as the
# session, unlike the transient subshell pid of any one tool call.
mx_harness_ancestry_pid() {
  local pid=$$ comm args
  for _ in 1 2 3 4 5 6 7 8; do
    comm=$(ps -o comm= -p "$pid" 2>/dev/null) || return 1
    args=$(ps -o args= -p "$pid" 2>/dev/null)
    if mx_harness_command_matches "$comm" "$args"; then
      echo "$pid"; return 0
    fi
    pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
    [ -n "$pid" ] && [ "$pid" -gt 1 ] || return 1
  done
  return 1
}

# True if $1 is a live process that looks like a verified harness.
mx_harness_pid_alive() {
  local pid=$1 comm args
  kill -0 "$pid" 2>/dev/null || return 1
  comm=$(ps -o comm= -p "$pid" 2>/dev/null) || return 1
  args=$(ps -o args= -p "$pid" 2>/dev/null) || return 1
  mx_harness_command_matches "$comm" "$args"
}

# True when state dir $1 holds a session lock whose pid is the harness ancestor
# of the current process: this script runs inside the session that owns the
# home's system lock. A missing lock, a lock held by another live harness, or an
# ancestry that cannot be resolved all fail closed.
mx_session_lock_owned_by_self() {
  local state=$1 lock_pid my_pid
  lock_pid=$(cat "$state/.lock" 2>/dev/null || true)
  case "$lock_pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  my_pid=$(mx_harness_ancestry_pid) || return 1
  [ "$my_pid" = "$lock_pid" ]
}
