#!/usr/bin/env bash
# bin/backends/tmux.sh - the tmux session-provider adapter.
#
# Reference backend (AGENTS.md section 8; data/mx-backend-design-d7). P1 moves
# the tmux command sequences that mx-send.sh, mx-peek.sh, mx-watch.sh,
# mx-spawn.sh, and mx-teardown.sh already ran inline into named functions
# here, running the EXACT same commands in the EXACT same order, so the
# default (tmux, `backend=` absent) path stays byte-identical. Sourced only
# through bin/mx-backend.sh's mx_backend_source, never directly.
#
# Worktree acquisition (running `treehouse get` inside the pane, and polling
# its cwd) is unchanged by this extraction: P1 scopes only the session
# provider, not the worktree provider, so mx-spawn.sh still drives that part
# inline with these same send/current-path primitives.
#
# The verified composer/busy-detection and verify-and-retry-submit primitives
# already live in bin/mx-tmux-lib.sh, shared with the away-mode daemon
# (bin/mx-supervise-daemon.sh); this adapter sources that file and re-exports
# its submit core under the backend's naming convention rather than
# duplicating it, so the two consumers cannot drift apart.
# shellcheck source=bin/mx-tmux-lib.sh
. "$MX_BACKEND_LIB_DIR/mx-tmux-lib.sh"

# mx_backend_tmux_resolve_bare_selector: the live-window-listing fallback for a
# selector that is neither an explicit target nor a task selector routed
# through meta - an ad hoc window name with no recorded task. Mirrors the
# `tmux list-windows -a ... | grep` pipeline that used to live inline in
# mx-send.sh's and mx-peek.sh's own (until now duplicated) resolve().
mx_backend_tmux_resolve_bare_selector() {  # <name>
  local name=$1
  tmux list-windows -a -F '#{session_name}:#{window_name}' | grep -m1 ":$name\$" \
    || { echo "error: no window named $name" >&2; return 1; }
}

# mx_backend_tmux_capture: bounded plain-text pane capture. Mirrors
# mx-peek.sh's and mx-watch.sh's `tmux capture-pane -p -t "$T" -S -"$N"`.
mx_backend_tmux_capture() {  # <target> <lines>
  tmux capture-pane -p -t "$1" -S -"$2"
}

# mx_backend_tmux_send_key: one named key. Mirrors mx-send.sh's --key path:
# `tmux display-message -p -t "$T" '#{pane_id}' >/dev/null`, then
# `tmux send-keys -t "$T" "$2"`.
mx_backend_tmux_send_key() {  # <target> <key>
  tmux display-message -p -t "$1" '#{pane_id}' >/dev/null
  tmux send-keys -t "$1" "$2"
}

# mx_backend_tmux_send_text_submit: type <text> into <target> once, then
# submit with Enter, retried (Enter only, never retyped) until the composer
# clears. Re-exports mx_tmux_submit_core (bin/mx-tmux-lib.sh) verbatim; see
# that file for the composer-verification contract and echoed verdicts.
mx_backend_tmux_send_text_submit() {  # <target> <text> <retries> <enter-sleep> <settle>
  mx_tmux_submit_core "$@"
}

# mx_backend_tmux_container_ensure: reuse the current tmux session when
# broker itself runs inside tmux, else ensure a dedicated detached
# "broker" session exists. Mirrors mx-spawn.sh's container-ensure block;
# prints the resolved session name.
mx_backend_tmux_container_ensure() {
  if [ -n "${TMUX:-}" ]; then
    tmux display-message -p '#S'
  else
    tmux has-session -t broker 2>/dev/null || tmux new-session -d -s broker
    printf 'broker'
  fi
}

# mx_backend_tmux_create_task: create the task's window in <proj-abs>,
# refusing an existing <window-name> in <session>. Mirrors mx-spawn.sh's
# duplicate-check-then-new-window sequence, including the exact error text
# (session:window, matching how mx-spawn.sh composed its own $T). Prints the
# created window's stable window id on stdout for the caller to target.
#
# Robustness (mx-spawn tmux window handling under a non-default maintainer config):
#   - Capture a STABLE window id with -P -F '#{window_id}', and let tmux append
#     at the next free index by targeting the session with a trailing colon
#     ("$ses:"), so a non-default base-index (e.g. base-index 1) cannot collide.
#   - PIN the window name by disabling automatic-rename and allow-rename on the
#     new window: the maintainer's tmux may rename the window away from mx-<id> once
#     treehouse cd's into the worktree, which would break name-based targeting.
# The returned window id lets callers target the window even if its name is ever
# lost, so worktree discovery cannot fall back to the active client's window.
mx_backend_tmux_create_task() {  # <session> <window-name> <proj-abs> -> prints window id
  local ses=$1 wname=$2 proj_abs=$3 wid
  if tmux list-windows -t "$ses" -F '#{window_name}' | grep -qx "$wname"; then
    echo "error: window $ses:$wname already exists" >&2
    return 1
  fi
  wid=$(tmux new-window -dP -F '#{window_id}' -t "$ses:" -n "$wname" -c "$proj_abs") || return 1
  tmux set-window-option -t "$wid" automatic-rename off 2>/dev/null || true
  tmux set-window-option -t "$wid" allow-rename off 2>/dev/null || true
  printf '%s\n' "$wid"
}

