#!/usr/bin/env bash
# PreToolUse guard against primary-session delegation outside the system.
#
# A broker primary that delegates through a harness's own delegation,
# scheduling, or background-work tool creates work with no `state/<id>.meta` and
# no `data/<id>/brief.md`. Only `bin/mx-spawn.sh` writes that metadata, and
# untracked project work contributes nothing to the in-flight branch of
# bin/mx-supervision-lib.sh or bin/mx-turnend-guard.sh. So such work is not
# merely unsupervised: it makes the whole guard stack structurally inert, and
# it dies with the primary session instead of living in its own backend
# session.
#
# This scoped PreToolUse guard is the delivered mechanism.
# Claude primaries should also use an untracked per-home local
# `permissions.deny` list as hardening for known Claude delegation tools,
# because it removes them from the model's schema entirely.
# That deny list must not be tracked: it is Claude-only rather than
# harness-agnostic, and tracked project settings propagate into linked
# worktrees where they disarm legitimate actors.
# The tracked Claude matcher is deliberately `.*`: a stem-enumerating matcher
# would reintroduce the fail-open-by-enumeration problem this guard exists to
# solve, because any future tool name outside the matcher would never reach this
# script.
# This script is therefore the single owner of classification.
# It matches a delegation-SHAPED tool name rather than a fixed list, so a future
# tool that delivers before anyone updates a local deny list is still refused.
#
# The guard is narrow by design. It classifies ONE thing: the shape of the tool
# name. It makes no judgment about whether the work should be delegated at all,
# which is a reasoning boundary no tool-shape hook can enforce.
# See docs/subagent-guard.md for the complete contract and validation record.
#
# Usage:
#   <PreToolUse JSON on stdin> | bin/mx-subagent-pretool-check.sh
#   bin/mx-subagent-pretool-check.sh --tool '<tool-name>'
#
# Stdin mode extracts .tool_name for Claude and Codex (with a legacy .toolName
# fallback). CLI mode is for adapters that already hold the tool name (Pi).
#
# Exit/output contract (identical shape to bin/mx-cd-pretool-check.sh):
#   ALLOW - exit 0 and no output.
#   DENY - exit 2, a Claude-shaped deny object on stderr, and a plain
#          {"decision":"deny",...} object on stdout unless --claude was supplied.
#   INERT - not a genuine primary home (an actor/scout task worktree or a
#           non-Multplx repo): exit 0 with no output, exactly like ALLOW.
#   ESCAPE - MX_ALLOW_SUBAGENT=1 in the environment allows deliberately.
#   FAIL OPEN - malformed or empty stdin, or missing jq for stdin transport.
#
# Claude requires stdout to remain empty on deny.
# Codex blocks on exit 2 and displays stderr.
# Pi consumes exit 2 plus stderr; the stdout decision object remains the
# default-mode transport for adapters that consume a decision JSON.
set -u

# Lowercase substrings that mark a tool name as delegation-shaped: it creates
# work, an agent, a schedule, or an isolated workspace that broker would not
# know about. This list is the single owner of the delivered classification.
DELEGATION_STEMS='agent subagent task workflow cron schedul worktree delegate spawn dispatch handoff remote sendmessage monitor'

# Exact lowercase tool names that match a stem above but only OBSERVE or STOP
# work that already exists. Reading or ending unaccounted work is not creating
# it, and denying these would strand already-running work with no way to inspect
# or end it. A local Claude deny list may still remove these from the
# schema; this delivered guard deliberately stays narrower so it can never be the
# reason a runaway task cannot be stopped.
OBSERVE_ONLY_TOOLS='taskoutput taskstop taskget tasklist cronlist bashoutput killshell'

TOOL=""
TOOL_SET=0
CLAUDE_MODE=0

usage() {
  cat <<'EOF'
Usage: mx-subagent-pretool-check.sh [--tool <tool-name>] [--claude]

With no --tool, reads a PreToolUse-style JSON payload on stdin (Claude/Codex
tool_name).
Denies a delegation-SHAPED tool name in a genuine primary home.
Claude primaries may also add an untracked per-home permissions.deny list that
removes known delegation tools from the model schema before this hook is needed.
Do not land that Claude-only list in tracked project settings, because linked
worktrees inherit it and legitimate actors would lose their delegation tools.
This hook remains as the delivered guard for future delegation-shaped names
outside any local fixed list.
Fires only in a genuine broker primary home; it is a silent no-op in a
actor/scout task worktree or any non-Multplx repo, where a worker using
delegation tools is legitimate.
Exits 0 to allow and 2 to deny, naming the real actor dispatch path instead.
Set MX_ALLOW_SUBAGENT=1 in the session environment to allow deliberately.
Malformed transport fails open.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tool)
      [ "$#" -gt 1 ] || { echo "error: --tool requires a value" >&2; exit 2; }
      TOOL=$2
      TOOL_SET=1
      shift 2
      ;;
    --tool=*)
      TOOL=${1#--tool=}
      TOOL_SET=1
      shift
      ;;
    --claude)
      CLAUDE_MODE=1
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

