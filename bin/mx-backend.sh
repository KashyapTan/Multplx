#!/usr/bin/env bash
# mx-backend.sh - runtime-backend selection, meta helpers, selector resolution,
# and dispatch for broker's session-provider abstraction.
#
# Design: data/mx-backend-design-d7/report.md ("Backend Interface") and
# data/mx-backend-design-d7/herdr-addendum.md ("Events as the core
# abstraction"). P1 extracted the tmux command sequences that mx-send.sh,
# mx-peek.sh, mx-watch.sh, mx-spawn.sh, and mx-teardown.sh already ran inline
# into bin/backends/tmux.sh, with those SAME command sequences, so the default
# (tmux) path stays byte-identical. P2 adds bin/backends/herdr.sh, an
# EXPERIMENTAL spawn-capable backend behind `--backend herdr`/`MX_BACKEND=herdr`/
# `config/backend`, and behind runtime auto-detection when broker itself is
# running inside herdr with no explicit backend setting; see herdr-addendum.md and
# data/mx-backend-design-d7/herdr-verification-p2.md for its empirical basis.
# P5 adds bin/backends/cmux.sh, also
# EXPERIMENTAL and spawn-capable, behind `--backend cmux`/`MX_BACKEND=cmux`/
# `config/backend`, and behind runtime auto-detection when broker itself is
# running inside a cmux-spawned terminal (primary CMUX_WORKSPACE_ID marker, or
# the documented macOS fallback signals when cmux's claude wrapper strips that
# marker) with no explicit backend setting; see
# docs/cmux-backend.md for its empirical basis.
# Codex App is intentionally not in the known set yet.
# docs/codex-app-backend.md owns that blocked backend contract.
#
# Compatibility contract: a task's meta may omit `backend=`; every reader here
# treats that as `tmux` (mx_backend_of_meta), and mx-spawn.sh does not write
# `backend=tmux` for a default-backend task, so existing and newly spawned
# default-path metas stay byte-identical. Only a task spawned on a non-tmux
# spawn-capable backend, currently experimental herdr or cmux,
# carries an explicit `backend=` line.
#
# Event-source framing (herdr-addendum "Events as the core abstraction"): a
# backend's supervision surface is conceptually an EVENT SOURCE - it produces
# task events (status-changed, went-stale, exited) that map onto broker's
# existing signal/stale/check/heartbeat wake vocabulary. The tmux adapter has
# no native event push, so mx-watch.sh's poll loop over the pull primitives
# below (capture, list-live, busy-state via regex) IS the default event-source
# implementation that synthesizes those events; P1 only names that seam, it
# does not change the loop's behavior. The pull primitives also stay available
# on their own for on-demand reads (mx-peek.sh, mx-actor-state.sh).

MX_BACKEND_SCRIPT=${BASH_SOURCE[0]:-$0}
MX_BACKEND_LIB_DIR="$(cd "$(dirname "$MX_BACKEND_SCRIPT")" && pwd)"
unset MX_BACKEND_SCRIPT
MX_BACKEND_DEFAULT_ROOT="$(cd "$MX_BACKEND_LIB_DIR/.." && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-${MX_ROOT:-$MX_BACKEND_DEFAULT_ROOT}}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
MX_BACKEND_CONFIG_DIR="${MX_CONFIG_OVERRIDE:-$MX_HOME/config}"
# shellcheck source=bin/mx-rust-runtime.sh
. "$MX_BACKEND_LIB_DIR/mx-rust-runtime.sh"
MX_BACKEND_IMPLEMENTATION_SELECTED=$(mx_backend_implementation) || {
  _mx_backend_implementation_status=$?
  return "$_mx_backend_implementation_status" 2>/dev/null || exit "$_mx_backend_implementation_status"
}

# Verified backend adapters. Extend only after a backend gets its own
# bin/backends/<name>.sh and empirical verification, mirroring AGENTS.md
# section 4's harness-verification discipline. herdr is EXPERIMENTAL (P2;
# data/mx-backend-design-d7/herdr-addendum.md) - verified against the real
# v0.7.1/protocol-14 binary (data/mx-backend-design-d7/herdr-verification-p2.md)
# but newer than tmux's long-proven default path.
# cmux is EXPERIMENTAL and spawn-capable, session-provider-only like
# herdr - verified against the real 0.64.17 binary (docs/cmux-backend.md).
# codex-app remains deliberately absent; see docs/codex-app-backend.md.
MX_BACKEND_KNOWN="tmux herdr cmux"
MX_BACKEND_SPAWN="tmux herdr cmux"