# mx_backend_tmux_current_path: the live pane's current working directory, or
# empty on any tmux error. Mirrors mx-spawn.sh's worktree-discovery poll:
# `tmux display-message -p -t "$T" '#{pane_current_path}'`.
mx_backend_tmux_current_path() {  # <target>
  tmux display-message -p -t "$1" '#{pane_current_path}' 2>/dev/null
}

# mx_backend_tmux_send_text_line: send one line of TEXT then Enter, with no
# composer verification - used for the fixed spawn-time commands
# (`treehouse get`, the GOTMPDIR export) that already ran this exact sequence
# inline in mx-spawn.sh. Mirrors `tmux send-keys -t "$T" "<text>" Enter`.
mx_backend_tmux_send_text_line() {  # <target> <text>
  tmux send-keys -t "$1" "$2" Enter
}

# mx_backend_tmux_send_literal: send TEXT as literal bytes with no
# submission - the caller sends Enter separately (mx-spawn.sh's launch-command
# send pauses between the literal send and Enter for the harness to settle).
# Mirrors `tmux send-keys -t "$T" -l "<text>"`.
mx_backend_tmux_send_literal() {  # <target> <text>
  tmux send-keys -t "$1" -l "$2"
}

# mx_backend_tmux_kill: remove the task's window, best-effort. Mirrors
# mx-teardown.sh's `tmux kill-window -t "$T" 2>/dev/null || true`.
mx_backend_tmux_kill() {  # <target>
  tmux kill-window -t "$1" 2>/dev/null || true
}

# mx_backend_tmux_current_command: <target>'s live foreground process name -
# tmux's own `#{pane_current_command}`, already resolved from the pty's
# foreground process group (verified empirically with real tmux 3.6a: a
# harness invoked interactively stays the reported command even while it
# shells out to subcommands that do not take over the pty - e.g. `bash -c
# "sleep 30"` alone reports "sleep" because bash execs directly into it, but
# a persisting parent script running `sleep` as a child reports the PARENT's
# own name throughout; the value reverts to the shell's own name only once
# the foreground command actually exits). Empty on any tmux error.
mx_backend_tmux_current_command() {  # <target>
  tmux display-message -p -t "$1" '#{pane_current_command}' 2>/dev/null
}

# mx_backend_tmux_agent_state: recovery-grade harness-agent state for one
# recorded target. See bin/mx-backend.sh's mx_backend_agent_state for the
# shared state vocabulary and docs/tmux-backend.md "Agent liveness probe" for
# the empirical basis. Tmux silently falls back to the active window when a
# named target is absent, so the exact recorded window must appear in a
# successful session inventory before its foreground command can be trusted.
# An omitted window or a definitive missing-session/server response is
# `missing`; any other inventory or pane read failure is `unreadable`, so a
# transient tmux problem never licenses a duplicate.
mx_backend_tmux_agent_state() {  # <target>
  local target=$1 comm session window windows inventory_status
  case "$target" in
    *:*:*|'':*|*:'') printf 'unreadable'; return 0 ;;
    *:*) ;;
    *) printf 'unreadable'; return 0 ;;
  esac
  session=${target%%:*}
  window=${target#*:}
  if windows=$(LC_ALL=C tmux list-windows -t "$session" -F '#{window_name}' 2>&1); then
    inventory_status=0
  else
    inventory_status=$?
  fi
  if [ "$inventory_status" -ne 0 ]; then
    case "$windows" in
      *"can't find session:"*|*"no server running on "*|*"error connecting to "*" (No such file or directory)"|*"error connecting to "*" (Connection refused)")
        printf 'missing'
        ;;
      *)
        printf 'unreadable'
        ;;
    esac
    return 0
  fi
  if ! printf '%s\n' "$windows" | grep -Fqx "$window"; then
    printf 'missing'
    return 0
  fi

  comm=$(mx_backend_tmux_current_command "$target") || {
    printf 'unreadable'
    return 0
  }
  comm=${comm#-}
  case "$comm" in
    *claude*|*codex*) printf 'alive' ;;
    zsh|bash|sh|dash|ash|ksh|mksh|tcsh|csh|fish) printf 'dead' ;;
    '') printf 'unreadable' ;;
    *) printf 'ambiguous' ;;
  esac
}

# Backward-compatible three-state view for callers that only need a yes/no
# agent verdict. The detailed state contract is owned by mx_backend_agent_state.
mx_backend_tmux_agent_alive() {  # <target>
  case "$(mx_backend_tmux_agent_state "$1")" in
    alive) printf 'alive' ;;
    dead|missing) printf 'dead' ;;
    *) printf 'unknown' ;;
  esac
}
