#!/usr/bin/env bash
# mx-afk-launch.sh - the single owner of the away-mode daemon TERMINAL lifecycle:
# launch it in a NON-VISIBLE tracked terminal per backend, record its exact id,
# tear it down by that exact id, and reconcile a leaked one after a crash.
#
# Why this exists (docs/herdr-backend.md "Away-mode daemon terminal launch"):
# bin/mx-afk-start.sh execs the supervise daemon in the FOREGROUND of whatever
# terminal it is already in. Harnesses with a native in-pane tracked-background
# tool (claude) run it there directly and it is fine. A harness with NO
# native background mechanism (pi) has to manufacture a terminal, and doing that
# by SPLITTING the maintainer's active pane visibly shrinks it - the regression this
# script fixes. Instead this creates a non-visible tracked terminal (a herdr tab/
# workspace with --no-focus, or a detached tmux session) that never touches the
# maintainer's active tab, and NEVER uses shell `&` (which herdr/codex can reap).
#
# Correct supervisor targeting: the daemon finds the maintainer pane to inject into
# from its OWN inherited env (discover_supervisor_target). Running it in a
# separate terminal would make it discover its OWN pane, so this captures the
# maintainer pane FIRST (from the pane this script runs in) and passes it in as
# MX_SUPERVISOR_TARGET/MX_SUPERVISOR_BACKEND explicitly.
#
# Usage:
#   mx-afk-launch.sh start     Capture the maintainer pane, then (unless the daemon
#                              is already running) launch the daemon in a fresh
#                              non-visible terminal for the detected backend and
#                              record it. Idempotent: an already-running daemon
#                              just refreshes state/.afk; a recorded-but-dead
#                              terminal is reconciled (closed by id) first.
#   mx-afk-launch.sh start-native
#                              Prepare lifecycle state for a harness-native
#                              background job and record that no terminal exists.
#   mx-afk-launch.sh stop      Correct-ordered exit: SIGTERM the daemon so its
#                              cleanup flushes WHILE state/.afk is still present,
#                              wait for it, close the recorded terminal by exact
#                              id, then clear state/.afk last.
#   mx-afk-launch.sh reconcile Close a recorded-but-dead daemon terminal by exact
#                              id and drop the record (recovery after a crash).
#
# Supported backends: herdr, tmux. Others (cmux) have no verified
# non-visible-launch primitive here yet and refuse loudly.
#
# Test seam: MX_AFK_LAUNCH_ENTRY overrides the command run in the created
# terminal (default bin/mx-afk-start.sh), so a topology test can run a harmless
# placeholder instead of a real daemon. MX_SUPERVISOR_TARGET/MX_SUPERVISOR_BACKEND
# override the captured maintainer pane/backend (an isolated lab pane in tests).
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
    exec "$mx_supervision_adapter_bin" supervision mx-afk-launch.sh "$@"
  fi
fi

MX_AFK_LAUNCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$MX_AFK_LAUNCH_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
MX_AFK_LAUNCH_STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
MX_AFK_LAUNCH_RECORD="$MX_AFK_LAUNCH_STATE/.afk-daemon-terminal"
MX_AFK_LAUNCH_LOCK="$MX_AFK_LAUNCH_STATE/.afk-launch.lock"
MX_AFK_LAUNCH_WS_LABEL="broker-afk-daemon"

# shellcheck source=bin/mx-backend.sh
. "$MX_AFK_LAUNCH_DIR/mx-backend.sh"
# shellcheck source=bin/mx-supervisor-target-lib.sh
. "$MX_AFK_LAUNCH_DIR/mx-supervisor-target-lib.sh"
# mx-afk-start.sh provides the daemon-lock liveness helpers and
# mx_afk_clear_stale_artifacts; it is sourceable (BASH_SOURCE guard) and its
# main does not run on source. It sets `set -eu`, so turn errexit back off for
# this script's best-effort flow immediately after.
# shellcheck source=bin/mx-afk-start.sh
. "$MX_AFK_LAUNCH_DIR/mx-afk-start.sh"
set +e