# mx_backend_list_contains: whitespace-delimited membership without relying on
# shell word splitting. mx-backend.sh is normally sourced by bash scripts, but
# zsh diagnostics can source it too, so backend-name matching must stay portable.
mx_backend_list_contains() {  # <list> <name>
  local list=$1 name=$2
  case "$name" in
    *[[:space:]]*) return 1 ;;
  esac
  case " $list " in
    *" $name "*) return 0 ;;
  esac
  return 1
}

mx_backend_is_known() {  # <name>
  mx_backend_list_contains "$MX_BACKEND_KNOWN" "$1"
}

# mx_backend_detect: detect the runtime broker itself is CURRENTLY executing
# inside, from verified environment markers (mirrors bin/mx-harness.sh's
# env-marker detection layer for harnesses). Prints the detected backend name
# and returns 0, or returns 1 when nothing is detected. Nesting resolves
# INNERMOST-first: tmux sets $TMUX in every process running inside it, even a
# tmux started inside a herdr pane, so $TMUX is checked first and wins over
# HERDR_ENV=1 in that nested case. herdr injects HERDR_ENV=1 (plus
# HERDR_SOCKET_PATH/HERDR_PANE_ID) into every process it manages a pane for;
# HERDR_ENV=1 alone (no $TMUX) selects herdr. cmux injects CMUX_WORKSPACE_ID
# (plus CMUX_SURFACE_ID/CMUX_SOCKET_PATH and the legacy CMUX_TAB_ID/
# CMUX_PANEL_ID aliases) into every terminal surface it spawns - verified from
# the delivered source (`TerminalSurface+StartupEnvironment.swift`'s
# `applyManagedCmuxContextEnvironment`, which marks all five keys
# `protectedKeys`, i.e. non-overridable) and corroborated by cmux's own CLI
# (`cmux_open.swift`) reading `CMUX_WORKSPACE_ID`/`CMUX_SURFACE_ID` as its own
# ambient-target fallback, exactly how `$TMUX` and `HERDR_ENV` work for their
# backends. CMUX_WORKSPACE_ID, not CMUX_SOCKET_PATH, is the chosen marker:
# CMUX_SOCKET_PATH is independently documented as a user-settable override for
# pointing the CLI at a non-default socket, so its mere presence would not
# reliably mean "running inside a cmux-spawned terminal" the way
# CMUX_WORKSPACE_ID does. cmux is checked LAST because it is a terminal
# application (the outermost layer, like iTerm2/Terminal.app), not a session
# multiplexer - both tmux and herdr can run nested inside a cmux-provided
# shell, but cmux cannot run nested inside either of them, so a tmux or herdr
# marker set alongside CMUX_WORKSPACE_ID always means that multiplexer is the
# innermost, currently-executing layer and must win.
#
# cmux FALLBACK signals (docs/cmux-backend.md "Runtime auto-detection" owns
# the empirical record): cmux's bundled `claude` PATH shim routes through
# cmux-claude-wrapper, whose passthrough path unsets every CMUX_* variable
# before exec'ing the real binary - so a claude-harness broker launched in
# a cmux tab can have NO CMUX_WORKSPACE_ID at all. When that primary marker is
# absent (and only then), two macOS-only fallback signals are consulted:
#   1. __CFBundleIdentifier == com.cmuxterm.app - LaunchServices' app-identity
#      env var, inherited by every process a cmux tab spawns and NOT stripped
#      by the wrapper (it only unsets CMUX_*, TERMINFO, and CLAUDECODE).
#      Authoritative in the common wrapper-strip case, but also inherited into
#      every pane of a tmux server started from a cmux tab - the $TMUX check
#      winning FIRST is what keeps that false positive absorbed.
#   2. Process ancestry reaching the running cmux app (resolved by bundle id
#      via lsappinfo, plus a bundle-shaped `ps` comm match so the install
#      location is never hardcoded). Authoritative when the environment was
#      scrubbed entirely (no bundle id to inherit); NOT usable from inside
#      tmux, where the tmux server reparents to launchd and the chain never
#      reaches cmux - which is fine, because $TMUX already won there.
# Callers needing the winning signal read MX_BACKEND_DETECT_SIGNAL (set to
# TMUX, HERDR_ENV, CMUX_WORKSPACE_ID, bundle-id, or ancestry) and
# MX_BACKEND_DETECTED after a direct (non-command-substitution) call.
MX_BACKEND_CMUX_BUNDLE_ID="com.cmuxterm.app"