if [ "$TOOL_SET" -eq 0 ]; then
  PAYLOAD=$(cat 2>/dev/null || true)
  [ -n "$PAYLOAD" ] || exit 0
  command -v jq >/dev/null 2>&1 || exit 0
  TOOL=$(printf '%s' "$PAYLOAD" | jq -r '(.tool_name // .toolName // empty)' 2>/dev/null) || exit 0
fi

[ -n "$TOOL" ] || exit 0

LC_ALL=C NORMALIZED=$(printf '%s' "$TOOL" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9')

# An MCP tool belongs to an external integration, not to the harness's own
# delegation surface, and its name is chosen by that server. Never classify one
# here: an MCP server with a task or agent noun in a tool name is common and
# blocking it would be a false positive with no bearing on system dispatch.
case "$TOOL" in
  mcp__*) exit 0 ;;
esac

for allowed in $OBSERVE_ONLY_TOOLS; do
  [ "$NORMALIZED" != "$allowed" ] || exit 0
done

MATCHED=""
for stem in $DELEGATION_STEMS; do
  case "$NORMALIZED" in
    *"$stem"*) MATCHED=$stem; break ;;
  esac
done
[ -n "$MATCHED" ] || exit 0

# The single deliberate escape hatch. It is an environment variable rather than
# a flag or a state file so it must be set when the session is launched, which
# makes a genuinely intended use possible and an accidental one impossible: no
# in-session tool call can set it for the call that follows.
[ "${MX_ALLOW_SUBAGENT:-}" != "1" ] || exit 0

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" 2>/dev/null && pwd -P) || exit 0
MX_ROOT=${MX_ROOT_OVERRIDE:-$(CDPATH='' cd -- "$SCRIPT_DIR/.." 2>/dev/null && pwd -P)} || exit 0
MX_HOME=${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}
STATE=${MX_STATE_OVERRIDE:-$MX_HOME/state}

# Scope to a genuine primary home, exactly as the session-start nudge and the
# turn-end guard do. mx_primary_scope_matches accepts a plain checkout or a
# marked daemon home - both operate a system and must dispatch through it -
# and rejects a linked task worktree, which is the shape bin/mx-spawn.sh always
# hands an actor. an actor using delegation tools inside its own task
# worktree is legitimate and stays allowed. Any failure to confirm the home is
# inert (exit 0), never a block, so a broken environment never denies a call.
# shellcheck source=bin/mx-primary-scope-lib.sh
. "$SCRIPT_DIR/mx-primary-scope-lib.sh"
mx_primary_scope_matches "$MX_ROOT" "$STATE" || exit 0

# Name the dedicated scout entry point only when this home carries it; degrade
# to the two-step brief-then-spawn path when it does not, rather than naming a
# script that is not there.
if [ -f "$MX_ROOT/bin/mx-scout.sh" ]; then
  ROUTE='first classify the work under the AGENTS.md intake contract: work already classified as a scout goes to bin/mx-scout.sh "<question>" [project], while authorized delivery work and its bounded research go to bin/mx-brief.sh then bin/mx-spawn.sh'
else
  ROUTE='first classify the work under the AGENTS.md intake contract, then use bin/mx-brief.sh followed by bin/mx-spawn.sh for dispatched work'
fi

REASON="[subagent-dispatch] the broker primary dispatches through the system, not the harness's own delegation tools: work started that way has no durable system record, leaves every broker guard inert, and dies with this session. Instead, $ROUTE (blocked tool: $TOOL, delegation-shaped on \"$MATCHED\"). Launch the session with MX_ALLOW_SUBAGENT=1 for a deliberate exception."

json_escape() {
  printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | tr '\n' ' '
}

ESCAPED=$(json_escape "$REASON")
printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"},"systemMessage":"%s"}\n' "$ESCAPED" >&2
[ "$CLAUDE_MODE" -eq 1 ] || printf '{"decision":"deny","reason":"%s"}\n' "$ESCAPED"
exit 2