mx_afk_launch_log() { printf 'mx-afk-launch: %s\n' "$*" >&2; }

mx_afk_launch_lock_owned() {
  local pid expected actual
  [ -d "$MX_AFK_LAUNCH_LOCK" ] || return 1
  pid=$(cat "$MX_AFK_LAUNCH_LOCK/pid" 2>/dev/null) || return 1
  expected=$(cat "$MX_AFK_LAUNCH_LOCK/pid-identity" 2>/dev/null) || return 1
  actual=$(mx_pid_identity "$pid" 2>/dev/null) || return 1
  [ -n "$expected" ] && [ "$actual" = "$expected" ]
}

mx_afk_launch_lock_acquire() {
  local attempt=0 incomplete=0 identity
  mkdir -p "$MX_AFK_LAUNCH_STATE" || return 1
  while [ "$attempt" -lt 200 ]; do
    attempt=$((attempt + 1))
    if mkdir "$MX_AFK_LAUNCH_LOCK" 2>/dev/null; then
      if ! printf '%s' "$$" > "$MX_AFK_LAUNCH_LOCK/pid"; then
        rm -rf "$MX_AFK_LAUNCH_LOCK"
        return 1
      fi
      identity=$(mx_pid_identity "$$" 2>/dev/null) || {
        rm -rf "$MX_AFK_LAUNCH_LOCK"
        return 1
      }
      if [ -z "$identity" ] || ! printf '%s' "$identity" > "$MX_AFK_LAUNCH_LOCK/pid-identity"; then
        rm -rf "$MX_AFK_LAUNCH_LOCK"
        return 1
      fi
      return 0
    fi
    if [ ! -s "$MX_AFK_LAUNCH_LOCK/pid" ] || [ ! -s "$MX_AFK_LAUNCH_LOCK/pid-identity" ]; then
      incomplete=$((incomplete + 1))
      if [ "$incomplete" -lt 20 ]; then
        sleep 0.05
        continue
      fi
    else
      incomplete=0
    fi
    if ! mx_afk_launch_lock_owned; then
      rm -rf "$MX_AFK_LAUNCH_LOCK" 2>/dev/null || return 1
      incomplete=0
      continue
    fi
    sleep 0.05
  done
  mx_afk_launch_log "timed out waiting for launcher lock"
  return 1
}

mx_afk_launch_lock_release() {
  local pid
  pid=$(cat "$MX_AFK_LAUNCH_LOCK/pid" 2>/dev/null || true)
  [ "$pid" = "$$" ] || return 0
  rm -rf "$MX_AFK_LAUNCH_LOCK"
}