mx_backend_detect() {
  MX_BACKEND_DETECTED=""
  MX_BACKEND_DETECT_SIGNAL=""
  if [ -n "${TMUX:-}" ]; then
    MX_BACKEND_DETECTED=tmux
    MX_BACKEND_DETECT_SIGNAL=TMUX
    printf 'tmux'
    return 0
  fi
  if [ "${HERDR_ENV:-}" = "1" ]; then
    MX_BACKEND_DETECTED=herdr
    MX_BACKEND_DETECT_SIGNAL=HERDR_ENV
    printf 'herdr'
    return 0
  fi
  if [ -n "${CMUX_WORKSPACE_ID:-}" ]; then
    MX_BACKEND_DETECTED=cmux
    MX_BACKEND_DETECT_SIGNAL=CMUX_WORKSPACE_ID
    printf 'cmux'
    return 0
  fi
  if mx_backend_detect_cmux_fallback; then
    MX_BACKEND_DETECTED=cmux
    printf 'cmux'
    return 0
  fi
  return 1
}

# mx_backend_detect_cmux_fallback: the two macOS-only cmux fallback signals
# (see mx_backend_detect's header comment). Sets MX_BACKEND_DETECT_SIGNAL to
# bundle-id or ancestry on success. Cheap-first: the bundle-id check is a pure
# env read; the ancestry walk (subprocess-per-hop) runs only when it misses.
mx_backend_detect_cmux_fallback() {
  [ "$(uname 2>/dev/null)" = Darwin ] || return 1
  if [ "${__CFBundleIdentifier:-}" = "$MX_BACKEND_CMUX_BUNDLE_ID" ]; then
    MX_BACKEND_DETECT_SIGNAL=bundle-id
    return 0
  fi
  if mx_backend_detect_cmux_app_is_ancestor; then
    MX_BACKEND_DETECT_SIGNAL=ancestry
    return 0
  fi
  return 1
}

# mx_backend_detect_cmux_app_pid: the running cmux app's pid, resolved by
# bundle id via lsappinfo (`"pid"=<n>`), or failure when lsappinfo is missing,
# errors, or the app is not running (lsappinfo prints nothing, exit 0).
mx_backend_detect_cmux_app_pid() {
  command -v lsappinfo >/dev/null 2>&1 || return 1
  local out pid
  out=$(lsappinfo info -only pid -app "$MX_BACKEND_CMUX_BUNDLE_ID" 2>/dev/null) || return 1
  pid=${out##*=}
  pid=$(printf '%s' "$pid" | tr -d '[:space:]"')
  case "$pid" in ''|*[!0-9]*) return 1 ;; esac
  printf '%s' "$pid"
}

# mx_backend_detect_cmux_app_is_ancestor: walk this process's parent chain and
# report whether it reaches the cmux app - matching either the lsappinfo-
# resolved pid (bundle id, no path assumption) or a bundle-shaped comm path
# (`*/cmux.app/Contents/MacOS/cmux`, any install location) when lsappinfo
# could not resolve one. Stops at launchd (ppid 1), where a tmux server that
# was started from a cmux tab has already reparented - ancestry can never
# false-positive from inside tmux.
mx_backend_detect_cmux_app_is_ancestor() {
  local cmux_pid pid ppid comm hops=0
  cmux_pid=$(mx_backend_detect_cmux_app_pid) || cmux_pid=""
  pid=$$
  while [ "$hops" -lt 32 ]; do
    if [ -n "$cmux_pid" ] && [ "$pid" = "$cmux_pid" ]; then
      return 0
    fi
    comm=$(ps -o comm= -p "$pid" 2>/dev/null) || comm=""
    comm="${comm#"${comm%%[![:space:]]*}"}"
    comm="${comm%"${comm##*[![:space:]]}"}"
    [ -n "$comm" ] || return 1
    case "$comm" in
      */cmux.app/Contents/MacOS/cmux) return 0 ;;
    esac
    ppid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d '[:space:]')
    case "$ppid" in ''|*[!0-9]*) return 1 ;; esac
    [ "$ppid" -gt 1 ] || return 1
    pid=$ppid
    hops=$((hops + 1))
  done
  return 1
}

