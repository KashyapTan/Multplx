#!/usr/bin/env bash
# Self-update a running broker and its daemons to the latest origin.
#
# Mechanical half of the /updatemultplx skill. Fast-forwards the running
# Multplx repo's default branch from origin, then fast-forwards every
# registered daemon home (each a treehouse worktree of this same repo, or
# a standalone clone) the same way. FAST-FORWARD ONLY, exactly like
# mx-system-sync.sh: never force, never create a merge commit, never stash;
# advance a target only when it is a clean fast-forward, otherwise skip and
# report. A tracked-files fast-forward never touches the gitignored operational
# dirs (data/, state/, config/, projects/), so a daemon's
# in-flight work is never disrupted. Worktrees of this repo share one object
# store, so a single fetch refreshes them all; standalone-clone homes are
# fetched on their own. Daemon homes are leased at a detached HEAD on the
# default branch, so a fast-forward there advances HEAD only and never touches
# any other worktree's checkout or the shared `main` branch.
#
# The fast-forward mechanics live in bin/mx-ff-lib.sh (base_mode "origin" here);
# the same library drives the local-HEAD daemon sync used by mx-spawn.sh and
# mx-bootstrap.sh, so there is one ff implementation, not several.
#
# It does NOT re-read AGENTS.md or nudge daemons itself - those are LLM /
# tmux actions the skill performs. The script's job is the safe git mechanics
# plus a parseable summary telling the caller what to do next:
#   - one status line per target (updated/already current/skipped)
#   - reread-broker: yes|no    (did the running broker's instructions change)
#   - nudge-daemons: mx-<id>...|none   (updated live daemons to nudge)
#
# Usage: mx-update.sh [--help]
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
DAEMONS_MD="$MX_HOME/data/daemons.md"
# shellcheck source=bin/mx-ff-lib.sh
. "$SCRIPT_DIR/mx-ff-lib.sh"

"$SCRIPT_DIR/mx-guard.sh" || true

usage() { echo "usage: mx-update.sh [--help]" >&2; }

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  usage
  exit 0
fi
[ $# -eq 0 ] || { usage; exit 1; }

# --- main Multplx repo ---------------------------------------------------

reread_broker="no"
ff_target "$MX_ROOT" "broker" origin no no
if [ "$FF_STATUS" = "updated" ] && [ -n "$FF_INSTR" ]; then
  reread_broker="yes"
fi

# --- daemons -----------------------------------------------------------
# An updated live daemon is nudged whenever it advanced (nudge_requires_instr
# is "no" here): /updatemultplx's nudge is a gentle re-read steer, kept on the
# same condition it has always used.

FF_NUDGE_WINDOWS=""
FF_SEEN_HOMES=""

# Live daemons first: state/<id>.meta with kind=daemon carries the
# authoritative home= path.
sweep_live_daemon_metas "$STATE" origin no

# Registry backstop: a daemon registered in data/daemons.md but without
# a live meta (e.g. between restarts) is still its persistent on-disk home.
if [ -f "$DAEMONS_MD" ]; then
  while IFS= read -r line; do
    case "$line" in
      "- "*) ;;
      *) continue ;;
    esac
    id=$(printf '%s\n' "$line" | sed -n 's/^- \([^ ][^ ]*\) - .*/\1/p')
    home=$(printf '%s\n' "$line" | sed -n 's/.*(home:[[:space:]]*\([^;]*\);.*/\1/p' | sed 's/[[:space:]]*$//')
    process_daemon "$id" "$home" "" origin no
  done < "$DAEMONS_MD"
fi

# --- caller action summary -------------------------------------------------

echo "reread-broker: $reread_broker"
echo "nudge-daemons:${FF_NUDGE_WINDOWS:- none}"