mx_afk_launch_usage() {
  sed -n '2,34p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# The command run inside the created terminal. Real launch runs the shared
# daemon entry; a test overrides it with a harmless placeholder.
mx_afk_launch_entry_cmd() {
  printf '%s' "${MX_AFK_LAUNCH_ENTRY:-$MX_ROOT/bin/mx-afk-start.sh}"
}

mx_afk_launch_record_write() {  # <backend> <target> <extra>
  local pending
  mkdir -p "$MX_AFK_LAUNCH_STATE" || return 1
  pending=$(mktemp "$MX_AFK_LAUNCH_STATE/.afk-daemon-terminal.pending.XXXXXX") || return 1
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" > "$pending" || { rm -f "$pending"; return 1; }
  mv "$pending" "$MX_AFK_LAUNCH_RECORD" || { rm -f "$pending"; return 1; }
}

mx_afk_launch_flag_write() {
  local pending="$MX_AFK_LAUNCH_STATE/.afk.pending.$$"
  date '+%s' > "$pending" || { rm -f "$pending"; return 1; }
  mv "$pending" "$MX_AFK_LAUNCH_STATE/.afk" || { rm -f "$pending"; return 1; }
}

# Read the recorded terminal into MX_AFK_REC_BACKEND/MX_AFK_REC_TARGET. The third
# field (a herdr workspace id, kept for the record's own documentation) is not
# needed to close by id, so it is discarded. Returns 1 when no record exists.
mx_afk_launch_record_read() {
  local extra record
  MX_AFK_REC_BACKEND=""; MX_AFK_REC_TARGET=""; extra=""
  [ -f "$MX_AFK_LAUNCH_RECORD" ] || return 1
  record=$(cat "$MX_AFK_LAUNCH_RECORD" 2>/dev/null) || record=""
  IFS=$'\t' read -r MX_AFK_REC_BACKEND MX_AFK_REC_TARGET extra \
    < "$MX_AFK_LAUNCH_RECORD" || true
  if ! printf '%s\n' "$record" | awk -F '\t' 'NF != 3 { bad=1 } END { exit !(NR == 1 && !bad) }' \
    || [ -z "$MX_AFK_REC_BACKEND" ] || [ -z "$MX_AFK_REC_TARGET" ]; then
    mx_afk_launch_log "daemon terminal record is malformed; refusing to act on it"
    return 2
  fi
  case "$MX_AFK_REC_BACKEND" in
    herdr) [ -n "$extra" ] ;;
    tmux) : ;;
    none) [ "$MX_AFK_REC_TARGET" = - ] && [ "$extra" = native ] ;;
    *) return 2 ;;
  esac || { mx_afk_launch_log "daemon terminal record is malformed; refusing to act on it"; return 2; }
}

mx_afk_launch_record_validate_if_present() {
  local result
  mx_afk_launch_record_read
  result=$?
  [ "$result" -ne 2 ]
}

# Close a recorded terminal by EXACT id (never a broad sweep). The
# recorded workspace id (herdr) needs no separate close: closing the pane takes
# its single-tab dedicated workspace with it.
mx_afk_launch_close_terminal() {  # <backend> <target>
  local backend=$1 target=$2
  case "$backend" in
    herdr)
      mx_backend_source herdr || return 1
      local session=${target%%:*} pane=${target#*:}
      [ -n "$session" ] && [ -n "$pane" ] && [ "$pane" != "$target" ] || return 1
      mx_backend_herdr_cli "$session" pane close "$pane" >/dev/null 2>&1
      ;;
    tmux)
      # target is the dedicated daemon session name - kill exactly it.
      tmux kill-session -t "$target" 2>/dev/null
      ;;
    none)
      return 0
      ;;
    *)
      mx_afk_launch_log "cannot close unknown recorded backend '$backend'"
      return 1
      ;;
  esac
}

mx_afk_launch_terminal_absent() {  # <backend> <target>
  local backend=$1 target=$2 session pane out result code
  case "$backend" in
    herdr)
      session=${target%%:*}
      pane=${target#*:}
      [ -n "$session" ] && [ -n "$pane" ] && [ "$pane" != "$target" ] || return 1
      out=$(mx_backend_herdr_cli "$session" pane get "$pane" 2>&1)
      result=$?
      [ "$result" -ne 0 ] || return 1
      code=$(printf '%s' "$out" | jq -r '.error.code // empty' 2>/dev/null) || return 1
      [ "$code" = pane_not_found ]
      ;;
    tmux)
      out=$(tmux has-session -t "$target" 2>&1)
      result=$?
      [ "$result" -eq 1 ] || return 1
      printf '%s' "$out" | grep -Eq "can't find session"
      ;;
    none)
      return 0
      ;;
    *) return 1 ;;
  esac
}