# mx_backend_name: resolve the ACTIVE backend for a NEW spawn, absent an
# explicit per-task override. Precedence: MX_BACKEND env, then config/backend
# (a single word on its first non-empty line, mirroring config/actor-harness),
# then runtime auto-detection (mx_backend_detect), then default tmux. A
# per-task `--backend` flag is parsed by the caller (mx-spawn.sh) and takes
# precedence over this resolution entirely; it is not read here. Auto-detect
# fires only when nothing was explicitly configured, so an explicit setting
# always wins. Selecting herdr or cmux via auto-detect prints one loud stderr
# notice (both are experimental); auto-detecting tmux stays silent - it is
# today's default-path behavior and callers must see zero change. The cmux
# notice names the winning signal, so a fallback-detected cmux (bundle id or
# ancestry, after the claude wrapper stripped CMUX_WORKSPACE_ID) is visibly
# distinct from the primary-marker case.
mx_backend_name() {
  local line v detected marker
  if [ -n "${MX_BACKEND:-}" ]; then
    printf '%s' "$MX_BACKEND"
    return 0
  fi
  if [ -f "$MX_BACKEND_CONFIG_DIR/backend" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
      v=$(printf '%s' "$line" | tr -d '[:space:]')
      if [ -n "$v" ]; then
        printf '%s' "$v"
        return 0
      fi
    done < "$MX_BACKEND_CONFIG_DIR/backend"
  fi
  # Called directly (not in a command substitution) so the detect signal
  # globals survive into the notice below.
  if mx_backend_detect >/dev/null; then
    detected=$MX_BACKEND_DETECTED
    if [ "$detected" = herdr ]; then
      echo "NOTICE: auto-detected herdr runtime (HERDR_ENV=1) - spawning into the EXPERIMENTAL herdr backend. Set config/backend or pass --backend tmux to opt out." >&2
    fi
    if [ "$detected" = cmux ]; then
      case "$MX_BACKEND_DETECT_SIGNAL" in
        bundle-id) marker="FALLBACK signal __CFBundleIdentifier=$MX_BACKEND_CMUX_BUNDLE_ID; CMUX_WORKSPACE_ID absent, stripped by cmux's bundled claude wrapper" ;;
        ancestry) marker="FALLBACK signal process-ancestry reaching the running cmux app; CMUX_WORKSPACE_ID absent, stripped by cmux's bundled claude wrapper" ;;
        *) marker="CMUX_WORKSPACE_ID" ;;
      esac
      echo "NOTICE: auto-detected cmux runtime ($marker) - spawning into the EXPERIMENTAL cmux backend. Set config/backend or pass --backend tmux to opt out." >&2
    fi
    printf '%s' "$detected"
    return 0
  fi
  printf 'tmux'
}

# mx_backend_validate: refuse an unknown backend LOUDLY. Silent on success.
mx_backend_validate() {  # <name>
  local name=$1
  if ! mx_backend_is_known "$name"; then
    echo "error: unknown backend '$name' (known: $MX_BACKEND_KNOWN)" >&2
    return 1
  fi
  return 0
}

mx_backend_validate_spawn() {  # <name>
  local name=$1
  mx_backend_validate "$name" || return 1
  mx_backend_list_contains "$MX_BACKEND_SPAWN" "$name" && return 0
  echo "error: backend '$name' does not support task spawning yet (spawn-supported: $MX_BACKEND_SPAWN)" >&2
  return 1
}

# mx_backend_required_tools: the backend-SPECIFIC CLI tools a Multplx home on
# <backend> genuinely requires, beyond broker's universal toolchain (owned by
# docs/configuration.md "Toolchain" and bootstrap's COMMON list). This is the
# single owner of the per-backend dependency delta, so bootstrap follows the
# RESOLVED backend instead of demanding an inactive backend's tools. Each set is:
#   - the session-provider CLI itself (tmux/herdr/cmux);
#   - jq, for the JSON-emitting experimental adapters (herdr, cmux) whose
#     spawn/liveness paths parse the backend's JSON output (see each adapter's
#     tool check, e.g. mx_backend_herdr_tool_check);
# Treehouse is deliberately absent from this backend delta because every
# supported backend delegates worktree acquisition to it; bootstrap owns that
# unconditional requirement in its universal toolchain.
# Prints a single space-separated line and returns 0 for a known backend; returns
# 1 and prints nothing for an unknown backend.
mx_backend_required_tools() {  # <backend>
  case "$1" in
    tmux)   printf '%s' 'tmux' ;;
    herdr)  printf '%s' 'herdr jq' ;;
    cmux)   printf '%s' 'cmux jq' ;;
    *) return 1 ;;
  esac
}

mx_backend_required_tool_available() {  # <backend> <tool>
  local backend=$1 tool=$2 required
  required=$(mx_backend_required_tools "$backend") || return 1
  mx_backend_list_contains "$required" "$tool" || return 1
  case "$backend:$tool" in
    cmux:cmux)
      mx_backend_source cmux >/dev/null 2>&1 || return 1
      mx_backend_cmux_bin >/dev/null 2>&1
      ;;
    *) command -v "$tool" >/dev/null 2>&1 ;;
  esac
}

