#!/usr/bin/env bash
# Detect the agent harness this process tree runs on.
# Usage: mx-harness.sh                  print own harness: claude|codex|cursor|pi|unknown
#        mx-harness.sh actor              print the effective ACTOR harness
#                                        (config/actor-harness; "default" resolves to own)
#        mx-harness.sh daemon       print the harness the PRIMARY uses to launch
#                                        DAEMON agents: config/daemon-harness ->
#                                        config/actor-harness -> own. "default" or absent
#                                        defers to the actor resolution, so an unset
#                                        daemon-harness behaves exactly as the actor
#                                        harness did before this knob existed.
#        mx-harness.sh daemon-model    print the optional MODEL token from
#                                        config/daemon-harness, or empty when absent.
#        mx-harness.sh daemon-effort   print the optional EFFORT token from
#                                        config/daemon-harness, or empty when absent.
# config/daemon-harness format: a single line "<harness> [<model>] [<effort>]",
# whitespace-separated. A bare "<harness>" (today's format) behaves exactly as before:
# harness only, no model/effort. Only the first non-empty, non-comment line is parsed.
# Model/effort come ONLY from this file - config/actor-harness stays a bare adapter
# name and is never parsed for a model.
# Detection layers: verified environment markers first, then process ancestry.
# Record each newly verified env marker here.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
CONFIG="${MX_CONFIG_OVERRIDE:-$MX_HOME/config}"

detect_own() {
  # Layer 1: environment markers for verified harnesses.
  [ "${CLAUDECODE:-}" = "1" ] && { echo claude; return; }
  [ "${PI_CODING_AGENT:-}" = "true" ] && { echo pi; return; }
  # Layer 2: walk the parent chain and match the command name.
  local pid=$$ comm args
  for _ in 1 2 3 4 5 6 7 8; do
    comm=$(ps -o comm= -p "$pid" 2>/dev/null) || break
    case "$(basename "$comm")" in
      *claude*) echo claude; return ;;
      *codex*) echo codex; return ;;
      cursor-agent) echo cursor; return ;;
      pi) echo pi; return ;;
      node*|python*)
        # Bare interpreter: match the harness name in its script path.
        args=$(ps -o args= -p "$pid" 2>/dev/null)
        case "$args" in
          *claude*) echo claude; return ;;
          *codex*) echo codex; return ;;
          *cursor-agent*) echo cursor; return ;;
          *" pi "*|*/pi) echo pi; return ;;
        esac ;;
    esac
    pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
    if [ -z "$pid" ] || [ "$pid" -le 1 ]; then
      break
    fi
  done
  echo unknown
}

# Resolve the effective actor harness: config/actor-harness (a bare adapter
# name) wins; absent or "default" mirrors broker's own harness.
resolve_actor() {
  local actor=
  [ -f "$CONFIG/actor-harness" ] && actor=$(tr -d '[:space:]' < "$CONFIG/actor-harness" || true)
  if [ -z "$actor" ] || [ "$actor" = "default" ]; then detect_own; else echo "$actor"; fi
}

# Print the first non-empty, non-comment line of config/daemon-harness
# (leading/trailing whitespace trimmed), or nothing when the file is absent or
# holds only blank/comment lines.
daemon_line() {
  local line
  [ -f "$CONFIG/daemon-harness" ] || return 0
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [ -n "$line" ] || continue
    case "$line" in
      '#'*) continue ;;
    esac
    printf '%s\n' "$line"
    return 0
  done < "$CONFIG/daemon-harness"
}

# Print the 1-based whitespace-separated token (1=harness, 2=model, 3=effort) of
# the resolved daemon_line, or nothing if the line or that field is absent.
daemon_field() {
  local idx=$1 line
  line=$(daemon_line)
  [ -n "$line" ] || return 0
  # shellcheck disable=SC2086  # deliberate word-splitting: tokenizing the line into fields
  set -- $line
  case "$idx" in
    1) printf '%s\n' "${1:-}" ;;
    2) printf '%s\n' "${2:-}" ;;
    3) printf '%s\n' "${3:-}" ;;
  esac
}

# Resolve the harness the PRIMARY uses to launch DAEMON agents: a fallback
# chain config/daemon-harness -> config/actor-harness -> own. An absent or
# "default" daemon-harness token defers to the actor resolution, so an unset
# daemon-harness behaves exactly as before this knob existed (a daemon
# launched on the actor harness). config/daemon-harness is the PRIMARY's own
# setting and is never inherited downstream - daemons do not spawn daemons.
resolve_daemon() {
  local sm
  sm=$(daemon_field 1)
  if [ -z "$sm" ] || [ "$sm" = "default" ]; then resolve_actor; else echo "$sm"; fi
}

# Print the optional model token (2nd field) from config/daemon-harness, or
# empty when the harness token is absent/"default" (harness-only file, same as
# today) or when no model token is present.
resolve_daemon_model() {
  local sm
  sm=$(daemon_field 1)
  [ -n "$sm" ] && [ "$sm" != "default" ] || return 0
  daemon_field 2
}

# Print the optional effort token (3rd field) from config/daemon-harness,
# the same way.
resolve_daemon_effort() {
  local sm
  sm=$(daemon_field 1)
  [ -n "$sm" ] && [ "$sm" != "default" ] || return 0
  daemon_field 3
}

case "${1:-}" in
  actor) resolve_actor ;;
  daemon) resolve_daemon ;;
  daemon-model) resolve_daemon_model ;;
  daemon-effort) resolve_daemon_effort ;;
  *) detect_own ;;
esac