mx_afk_launch_close_recorded() {
  local close_result=0
  mx_afk_launch_close_terminal "$MX_AFK_REC_BACKEND" "$MX_AFK_REC_TARGET" || close_result=$?
  if mx_afk_launch_terminal_absent "$MX_AFK_REC_BACKEND" "$MX_AFK_REC_TARGET"; then
    rm -f "$MX_AFK_LAUNCH_RECORD" || return 1
    [ "$close_result" -eq 0 ] || mx_afk_launch_log "terminal close command failed, but exact absence was confirmed"
    return 0
  fi
  mx_afk_launch_log "recorded terminal teardown is unconfirmed; preserving exact id"
  return 1
}

mx_afk_launch_terminal_alive() {  # <backend> <target>
  local backend=$1 target=$2 session pane
  case "$backend" in
    herdr)
      session=${target%%:*}
      pane=${target#*:}
      [ -n "$session" ] && [ -n "$pane" ] && [ "$pane" != "$target" ] || return 1
      mx_backend_herdr_cli "$session" pane get "$pane" >/dev/null 2>&1
      ;;
    tmux)
      tmux has-session -t "$target" 2>/dev/null
      ;;
    *) return 1 ;;
  esac
}

mx_afk_launch_wait_ready() {  # <backend> <target>
  local backend=$1 target=$2 attempt=0
  if [ -n "${MX_AFK_LAUNCH_ENTRY:-}" ]; then
    mx_afk_launch_terminal_alive "$backend" "$target"
    return
  fi
  while [ "$attempt" -lt 100 ]; do
    attempt=$((attempt + 1))
    daemon_lock_held_by_live_daemon && return 0
    mx_afk_launch_terminal_alive "$backend" "$target" || return 1
    sleep 0.05
  done
  return 1
}

mx_afk_launch_commit_terminal() {  # <backend> <target> <extra> [already-recorded]
  local backend=$1 target=$2 extra=$3 already_recorded=${4:-0}
  if [ "$already_recorded" -ne 1 ] && ! mx_afk_launch_record_write "$backend" "$target" "$extra"; then
    mx_afk_launch_log "failed to persist daemon terminal record; closing $backend:$target"
    mx_afk_launch_close_terminal "$backend" "$target"
    return 1
  fi
  if ! mx_afk_launch_wait_ready "$backend" "$target"; then
    mx_afk_launch_log "daemon did not become ready; closing $backend:$target"
    MX_AFK_REC_BACKEND=$backend
    MX_AFK_REC_TARGET=$target
    mx_afk_launch_close_recorded
    return 1
  fi
}

mx_afk_launch_herdr_recover_created() {  # <session> <label>
  local session=$1 label=$2 workspaces ws_count wsid panes pane_count pane attempt=0
  while [ "$attempt" -lt 20 ]; do
    attempt=$((attempt + 1))
    workspaces=$(mx_backend_herdr_cli "$session" workspace list 2>/dev/null) || { sleep 0.05; continue; }
    ws_count=$(printf '%s' "$workspaces" | jq --arg want "$label" \
      '[.result.workspaces[]? | select(.label == $want)] | length' 2>/dev/null) || { sleep 0.05; continue; }
    if [ "$ws_count" = 0 ]; then
      sleep 0.05
      continue
    fi
    [ "$ws_count" = 1 ] || return 1
    wsid=$(printf '%s' "$workspaces" | jq -r --arg want "$label" \
      '.result.workspaces[]? | select(.label == $want) | .workspace_id' 2>/dev/null) || return 1
    [ -n "$wsid" ] || return 1
    panes=$(mx_backend_herdr_cli "$session" pane list --workspace "$wsid" 2>/dev/null) || { sleep 0.05; continue; }
    pane_count=$(printf '%s' "$panes" | jq '[.result.panes[]?] | length' 2>/dev/null) || { sleep 0.05; continue; }
    if [ "$pane_count" = 0 ]; then
      sleep 0.05
      continue
    fi
    [ "$pane_count" = 1 ] || return 1
    pane=$(printf '%s' "$panes" | jq -r '.result.panes[0].pane_id // empty' 2>/dev/null) || return 1
    [ -n "$pane" ] || return 1
    printf '%s\t%s' "$wsid" "$pane"
    return 0
  done
  return 1
}