# mx_meta_get: the LAST value of `key=` in <meta-file>, or empty (never
# errors) if the file or key is absent. Mirrors the ad hoc `grep '^key=' |
# tail -1 | cut -d= -f2-` snippet every mx-*.sh script used to repeat inline.
mx_meta_get() {  # <meta-file> <key>
  local meta=$1 key=$2
  [ -f "$meta" ] || return 0
  grep "^$key=" "$meta" 2>/dev/null | tail -1 | cut -d= -f2- || true
}

# mx_backend_of_meta: the backend recorded in <meta-file>, defaulting to
# `tmux` when the field is absent - the P1 compatibility contract.
mx_backend_of_meta() {  # <meta-file>
  local v
  v=$(mx_meta_get "$1" backend)
  printf '%s' "${v:-tmux}"
}

mx_backend_target_of_meta() {  # <meta-file>
  local meta=$1 window
  window=$(mx_meta_get "$meta" window)
  [ -n "$window" ] && printf '%s' "$window"
}

mx_backend_meta_for_window() {  # <target> <state-dir>
  local target=$1 state=$2 meta window
  for meta in "$state"/*.meta; do
    [ -e "$meta" ] || continue
    window=$(mx_meta_get "$meta" window)
    [ -n "$window" ] && [ "$window" = "$target" ] || continue
    printf '%s' "$meta"
    return 0
  done
  return 1
}

mx_backend_task_id_for_selector() {  # <raw-target> <state-dir>
  local raw=$1 state=$2 id
  case "$raw" in
    *:*) return 1 ;;
  esac
  if [ -f "$state/$raw.meta" ]; then
    printf '%s' "$raw"
    return 0
  fi
  case "$raw" in
    mx-*)
      id=${raw#mx-}
      [ -f "$state/$id.meta" ] || return 1
      printf '%s' "$id"
      return 0
      ;;
  esac
  return 1
}

mx_backend_meta_for_selector() {  # <raw-target> <state-dir>
  local raw=$1 state=$2 id
  id=$(mx_backend_task_id_for_selector "$raw" "$state") || return 1
  printf '%s/%s.meta' "$state" "$id"
}

mx_backend_of_selector() {  # <raw-target> <resolved-target> <state-dir>
  local raw=$1 resolved=$2 state=$3 meta
  meta=$(mx_backend_meta_for_selector "$raw" "$state" 2>/dev/null || true)
  [ -n "$meta" ] && { mx_backend_of_meta "$meta"; return 0; }
  if [ -n "$resolved" ]; then
    meta=$(mx_backend_meta_for_window "$resolved" "$state" 2>/dev/null || true)
    [ -n "$meta" ] && { mx_backend_of_meta "$meta"; return 0; }
  fi
  printf 'tmux'
}

mx_backend_expected_label_of_selector() {  # <raw-target> <state-dir>
  local raw=$1 state=$2 id
  id=$(mx_backend_task_id_for_selector "$raw" "$state" 2>/dev/null || true)
  [ -n "$id" ] && printf 'mx-%s' "$id"
  return 0
}

# mx_backend_source: source the named backend's adapter file, once per shell.
# Each adapter is an independently linted canonical root. The /dev/null source
# boundaries keep runtime dispatch from importing all three adapter ASTs into
# every dispatcher consumer while preserving the runtime source operations.
mx_backend_source() {  # <name>
  local name=$1
  mx_backend_validate "$name" || return 1
  case "$name" in
    tmux)
      if [ -z "${_MX_BACKEND_TMUX_SOURCED:-}" ]; then
        case "$MX_BACKEND_IMPLEMENTATION_SELECTED" in
          rust)
            # shellcheck source=/dev/null
            . "$MX_BACKEND_LIB_DIR/backends/tmux-rust.sh" || return 1
            ;;
          legacy)
            # shellcheck source=/dev/null
            . "$MX_BACKEND_LIB_DIR/backends/tmux.sh" || return 1
            ;;
        esac
        _MX_BACKEND_TMUX_SOURCED=1
      fi
      ;;
    herdr)
      if [ -z "${_MX_BACKEND_HERDR_SOURCED:-}" ]; then
        # shellcheck source=/dev/null
        . "$MX_BACKEND_LIB_DIR/backends/herdr.sh" || return 1
        _MX_BACKEND_HERDR_SOURCED=1
      fi
      ;;
    cmux)
      if [ -z "${_MX_BACKEND_CMUX_SOURCED:-}" ]; then
        # shellcheck source=/dev/null
        . "$MX_BACKEND_LIB_DIR/backends/cmux.sh" || return 1
        _MX_BACKEND_CMUX_SOURCED=1
      fi
      ;;
  esac
}

# mx_backend_resolve_selector: resolve a raw mx-send.sh/mx-peek.sh style
# selector to a live session-provider target. Four forms, in order:
#   target with ":"   used as-is (the escape hatch for a window/pane outside
#                      this Multplx home) - backend-independent, a literal string.
#   exact task id      routed through <state-dir>/<id>.meta's backend target
#                      (`window=`) -
#                      backend-independent, a stored value, NOT re-verified
#                      against a live backend inventory (matches today's
#                      behavior: tmux window names can be trusted from meta
#                      without a live re-check).
#   "mx-<id>"          legacy task window label fallback routed through
#                      <state-dir>/<id>.meta when no exact
#                      <state-dir>/mx-<id>.meta exists.
#   anything else      first matched against recorded `window=`
#                      metadata, then treated as an ad hoc bare window name and
#                      resolved by searching the legacy tmux live inventory.
mx_backend_resolve_selector() {  # <raw-target> <state-dir>
  local raw=$1 state=$2 meta window
  case "$raw" in
    *:*)
      printf '%s' "$raw"
      return 0
      ;;
  esac
  meta=$(mx_backend_meta_for_selector "$raw" "$state" 2>/dev/null || true)
  if [ -n "$meta" ]; then
    window=$(mx_backend_target_of_meta "$meta")
    [ -n "$window" ] || { echo "error: no backend target recorded in $meta" >&2; return 1; }
    printf '%s' "$window"
    return 0
  fi
  case "$raw" in
    mx-*)
      echo "error: no metadata for $raw in $state; pass session:window to target a window outside this Multplx home" >&2
      return 1
      ;;
    *)
      meta=$(mx_backend_meta_for_window "$raw" "$state" 2>/dev/null || true)
      if [ -n "$meta" ]; then
        window=$(mx_backend_target_of_meta "$meta")
        [ -n "$window" ] || { echo "error: no backend target recorded in $meta" >&2; return 1; }
        printf '%s' "$window"
        return 0
      fi
      mx_backend_source tmux || return 1
      mx_backend_tmux_resolve_bare_selector "$raw"
      ;;
  esac
}

# --- generic per-op dispatch -------------------------------------------------
#
# Thin case-dispatch wrappers so a caller names an operation and a backend
# rather than hand-writing `case "$backend" in tmux) mx_backend_tmux_x ;; esac`
# at every call site. Each verified backend adds its own arm here, without
# changing call sites.

# mx_backend_capture: bounded plain-text session capture.
mx_backend_capture() {  # <backend> <target> <lines> [expected-label]
  local backend=$1
  shift
  mx_backend_source "$backend" || return 1
  case "$backend" in
    tmux) mx_backend_tmux_capture "$@" ;;
    herdr) mx_backend_herdr_capture "$@" ;;
    cmux) mx_backend_cmux_capture "$@" ;;
    *) echo "error: no capture implementation for backend '$backend'" >&2; return 1 ;;
  esac
}

# mx_backend_send_key: one backend-supported named special key.
mx_backend_send_key() {  # <backend> <target> <key> [expected-label]
  local backend=$1
  shift
  mx_backend_source "$backend" || return 1
  case "$backend" in
    tmux) mx_backend_tmux_send_key "$@" ;;
    herdr) mx_backend_herdr_send_key "$@" ;;
    cmux) mx_backend_cmux_send_key "$@" ;;
    *) echo "error: no send-key implementation for backend '$backend'" >&2; return 1 ;;
  esac
}

# mx_backend_send_text_submit: type text once, then submit and verify,
# retrying only the submission (never retyping). Echoes the verdict
# (empty|pending|unknown|send-failed for submit-verifying adapters).
mx_backend_send_text_submit() {  # <backend> <target> <text> <retries> <enter-sleep> <settle> [expected-label]
  local backend=$1
  shift
  mx_backend_source "$backend" || return 1
  case "$backend" in
    tmux) mx_backend_tmux_send_text_submit "$@" ;;
    herdr) mx_backend_herdr_send_text_submit "$@" ;;
    cmux) mx_backend_cmux_send_text_submit "$@" ;;
    *) echo "error: no send-text implementation for backend '$backend'" >&2; return 1 ;;
  esac
}

# mx_backend_kill: remove the task's session endpoint (best-effort; a
# nonexistent/already-gone target is not an error - callers already swallow
# failures here exactly as the inline `tmux kill-window ... || true` did).
mx_backend_kill() {  # <backend> <target>
  local backend=$1
  shift
  mx_backend_source "$backend" || return 1
  case "$backend" in
    tmux) mx_backend_tmux_kill "$@" ;;
    herdr) mx_backend_herdr_kill "$@" ;;
    cmux) mx_backend_cmux_kill "$@" ;;
    *) echo "error: no kill implementation for backend '$backend'" >&2; return 1 ;;
  esac
}

# mx_backend_busy_state: semantic busy/idle/unknown for backends that expose
# native agent-state (herdr-addendum "busy state" row - the first backend
# where this gets real semantics beyond pane-regex). Backends with no such
# primitive (tmux) report unknown. Callers own the fallback policy: mx-watch.sh
# uses unknown as the cue for its pane-hash + MX_BUSY_REGEX detection, while
# mx-actor-state.sh also corroborates native idle verdicts before treating a
# no-run actors as not busy.
mx_backend_busy_state() {  # <backend> <target>
  local backend=$1
  shift
  mx_backend_source "$backend" || { printf 'unknown'; return 0; }
  case "$backend" in
    herdr) mx_backend_herdr_busy_state "$@" ;;
    *) printf 'unknown' ;;
  esac
}

# mx_backend_native_state: the exact harness-native task state used by the
# shared signal-precedence resolver. Backends without a semantic state
# primitive contribute `unknown`; today only Herdr exposes this level read.
mx_backend_native_state() {  # <backend> <target>
  local backend=$1
  shift
  mx_backend_source "$backend" || { printf 'unknown'; return 0; }
  case "$backend" in
    herdr) mx_backend_herdr_native_state "$@" ;;
    *) printf 'unknown' ;;
  esac
}

# mx_backend_composer_state: classify the composer/input row of <target> as
# empty|pending|unknown for callers that need a pre-submit pending-input guard
# or an adapter's conservative submit fallback. It is exposed generically so a
# caller other than the send path (the away-mode daemon's supervisor-pane
# pending-input guard, bin/mx-supervise-daemon.sh) can ask the same question
# without duplicating per-backend composer-reading logic. tmux and herdr both
# expose a named classifier already (mx_tmux_composer_state,
# mx_backend_herdr_composer_state), as does cmux
# (mx_backend_cmux_composer_state); a backend with no named classifier
# reports unknown here - callers fall back to their own
# policy, exactly as an unknown mx_backend_busy_state already does.
mx_backend_composer_state() {  # <backend> <target> -> empty|pending|unknown
  local backend=$1
  shift
  mx_backend_source "$backend" || { printf 'unknown'; return 0; }
  case "$backend" in
    tmux) mx_tmux_composer_state "$@" ;;
    herdr) mx_backend_herdr_composer_state "$@" ;;
    cmux) mx_backend_cmux_composer_state "$@" ;;
    *) printf 'unknown' ;;
  esac
}

# mx_backend_target_exists: cheap, READ-ONLY existence check - does the
# recorded TARGET endpoint still exist on BACKEND? Never starts a server or
# session: for herdr this deliberately queries the pane directly instead of
# going through mx_backend_herdr_target_ready (which auto-starts the herdr
# server as a side effect via mx_backend_herdr_server_ensure - fine for an
# operation that is about to use the pane, wrong for a passive liveness
# probe). A gone tmux window or an unqueryable herdr pane (server down, pane
# closed) simply fails, which
# IS "does not exist" for this purpose.
# Mirrors mx-actor-state.sh's pane_readable check; exists here as one shared
# primitive so callers that only need a fast alive/dead read (recovery
# digests, the session-start system digest) do not re-derive it inline.
mx_backend_target_exists() {  # <backend> <target> [expected-label]
  local backend=$1 target=$2 expected_label=${3:-} session pane
  case "$backend" in
    tmux)
      if [ "$MX_BACKEND_IMPLEMENTATION_SELECTED" = rust ]; then
        mx_backend_source tmux >/dev/null 2>&1 || return 1
        mx_backend_tmux_target_ready "$target" >/dev/null 2>&1
      else
        tmux display-message -p -t "$target" '#{pane_id}' >/dev/null 2>&1
      fi
      ;;
    herdr)
      mx_backend_source herdr || return 1
      session=${target%%:*}
      pane=${target#*:}
      [ -n "$session" ] && [ -n "$pane" ] && [ "$pane" != "$target" ] || return 1
      # mx_backend_herdr_cli (not a raw HERDR_SESSION-only call): verified
      # empirically (docs/herdr-backend.md "Session targeting") that the bare
      # env var alone is NOT reliably honored once another herdr server is
      # already bound on the machine - it silently queries whatever server IS
      # running instead. mx_backend_herdr_cli appends the required --session
      # flag on top, so this check is correctly scoped even when the caller's
      # own ambient session (e.g. the primary broker's default session) is
      # a DIFFERENT one than the target's.
      mx_backend_herdr_cli "$session" pane get "$pane" >/dev/null 2>&1
      ;;
    cmux)
      mx_backend_source cmux || return 1
      mx_backend_cmux_target_ready "$target" "$expected_label"
      ;;
    *)
      return 1
      ;;
  esac
}

# mx_backend_agent_state: the single recovery-grade agent/endpoint state
# contract. It is deliberately richer than mx_backend_target_exists's cheap
# pane-presence read and prints exactly one of:
#   alive      - a verified harness agent is running.
#   dead       - the endpoint exists but confidently has no agent.
#   missing    - the recorded endpoint is authoritatively absent.
#   ambiguous  - the endpoint exists but its process cannot be attributed.
#   unreadable - a target or inventory read failed or contradicted itself.
#   unverified - this backend has no recovery classifier.
# Only `dead` and `missing` license recovery. The tmux adapter requires a
# successful session inventory and returns `missing` only when it omits the
# exact window; the Herdr adapter reuses its husk
# classifier. cmux does not support daemon spawns.
mx_backend_agent_state() {  # <backend> <target>
  local backend=$1 target=$2
  mx_backend_source "$backend" || { printf 'unverified'; return 0; }
  case "$backend" in
    tmux) mx_backend_tmux_agent_state "$target" ;;
    herdr) mx_backend_herdr_agent_state "$target" ;;
    *) printf 'unverified' ;;
  esac
}

# Backward-compatible three-state view for existing callers. An
# authoritatively missing endpoint is confidently not a live agent, while every
# ambiguous, unreadable, or unverified result stays unknown.
mx_backend_agent_alive() {  # <backend> <target>
  case "$(mx_backend_agent_state "$1" "$2")" in
    alive) printf 'alive' ;;
    dead|missing) printf 'dead' ;;
    *) printf 'unknown' ;;
  esac
}

# --- native event push (backend-extensible) ---------------------------------
#
# The watcher's event-wait splice (bin/mx-watch.sh) is backend-agnostic: it asks
# mx_backend_has_push whether a window's backend can push semantic state changes,
# and for those backends replaces its blind `sleep POLL` with a bounded wait on
# mx_backend_wait_transition. Every push-capable backend reuses the shared
# normalized-transition shape and policy table (bin/mx-transition-lib.sh); today
# only herdr implements the surface (docs/herdr-backend.md "Native
# pane.agent_status_changed push escalation"). A backend with no native push
# reports has-push false and returns 2 from the dispatchers below, so the
# watcher falls back to its poll loop - the permanent fail-closed backstop.

# mx_backend_has_push: 0 if <backend> exposes a native transition push stream.
mx_backend_has_push() {  # <backend>
  case "$1" in
    herdr) return 0 ;;
    *) return 1 ;;
  esac
}

# mx_backend_events_capable: 0 if <backend>'s push path is usable for <session>
# right now (version/schema/reader gate). Non-push backends are never capable.
# The watcher memoizes this per session so the potentially heavy capability
# probe is not repeated every poll.
mx_backend_events_capable() {  # <backend> <session>
  local backend=$1
  shift
  mx_backend_has_push "$backend" || return 1
  mx_backend_source "$backend" || return 1
  case "$backend" in
    herdr) mx_backend_herdr_events_capable "$@" ;;
    *) return 1 ;;
  esac
}

# mx_backend_wait_transition: bounded wait for a fresh actionable (blocked)
# transition on one of <pane_window...> in <session>, up to <timeout_secs>.
# Prints the normalized transition record and returns 0 on a fresh actionable
# edge; returns 1 on a clean timeout (the caller has effectively already slept);
# returns 2 when the event path is unusable (the caller sleeps the budget
# itself). Non-push backends always return 2.
mx_backend_wait_transition() {  # <backend> <session> <timeout_secs> <state_dir> <pane_window...>
  local backend=$1
  shift
  mx_backend_has_push "$backend" || return 2
  mx_backend_source "$backend" || return 2
  case "$backend" in
    herdr) mx_backend_herdr_wait_transition "$@" ;;
    *) return 2 ;;
  esac
}

mx_backend_commit_transition() {  # <backend> <state_dir> <session> <record>
  local backend=$1
  shift
  mx_backend_has_push "$backend" || return 1
  mx_backend_source "$backend" || return 1
  case "$backend" in
    herdr) mx_backend_herdr_commit_transition "$@" ;;
    *) return 1 ;;
  esac
}

mx_backend_clear_transition() {  # <backend> <state_dir> <window>
  local backend=$1
  shift
  mx_backend_has_push "$backend" || return 0
  mx_backend_source "$backend" || return 1
  case "$backend" in
    herdr) mx_backend_herdr_clear_transition "$@" ;;
    *) return 0 ;;
  esac
}