# Reconcile a recorded-but-dead terminal: if a record exists and no live daemon
# owns it, close the leaked terminal by exact id and drop the record.
mx_afk_launch_reconcile() {
  local read_result
  if daemon_lock_held_by_live_daemon; then
    return 0
  fi
  mx_afk_launch_record_read
  read_result=$?
  if [ "$read_result" -eq 0 ]; then
    mx_afk_launch_log "reconciling leaked daemon terminal ${MX_AFK_REC_BACKEND}:${MX_AFK_REC_TARGET}"
    mx_afk_launch_close_recorded
  elif [ "$read_result" -eq 2 ]; then
    return 1
  fi
}

mx_afk_launch_restore_backup() {  # <backup> <had-afk>
  local backup=$1 had_afk=$2 artifact result=0
  rm -f "$MX_AFK_LAUNCH_STATE/.afk" \
    "$MX_AFK_LAUNCH_STATE/.subsuper-escalations" \
    "$MX_AFK_LAUNCH_STATE/.subsuper-escalations.since" \
    "$MX_AFK_LAUNCH_STATE/.subsuper-inject-wedged" || result=1
  if [ "$had_afk" -eq 1 ]; then
    cp "$backup/.afk" "$MX_AFK_LAUNCH_STATE/.afk" || result=1
  fi
  for artifact in .subsuper-escalations .subsuper-escalations.since .subsuper-inject-wedged; do
    if [ -e "$backup/$artifact" ]; then
      cp -p "$backup/$artifact" "$MX_AFK_LAUNCH_STATE/$artifact" || result=1
    fi
  done
  if [ "$result" -eq 0 ]; then
    rm -rf "$backup" || return 1
  else
    mx_afk_launch_log "rollback restoration incomplete; backup retained at $backup"
  fi
  return "$result"
}

# Launch the daemon in a non-visible herdr terminal in the MAINTAINER's session
# (so the daemon can inject into the maintainer pane, which lives there). A
# dedicated background workspace (--no-focus) holds exactly one tab/pane; it
# never touches the maintainer's active tab. Prints the record line on success.
mx_afk_launch_create_herdr() {  # <maintainer-target> <maintainer-backend>
  local maintainer_target=$1 maintainer_backend=$2 session out wsid pane entry cmd label recovered create_result
  session=${maintainer_target%%:*}
  if [ -z "$session" ] || [ "$session" = "$maintainer_target" ]; then
    mx_afk_launch_log "cannot derive herdr session from maintainer target '$maintainer_target'"
    return 1
  fi
  mx_backend_source herdr || return 1
  mx_backend_herdr_server_ensure "$session" || { mx_afk_launch_log "herdr server not ready for session '$session'"; return 1; }
  label=${MX_AFK_LAUNCH_LABEL:-"$MX_AFK_LAUNCH_WS_LABEL-$$-${RANDOM:-0}-$(date '+%s')"}
  out=$(mx_backend_herdr_cli "$session" workspace create --cwd "$MX_HOME" --label "$label" --no-focus 2>/dev/null)
  create_result=$?
  wsid=$(printf '%s' "$out" | jq -r '.result.workspace.workspace_id // empty' 2>/dev/null)
  pane=$(printf '%s' "$out" | jq -r '.result.root_pane.pane_id // empty' 2>/dev/null)
  if [ "$create_result" -ne 0 ] && [ -n "$wsid" ] && [ -n "$pane" ]; then
    mx_afk_launch_log "herdr create failed after returning exact ids; closing $session:$pane"
    if mx_afk_launch_record_write herdr "$session:$pane" "$wsid"; then
      MX_AFK_REC_BACKEND=herdr
      MX_AFK_REC_TARGET="$session:$pane"
      mx_afk_launch_close_recorded || true
    else
      mx_afk_launch_log "failed to persist exact id for failed herdr create"
    fi
    return 1
  fi
  if [ -z "$wsid" ] || [ -z "$pane" ]; then
    recovered=$(mx_afk_launch_herdr_recover_created "$session" "$label") || {
      mx_afk_launch_log "herdr create did not yield a recoverable exact workspace/pane id"
      return 1
    }
    IFS=$'\t' read -r wsid pane <<< "$recovered"
  fi
  entry=$(mx_afk_launch_entry_cmd)
  cmd=$(printf 'exec env MX_HOME=%q MX_SUPERVISOR_TARGET=%q MX_SUPERVISOR_BACKEND=%q %q' \
    "$MX_HOME" "$maintainer_target" "$maintainer_backend" "$entry")
  if ! mx_afk_launch_record_write herdr "$session:$pane" "$wsid"; then
    mx_afk_launch_log "failed to persist herdr daemon terminal record; closing $session:$pane"
    mx_afk_launch_close_terminal herdr "$session:$pane"
    return 1
  fi
  if ! mx_backend_herdr_cli "$session" pane run "$pane" "$cmd" >/dev/null 2>&1; then
    mx_afk_launch_log "failed to run daemon in herdr pane $session:$pane; closing it"
    MX_AFK_REC_BACKEND=herdr
    MX_AFK_REC_TARGET="$session:$pane"
    mx_afk_launch_close_recorded || true
    return 1
  fi
  mx_afk_launch_commit_terminal herdr "$session:$pane" "$wsid" 1 || return 1
  mx_afk_launch_log "daemon launched in non-visible herdr workspace $wsid (pane $session:$pane), supervising $maintainer_target"
}

# Launch the daemon in a detached tmux session (never a split-window in the
# maintainer's window). tmux pane ids are server-global, so the daemon reaches the
# maintainer pane by its %id from this separate session.
mx_afk_launch_create_tmux() {  # <maintainer-target> <maintainer-backend>
  local maintainer_target=$1 maintainer_backend=$2 session entry cmd hash nonce
  hash=$(printf '%s' "$MX_HOME" | cksum | cut -d' ' -f1)
  nonce="$$-${RANDOM:-0}-$(date '+%s')"
  session="mx-afk-daemon-$hash-$nonce"
  entry=$(mx_afk_launch_entry_cmd)
  cmd=$(printf 'exec env MX_HOME=%q MX_SUPERVISOR_TARGET=%q MX_SUPERVISOR_BACKEND=%q %q' \
    "$MX_HOME" "$maintainer_target" "$maintainer_backend" "$entry")
  if ! mx_afk_launch_record_write tmux "$session" ""; then
    mx_afk_launch_log "failed to persist planned tmux daemon session '$session'"
    return 1
  fi
  if ! tmux new-session -d -s "$session" "$cmd" 2>/dev/null; then
    mx_afk_launch_log "failed to create detached tmux daemon session '$session'"
    if ! rm -f "$MX_AFK_LAUNCH_RECORD"; then
      mx_afk_launch_log "failed to remove planned tmux daemon record after creation failure"
    fi
    return 1
  fi
  mx_afk_launch_commit_terminal tmux "$session" "" 1 || return 1
  mx_afk_launch_log "daemon launched in detached tmux session '$session', supervising $maintainer_target"
}

mx_afk_launch_start() {
  local maintainer_target maintainer_backend backup artifact had_afk=0 result
  if [ -e "$MX_AFK_LAUNCH_STATE/.afk-return-catchup" ]; then
    mx_afk_launch_log "return catch-up is still pending; run bin/mx-afk-return.sh check before re-entering away mode"
    return 1
  fi
  # Capture the maintainer pane FIRST, before creating anything.
  maintainer_target=$(discover_supervisor_target) || {
    mx_afk_launch_log "could not resolve the maintainer supervisor pane (set MX_SUPERVISOR_TARGET)"; return 1; }
  maintainer_backend=$(discover_supervisor_backend) || {
    mx_afk_launch_log "could not resolve the maintainer supervisor backend (set MX_SUPERVISOR_BACKEND)"; return 1; }

  mkdir -p "$MX_AFK_LAUNCH_STATE"

  if daemon_lock_held_by_live_daemon; then
    mx_afk_launch_record_validate_if_present || return 1
    if ! mx_afk_launch_flag_write; then
      mx_afk_launch_log "failed to refresh away-mode flag"
      return 1
    fi
    mx_afk_launch_log "daemon already running; refreshed away-mode flag (no new terminal)"
    return 0
  fi

  backup=$(mktemp -d "$MX_AFK_LAUNCH_STATE/.afk-launch-backup.XXXXXX") || return 1
  if [ -f "$MX_AFK_LAUNCH_STATE/.afk" ]; then
    had_afk=1
    cp "$MX_AFK_LAUNCH_STATE/.afk" "$backup/.afk" || { rm -rf "$backup"; return 1; }
  fi
  for artifact in .subsuper-escalations .subsuper-escalations.since .subsuper-inject-wedged; do
    if [ -e "$MX_AFK_LAUNCH_STATE/$artifact" ]; then
      cp -p "$MX_AFK_LAUNCH_STATE/$artifact" "$backup/$artifact" || { rm -rf "$backup"; return 1; }
    fi
  done
  if ! mx_afk_launch_reconcile; then
    result=1
  else
    if mx_afk_clear_stale_artifacts "$MX_AFK_LAUNCH_STATE"; then
      result=0
    else
      mx_afk_launch_log "failed to clear stale away-mode artifacts"
      result=1
    fi
  fi
  if [ "$result" -eq 0 ]; then
    if ! mx_afk_launch_flag_write; then
      mx_afk_launch_log "failed to write away-mode flag"
      result=1
    fi
  fi

  if [ "$result" -eq 0 ]; then
    case "$maintainer_backend" in
      herdr) mx_afk_launch_create_herdr "$maintainer_target" "$maintainer_backend"; result=$? ;;
      tmux)  mx_afk_launch_create_tmux "$maintainer_target" "$maintainer_backend"; result=$? ;;
      *)
        mx_afk_launch_log "no non-visible daemon-launch primitive for backend '$maintainer_backend' yet (supported: herdr, tmux)"
        result=1
        ;;
    esac
  fi
  if [ "$result" -ne 0 ]; then
    mx_afk_launch_restore_backup "$backup" "$had_afk" || result=1
  else
    rm -rf "$backup" || result=1
  fi
  return "$result"
}

mx_afk_launch_start_native() {
  local backup artifact had_afk=0 result=0
  mkdir -p "$MX_AFK_LAUNCH_STATE" || return 1
  if [ -e "$MX_AFK_LAUNCH_STATE/.afk-return-catchup" ]; then
    mx_afk_launch_log "return catch-up is still pending; run bin/mx-afk-return.sh check before re-entering away mode"
    return 1
  fi
  if daemon_lock_held_by_live_daemon; then
    mx_afk_launch_record_validate_if_present || return 1
    mx_afk_launch_flag_write || return 1
    mx_afk_launch_log "daemon already running; refreshed away-mode flag"
    return 0
  fi
  backup=$(mktemp -d "$MX_AFK_LAUNCH_STATE/.afk-launch-backup.XXXXXX") || return 1
  if [ -f "$MX_AFK_LAUNCH_STATE/.afk" ]; then
    had_afk=1
    cp "$MX_AFK_LAUNCH_STATE/.afk" "$backup/.afk" || { rm -rf "$backup"; return 1; }
  fi
  for artifact in .subsuper-escalations .subsuper-escalations.since .subsuper-inject-wedged; do
    if [ -e "$MX_AFK_LAUNCH_STATE/$artifact" ]; then
      cp -p "$MX_AFK_LAUNCH_STATE/$artifact" "$backup/$artifact" || { rm -rf "$backup"; return 1; }
    fi
  done
  mx_afk_launch_reconcile || result=1
  if [ "$result" -eq 0 ]; then
    if ! mx_afk_clear_stale_artifacts "$MX_AFK_LAUNCH_STATE"; then
      mx_afk_launch_log "failed to clear stale away-mode artifacts"
      result=1
    elif ! mx_afk_launch_flag_write; then
      result=1
    fi
  fi
  if [ "$result" -eq 0 ]; then
    mx_afk_launch_record_write none - native || result=1
  fi
  if [ "$result" -ne 0 ]; then
    mx_afk_launch_restore_backup "$backup" "$had_afk" || result=1
  else
    rm -rf "$backup" || result=1
  fi
  return "$result"
}

mx_afk_launch_stop() {
  local pid pid_identity current_identity result=0 read_result
  mx_afk_launch_record_read
  read_result=$?
  if [ "$read_result" -eq 2 ]; then
    mx_afk_launch_log "malformed daemon terminal record; refusing to stop away mode"
    return 1
  fi
  # (1) SIGTERM the daemon so its cleanup trap flushes buffered escalations
  # WHILE state/.afk is still present (the exit-ordering fix: clearing .afk
  # first would make that flush a no-op via inject_msg's presence gate).
  pid=""
  pid_identity=""
  if daemon_lock_held_by_live_daemon; then
    pid=$(daemon_lock_pid 2>/dev/null) || return 1
    pid_identity=$(mx_pid_identity "$pid" 2>/dev/null) || return 1
  fi
  if [ -n "$pid" ]; then
    if ! kill -TERM "$pid" 2>/dev/null; then
      mx_afk_launch_log "failed to signal away-mode daemon pid=$pid"
      result=1
    fi
    for _ in $(seq 1 40); do
      mx_pid_alive "$pid" || break
      sleep 0.25
    done
  fi
  if [ -n "$pid" ] && mx_pid_alive "$pid"; then
    current_identity=$(mx_pid_identity "$pid" 2>/dev/null) || {
      mx_afk_launch_log "could not confirm away-mode daemon exit; preserving lifecycle state"
      return 1
    }
    if [ "$current_identity" = "$pid_identity" ]; then
      mx_afk_launch_log "away-mode daemon did not exit after SIGTERM; preserving lifecycle state"
      return 1
    fi
  fi
  # (2) Close the daemon's own terminal by exact id.
  if [ "$read_result" -eq 0 ]; then
    mx_afk_launch_close_recorded || result=1
  fi
  # (3) Clear the away-mode flag LAST.
  if ! rm -f "$MX_AFK_LAUNCH_STATE/.afk"; then
    mx_afk_launch_log "failed to clear away-mode flag"
    result=1
  fi
  if [ "$result" -eq 0 ]; then
    mx_afk_launch_log "away mode stopped; daemon terminal torn down and .afk cleared"
  else
    mx_afk_launch_log "away mode stopped; terminal teardown remains recorded for retry"
  fi
  return "$result"
}

mx_afk_launch_main() {
  local result
  mx_afk_launch_lock_acquire || return 1
  trap mx_afk_launch_lock_release EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  case "${1:-start}" in
    start) mx_afk_launch_start ;;
    start-native) mx_afk_launch_start_native ;;
    stop) mx_afk_launch_stop ;;
    reconcile) mx_afk_launch_reconcile ;;
    -h|--help|help) mx_afk_launch_usage ;;
    *) mx_afk_launch_usage >&2; return 2 ;;
  esac
  result=$?
  mx_afk_launch_lock_release || result=1
  trap - EXIT INT TERM
  return "$result"
}

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  mx_afk_launch_main "$@"
fi
