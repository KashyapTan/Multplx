#!/usr/bin/env bash
# Bootstrap detection, best-effort system refresh/prune, and installs.
# Usage: mx-bootstrap.sh
#          Detect: prints one line per actionable problem, or an explicit
#          BOOTSTRAP_INFO no-action fact for completed benign bootstrap work, and
#          exits 0.
#          Silent = all good.
#          Lines: "MISSING: <tool> (install: <command>)",
#                 "MISSING_MANUAL: <tool> (instructions: <url>)",
#                 "BACKEND_INVALID: <name> (known: <names>)",
#                 "VPLAN_INVALID: bundled mx-vplan.sh self-check failed",
#                 "ACTOR_DISPATCH: invalid config/actor-dispatch.json - <reason>",
#                 "SYSTEM_SYNC: <repo>: skipped|recovered|STUCK: <detail>",
#                 "PR_CHECK_MIGRATION: <private remediation>",
#                 "TANGLE: <remediation>",
#                 "DAEMON_SYNC: daemon <id>: skipped: <reason>",
#                 "NUDGE_DAEMONS: daemon <id>: send failed: <reason>",
#                 "BOOTSTRAP_INFO: nudged mx-<id> with '<message>'",
#                 "DAEMON_LIVENESS: daemon <id>: skipped: <reason>|respawn failed after <cause>: <reason>".
#          When a RUNNING daemon worktree is fast-forwarded to broker's
#          own current default-branch commit (a purely LOCAL fast-forward, never
#          an origin fetch) AND its loaded instruction surface (AGENTS.md, bin/,
#          or .agents/skills/) actually changed, bootstrap immediately nudges it
#          via MX_HOME=<active-home> bin/mx-send.sh mx-<id> so meta resolves the
#          current backend target and the standard from-broker marker is
#          applied. A successful send prints one BOOTSTRAP_INFO line with the
#          exact target and message sent; a failed send leaves an idempotent
#          retry marker under state/.daemon-nudge-pending/ and prints an
#          actionable NUDGE_DAEMONS line.
#          Already-current or no-instruction-change homes are silently left alone.
#          The daemon sweep also propagates declared inherited local material
#          into each validated live daemon home.
#          DAEMON_SYNC lines report actionable skipped local-HEAD syncs or
#          inheritance failures for live daemon homes, plus quarantine
#          diagnostics for divergent shared maintainer-preference copies;
#          no-op/current and successful updates stay quiet.
#          DAEMON_LIVENESS lines report only actionable failures from the
#          recovery-grade state owned by bin/mx-backend.sh's
#          mx_backend_agent_state: skipped distinguishes an existing ambiguous
#          process, an unreadable target, and an unverified backend; respawn
#          failed names whether the endpoint was missing or agent-less.
#          Already-live and successfully relaunched daemons are silent
#          unless MX_BOOTSTRAP_VERBOSE_FACTS=1 requests BOOTSTRAP_INFO facts.
#          A TANGLE line means the broker primary checkout (MX_ROOT) is stranded
#          on a feature branch instead of its default branch - an actor's work
#          landed in the primary instead of its own worktree; restore it per the line.
#          treehouse is also MISSING when its installed version lacks
#          "treehouse get --lease" support.
#          The bundled mx-headroom.sh is self-checked instead of requiring an
#          external quota wrapper. The owned backlog library ships with the
#          repo and needs no presence or version probe.
#          The bundled mx-vplan.sh and its vendored review assets are
#          self-checked instead of requiring an external rich-review tool.
#          System sync fetches, fast-forwards safe default-branch states, reports
#          recovered and STUCK clone drift, and prunes gone local branches; it is
#          bounded by MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT when it is a non-empty
#          numeric override, while non-numeric values fall back to 20s.
#          When the override is unset or blank, the timeout is
#          max(20, 5 + 3 * origin-backed project clone count). A timed-out
#          refresh relays any completed mx-system-sync.sh output before the
#          aggregate timeout skip line with timeout and elapsed seconds.
#          Set MX_SYSTEM_PRUNE=0 to skip branch pruning during that refresh.
#          Set MX_BOOTSTRAP_DETECT_ONLY=1 to skip the four MUTATING sweeps
#          (PR-check migration, daemon_sync, daemon_liveness_sweep,
#          system_sync) while still printing every read-only detect line
#          above; the TANGLE line switches to advisory-only wording with no
#          checkout command. Used by
#          mx-session-start.sh's read-only path when another live session holds
#          the system lock, so a second concurrent session never race-mutates
#          PR-check artifacts, daemon homes, project
#          clones, or repair instructions.
#          Unset/0 (the default) runs every sweep exactly as before - this flag
#          is purely additive.
#        mx-bootstrap.sh install <tool>...
#          Install the named tools (only ones the maintainer approved).
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
PROJECTS="${MX_PROJECTS_OVERRIDE:-$MX_HOME/projects}"
CONFIG="${MX_CONFIG_OVERRIDE:-$MX_HOME/config}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
# shellcheck source=bin/mx-backlog-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-backlog-lib.sh"
# shellcheck source=bin/mx-tangle-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-tangle-lib.sh"
# shellcheck source=bin/mx-ff-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-ff-lib.sh"
# shellcheck source=bin/mx-config-inherit-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-config-inherit-lib.sh"
# shellcheck source=bin/mx-backend.sh disable=SC1091
. "$SCRIPT_DIR/mx-backend.sh"

system_sync_origin_backed_project_count() {
  local count proj
  count=0
  [ -d "$PROJECTS" ] || { echo 0; return 0; }
  for proj in "$PROJECTS"/*; do
    [ -d "$proj" ] || continue
    git -C "$proj" rev-parse --git-dir >/dev/null 2>&1 || continue
    git -C "$proj" remote get-url origin >/dev/null 2>&1 || continue
    count=$((count + 1))
  done
  echo "$count"
}

system_sync_bootstrap_timeout() {
  local count timeout
  if [ -n "${MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT:-}" ]; then
    case "$MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT" in
      *[!0-9]*) echo 20 ;;
      *) echo "$MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT" ;;
    esac
    return 0
  fi

  count=$(system_sync_origin_backed_project_count)
  timeout=$((5 + (3 * count)))
  [ "$timeout" -ge 20 ] || timeout=20
  echo "$timeout"
}

system_sync_relay_filtered_output() {
  local tmp=$1 line
  while IFS= read -r line; do
    case "$line" in
      *': skipped: local-only project') ;;
      *': skipped: no origin remote') ;;
      *': skipped:'*) echo "SYSTEM_SYNC: $line" ;;
      *': STUCK:'*) echo "SYSTEM_SYNC: $line" ;;
      *': recovered:'*) echo "SYSTEM_SYNC: $line" ;;
    esac
  done < "$tmp"
}

system_sync_relay_all_output() {
  local tmp=$1 line
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    echo "SYSTEM_SYNC: $line"
  done < "$tmp"
}

system_sync() {
  [ -x "$MX_ROOT/bin/mx-system-sync.sh" ] || return 0
  [ -d "$PROJECTS" ] || return 0

  tmp=$(mktemp "${TMPDIR:-/tmp}/mx-system-sync.XXXXXX" 2>/dev/null) || return 0
  timeout=$(system_sync_bootstrap_timeout)
  monitor_was_on=0
  case $- in *m*) monitor_was_on=1 ;; esac
  set -m 2>/dev/null || true
  "$MX_ROOT/bin/mx-system-sync.sh" >"$tmp" 2>/dev/null &
  pid=$!

  start=$SECONDS
  while jobs -r -p | grep -qx "$pid"; do
    elapsed=$((SECONDS - start))
    if [ "$elapsed" -ge "$timeout" ]; then
      kill -TERM "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      [ "$monitor_was_on" -eq 1 ] || set +m 2>/dev/null || true
      system_sync_relay_all_output "$tmp"
      echo "SYSTEM_SYNC: system: skipped: bootstrap refresh timed out (timeout=${timeout}s elapsed=${elapsed}s)"
      rm -f "$tmp"
      return 0
    fi
    sleep 1
  done
  wait "$pid" 2>/dev/null || true
  [ "$monitor_was_on" -eq 1 ] || set +m 2>/dev/null || true

  system_sync_relay_filtered_output "$tmp"
  rm -f "$tmp"
}

daemon_sync() {
  # shellcheck source=bin/mx-wake-lib.sh disable=SC1091
  . "$SCRIPT_DIR/mx-wake-lib.sh"
  # Local-HEAD daemon sync: fast-forward every LIVE daemon home
  # to the primary checkout's current default-branch commit. Purely LOCAL - no
  # fetch, no origin dependency: a linked-worktree home already holds the primary's
  # commit (mx-ff-lib.sh), while a standalone clone without it is skipped until
  # /updatemultplx refreshes it from origin. Startup sends reread nudges only
  # for RUNNING daemons whose instruction surface (AGENTS.md, bin/, or
  # .agents/skills/) actually changed, so a daemon already on the primary's
  # version is never disturbed (AGENTS.md bootstrap + supervision). Unlike
  # /updatemultplx, startup owns the live-convergence send itself because it is
  # a deterministic locked sweep and can report success as BOOTSTRAP_INFO while
  # preserving failed sends as NUDGE_DAEMONS retry markers.
  [ -d "$STATE" ] || return 0
  local primary_head
  if ! primary_head=$(primary_head_commit "$MX_ROOT"); then
    local meta id
    for meta in "$STATE"/*.meta; do
      [ -f "$meta" ] || continue
      grep -q '^kind=daemon' "$meta" 2>/dev/null || continue
      id=$(basename "$meta" .meta)
      echo "DAEMON_SYNC: daemon $id: skipped: primary default-branch commit cannot be resolved"
    done
    return 0
  fi
  FF_NUDGE_WINDOWS=""
  FF_SEEN_HOMES=""
  DAEMON_NUDGE_MESSAGE='broker was updated to the latest - please re-read your AGENTS.md to pick up the new instructions.'
  DAEMON_NUDGE_PENDING_DIR="$STATE/.daemon-nudge-pending"

  daemon_nudge_marker_path() {
    case "$1" in
      *[!/A-Za-z0-9._-]*|""|*/*) return 1 ;;
    esac
    printf '%s/%s.pending' "$DAEMON_NUDGE_PENDING_DIR" "$1"
  }

  daemon_write_nudge_marker() {
    local id=$1 home=$2 commit=$3 instr=$4 selector marker tmp parent
    selector="mx-$id"
    marker=$(daemon_nudge_marker_path "$id") || return 1
    parent=${marker%/*}
    mkdir -p "$parent" || return 1
    tmp=$(mktemp "$parent/.nudge.XXXXXX" 2>/dev/null) || return 1
    {
      printf 'id=%s\n' "$id"
      printf 'selector=%s\n' "$selector"
      printf 'home=%s\n' "$home"
      printf 'commit=%s\n' "$commit"
      printf 'instructions=%s\n' "$instr"
      printf 'message=%s\n' "$DAEMON_NUDGE_MESSAGE"
    } > "$tmp" || { rm -f "$tmp"; return 1; }
    mv -f "$tmp" "$marker" || { rm -f "$tmp"; return 1; }
  }

  daemon_send_nudge() {
    local id=$1 home=$2 commit=$3 instr=$4 selector marker out
    selector="mx-$id"
    marker=$(daemon_nudge_marker_path "$id") || {
      echo "NUDGE_DAEMONS: daemon $id: send failed: unsafe id"
      return 0
    }
    if ! daemon_write_nudge_marker "$id" "$home" "$commit" "$instr"; then
      echo "NUDGE_DAEMONS: daemon $id: send failed: cannot record retry marker"
      return 0
    fi
    if out=$(MX_HOME="$MX_HOME" MX_ROOT_OVERRIDE="$MX_ROOT" MX_STATE_OVERRIDE="$STATE" "$SCRIPT_DIR/mx-send.sh" "$selector" "$DAEMON_NUDGE_MESSAGE" 2>&1); then
      rm -f "$marker"
      echo "BOOTSTRAP_INFO: nudged $selector with '$DAEMON_NUDGE_MESSAGE'"
    else
      echo "NUDGE_DAEMONS: daemon $id: send failed: $(first_line "$out")"
    fi
  }

  mx_ff_after_instruction_update() {
    local id=$1 home=$2 _window=$3 instr=$4
    daemon_send_nudge "$id" "$home" "$primary_head" "$instr"
  }

  daemon_retry_pending_nudges() {
    local marker id selector home commit message expected_marker meta meta_home home_real head
    [ -d "$DAEMON_NUDGE_PENDING_DIR" ] || return 0
    for marker in "$DAEMON_NUDGE_PENDING_DIR"/*.pending; do
      [ -f "$marker" ] || continue
      id=$(mx_meta_get "$marker" id)
      if ! expected_marker=$(daemon_nudge_marker_path "$id"); then
        echo "NUDGE_DAEMONS: daemon ${id:-unknown}: send failed: retry marker has unsafe id"
        continue
      fi
      [ "$expected_marker" = "$marker" ] || {
        echo "NUDGE_DAEMONS: daemon $id: send failed: retry marker filename mismatch"
        continue
      }
      selector=$(mx_meta_get "$marker" selector)
      home=$(mx_meta_get "$marker" home)
      commit=$(mx_meta_get "$marker" commit)
      message=$(mx_meta_get "$marker" message)
      [ "$selector" = "mx-$id" ] || {
        echo "NUDGE_DAEMONS: daemon ${id:-unknown}: send failed: retry marker selector mismatch"
        continue
      }
      [ "$message" = "$DAEMON_NUDGE_MESSAGE" ] || {
        echo "NUDGE_DAEMONS: daemon ${id:-unknown}: send failed: retry marker message mismatch"
        continue
      }
      meta="$STATE/$id.meta"
      [ -f "$meta" ] && [ "$(mx_meta_get "$meta" kind)" = daemon ] || {
        echo "NUDGE_DAEMONS: daemon ${id:-unknown}: send failed: retry target has no live daemon metadata"
        continue
      }
      meta_home=$(mx_meta_get "$meta" home)
      [ -n "$meta_home" ] || meta_home=$(daemon_registry_field "$DATA/daemons.md" "$id" home || true)
      if ! validate_daemon_home "$id" "$meta_home"; then
        echo "NUDGE_DAEMONS: daemon $id: send failed: retry target home unsafe: $VALIDATION_ERROR"
        continue
      fi
      home_real="$VALIDATED_HOME"
      [ "$home_real" = "$home" ] || {
        echo "NUDGE_DAEMONS: daemon $id: send failed: retry target home changed"
        continue
      }
      head=$(git -C "$home_real" rev-parse HEAD 2>/dev/null || true)
      [ -n "$head" ] && [ "$head" = "$commit" ] || {
        echo "NUDGE_DAEMONS: daemon $id: send failed: retry target is not at recorded instruction commit"
        continue
      }
      if out=$(MX_HOME="$MX_HOME" MX_ROOT_OVERRIDE="$MX_ROOT" MX_STATE_OVERRIDE="$STATE" "$SCRIPT_DIR/mx-send.sh" "$selector" "$DAEMON_NUDGE_MESSAGE" 2>&1); then
        rm -f "$marker"
        echo "BOOTSTRAP_INFO: nudged $selector with '$DAEMON_NUDGE_MESSAGE'"
      else
        echo "NUDGE_DAEMONS: daemon $id: send failed: $(first_line "$out")"
      fi
    done
  }

  local tmp line
  daemon_retry_pending_nudges
  tmp=$(mktemp "${TMPDIR:-/tmp}/mx-daemon-sync.XXXXXX" 2>/dev/null) || return 0
  sweep_live_daemon_metas "$STATE" "$primary_head" yes "$DATA/daemons.md" >"$tmp"
  while IFS= read -r line; do
    case "$line" in
      daemon\ *': skipped:'*) echo "DAEMON_SYNC: $line" ;;
      BOOTSTRAP_INFO:\ *) echo "$line" ;;
      NUDGE_DAEMONS:\ *) echo "$line" ;;
    esac
  done < "$tmp"
  rm -f "$tmp"
  unset -f mx_ff_after_instruction_update
  # Inheritance propagation: push the primary-authoritative local inheritance
  # surface into every VALIDATED live daemon home swept above.
  # FF_SEEN_HOMES is exactly that set, and mx-config-inherit-lib.sh owns the
  # declared config items plus data/maintainer-shared.md.
  # After a successful push that changes allowlisted config/* for an already-
  # running home, send its literal-content reread instruction pointer so the
  # live agent does not keep applying stale defaults. Spawn/respawn already
  # re-reads at launch and needs no redundant nudge unless files changed after launch.
  local id home home_real home_lock propagated_homes report reread_out reread_skip_pending
  propagated_homes=""
  DAEMON_RESPAWNED_IDS=${DAEMON_RESPAWNED_IDS:-}
  while IFS='|' read -r id home _window _meta; do
    validate_daemon_home "$id" "$home" || continue
    home_real="$VALIDATED_HOME"
    case " $FF_SEEN_HOMES " in
      *" $home_real "*) ;;
      *) continue ;;
    esac
    case " $propagated_homes " in
      *" $home_real "*) continue ;;
    esac
    propagated_homes="$propagated_homes $home_real"
    mkdir -p "$home_real/state" || {
      echo "CONFIG_REREAD: daemon $id: send failed: could not create state directory"
      continue
    }
    home_lock=$(mx_config_inherit_lock_path "$home_real") || {
      echo "CONFIG_REREAD: daemon $id: send failed: could not resolve per-home lock"
      continue
    }
    mx_lock_acquire_wait "$home_lock" || {
      echo "CONFIG_REREAD: daemon $id: send failed: could not acquire per-home lock"
      continue
    }
    reread_skip_pending=0
    case " $DAEMON_RESPAWNED_IDS " in
      *" $id "*) reread_skip_pending=1 ;;
    esac
    if [ "$reread_skip_pending" -eq 0 ] \
      && mx_config_reread_retry_queue_is_full "$MX_HOME" "$id"; then
      mx_config_reread_retry_pending "$id" "$home_real" || true
      if mx_config_reread_retry_queue_is_full "$MX_HOME" "$id"; then
        echo "CONFIG_REREAD: daemon $id: send failed: retry instruction queue is full"
        mx_lock_release "$home_lock" || true
        continue
      fi
    fi
    report=$(mktemp "${TMPDIR:-/tmp}/mx-bootstrap-inherit.XXXXXX" 2>/dev/null) || {
      echo "DAEMON_SYNC: daemon $id: skipped: inheritance failed"
      mx_lock_release "$home_lock" || true
      continue
    }
    if MX_CONFIG_INHERIT_REPORT="$report" \
      propagate_daemon_inheritance "$MX_HOME" "$home_real" "$CONFIG" "$DATA"; then
      :
    else
      echo "DAEMON_SYNC: daemon $id: skipped: inheritance failed"
    fi
    if ! reread_out=$(MX_HOME="$MX_HOME" MX_ROOT_OVERRIDE="$MX_ROOT" \
      MX_STATE_OVERRIDE="$STATE" \
      MX_CONFIG_REREAD_SKIP_PENDING="$reread_skip_pending" \
      mx_config_send_reread_nudge "$id" "$home_real" "$report" 2>&1); then
      if [ -n "$reread_out" ]; then
        printf '%s\n' "$reread_out"
      else
        echo "CONFIG_REREAD: daemon $id: send failed: unknown error"
      fi
    elif [ -n "$reread_out" ]; then
      printf '%s\n' "$reread_out"
    fi
    rm -f "$report"
    mx_lock_release "$home_lock" || true
  done < <(live_daemon_meta_records "$STATE" "$DATA/daemons.md")
  return 0
}

daemon_liveness_sweep() {
  # Idempotent daemon liveness guarantee - SESSION START ONLY. The detailed
  # state machine and its only recovery-authorizing states are owned by
  # mx_backend_agent_state. A missing tmux pane is not enough: tmux must prove
  # the window or session absent. This preserves duplicate prevention for
  # existing ambiguous processes and every transiently unreadable target while
  # adding the missing-session path the original bare-shell and Herdr-husk sweep
  # lacked.
  # A meta with no window remains owned by daemon-provisioning recovery.
  # Daemon homes never contain kind=daemon meta, so this is naturally a
  # primary-only no-op there. Mid-session liveness remains explicitly out of
  # scope and requires a separate periodic signal.
  [ -d "$STATE" ] || return 0
  local meta id window harness backend target agent_state out cause
  DAEMON_RESPAWNED_IDS=""
  for meta in "$STATE"/*.meta; do
    [ -f "$meta" ] || continue
    grep -q '^kind=daemon$' "$meta" 2>/dev/null || continue
    id=$(basename "$meta" .meta)
    window=$(mx_meta_get "$meta" window)
    [ -n "$window" ] || continue
    harness=$(mx_meta_get "$meta" harness)
    backend=$(mx_backend_of_meta "$meta")
    target=$(mx_backend_target_of_meta "$meta")
    [ -n "$target" ] || target="$window"
    agent_state=$(mx_backend_agent_state "$backend" "$target" 2>/dev/null) || agent_state=unreadable
    case "$harness" in
      claude|codex|pi) ;;
      *)
        case "$agent_state" in dead|missing) agent_state=unverified-harness ;; esac
        ;;
    esac
    case "$agent_state" in
      alive)
        if [ "${MX_BOOTSTRAP_VERBOSE_FACTS:-0}" = 1 ]; then
          echo "BOOTSTRAP_INFO: daemon $id already live (backend=$backend)"
        fi
        ;;
      dead|missing)
        if [ "$agent_state" = dead ]; then
          cause="confirmed agent absence on existing endpoint"
          mx_backend_kill "$backend" "$target" 2>/dev/null || true
        else
          cause="recorded endpoint confidently missing"
        fi
        if out=$(MX_SPAWN_NO_GUARD=1 "$MX_ROOT/bin/mx-spawn.sh" "$id" --daemon 2>&1); then
          DAEMON_RESPAWNED_IDS="$DAEMON_RESPAWNED_IDS $id"
          if [ "${MX_BOOTSTRAP_VERBOSE_FACTS:-0}" = 1 ]; then
            echo "BOOTSTRAP_INFO: daemon $id relaunched after $cause (backend=$backend)"
          fi
        else
          echo "DAEMON_LIVENESS: daemon $id: respawn failed after $cause: $(first_line "$out")"
        fi
        ;;
      ambiguous)
        echo "DAEMON_LIVENESS: daemon $id: skipped: existing endpoint has ambiguous agent process (backend=$backend)"
        ;;
      unreadable)
        echo "DAEMON_LIVENESS: daemon $id: skipped: endpoint probe unreadable (backend=$backend)"
        ;;
      unverified-harness)
        echo "DAEMON_LIVENESS: daemon $id: skipped: recorded harness '$harness' is unverified for recovery (backend=$backend)"
        ;;
      *)
        echo "DAEMON_LIVENESS: daemon $id: skipped: agent recovery classifier unverified (backend=$backend)"
        ;;
    esac
  done
  return 0
}

install_cmd() {
  case "$1" in
    tmux|node|git|gh|curl|jq) echo "brew install $1  # or the platform's package manager" ;;
    cmux) echo "brew install --cask cmux  # or see https://cmux.com" ;;
    treehouse) echo "curl -fsSL https://kunchenguid.github.io/treehouse/install.sh | sh" ;;
    *) return 1 ;;
  esac
}

manual_install_url() {
  case "$1" in
    herdr) echo "https://herdr.dev" ;;
    *) return 1 ;;
  esac
}

missing_tool_diagnostic() {
  local tool=$1 instructions
  if instructions=$(manual_install_url "$tool"); then
    echo "MISSING_MANUAL: $tool (instructions: $instructions)"
    return 0
  fi
  echo "MISSING: $tool (install: $(install_cmd "$tool"))"
}

# Required-tool detection combines the universal toolchain every home needs with
# the backend-specific delta owned by mx_backend_required_tools
# (bin/mx-backend.sh). Treehouse is universal because every supported backend is
# a session provider only. A herdr/cmux home is therefore never told tmux is
# missing, while an invalid backend still cannot suppress the worktree-provider
# probe. A backend value with no verified dependency set is reported before the
# universal checks continue.
COMMON_TOOLS="node git gh jq treehouse"
BACKEND=$(mx_backend_name)
BACKEND_VALID=1
if ! BACKEND_TOOLS=$(mx_backend_required_tools "$BACKEND"); then
  BACKEND_VALID=0
  BACKEND_TOOLS=""
fi
treehouse_supports_lease() {
  treehouse get --help 2>&1 | grep -Eq '(^|[^[:alnum:]_-])--lease([^[:alnum:]_-]|$)'
}

actor_dispatch_validate() {
  local file err
  file="$CONFIG/actor-dispatch.json"
  [ -f "$file" ] || return 0
  if ! command -v jq >/dev/null 2>&1; then
    echo "MISSING: jq (install: $(install_cmd jq))"
    return 0
  fi
  if ! jq -e . "$file" >/dev/null 2>&1; then
    echo "ACTOR_DISPATCH: invalid config/actor-dispatch.json - malformed JSON"
    return 0
  fi
  err=$(jq -r '
    def verified($h): ["claude","codex","pi"] | index($h);
    def effort_ok($h; $e):
      if $e == null then true
      elif ($e | type) != "string" then false
      elif $h == "claude" then (["low","medium","high","xhigh","max"] | index($e))
      elif $h == "codex" then (["low","medium","high","xhigh"] | index($e))
      elif $h == "pi" then (["low","medium","high","xhigh","max"] | index($e))
      else true
      end;
    def profiles($value):
      if ($value | type) == "array" then $value
      elif ($value | type) == "object" then [$value]
      else []
      end;
    def configured_profiles:
      ([(.rules // [])[]? | profiles(.use?)[]?]
        + (if has("default") then [profiles(.default)[]?] else [] end));
    def malformed_optional_fields($items):
      ($items | any(has("model") and (((.model | type) != "string") or (.model | length) == 0)))
      or ($items | any(has("effort") and (((.effort | type) != "string") or (.effort | length) == 0)));
    def bad_efforts:
      configured_profiles
      | map({h: .harness, e: .effort})
      | map(select(.e != null))
      | map(select((.h | type) == "string" and verified(.h)))
      | map(select(. as $p | effort_ok($p.h; $p.e) | not))
      | map("\(.h):\(.e)")
      | unique;
    if type != "object" then "top-level value must be an object"
    elif has("rules") and (.rules | type) != "array" then "rules must be an array"
    elif [(.rules // [])[]? | select(type != "object")] | length > 0 then "each rule must be an object"
    elif [(.rules // [])[]? | select((.when? | type) != "string" or (.when | length) == 0)] | length > 0 then "each rule needs non-empty when"
    elif [(.rules // [])[]? | select((.use? | type) != "object" and (.use? | type) != "array")] | length > 0 then "each rule needs use"
    elif [(.rules // [])[]? | select((.use? | type) == "array" and (.use | length) == 0)] | length > 0 then "each rule needs at least one use profile"
    elif [(.rules // [])[]? | profiles(.use?)[]? | select(type != "object")] | length > 0 then "each use profile must be an object"
    elif [(.rules // [])[]? | profiles(.use?)[]? | select((.harness? | type) != "string" or (.harness | length) == 0)] | length > 0 then "each use profile needs harness"
    elif malformed_optional_fields([(.rules // [])[]? | profiles(.use?)[]?]) then "use profile model and effort must be non-empty strings when present"
    elif [(.rules // [])[]? | select(has("select") and ((.select? | type) != "string" or (.select | length) == 0))] | length > 0 then "select must be a non-empty string"
    elif [(.rules // [])[]? | .select? // empty | select(. != "quota-balanced")] | length > 0 then
      "unknown select: " + ([ (.rules // [])[]? | .select? // empty | select(. != "quota-balanced") ] | unique | join(", "))
    elif has("default") and ((.default | type) != "object" and (.default | type) != "array") then "default must be a profile object or non-empty profile array"
    elif has("default") and ((.default | type) == "array" and (.default | length) == 0) then "default needs at least one profile"
    elif has("default") and ([profiles(.default)[]? | select(type != "object")] | length) > 0 then "each default profile must be an object"
    elif has("default") and ([profiles(.default)[]? | select((.harness? | type) != "string" or (.harness | length) == 0)] | length) > 0 then "each default profile needs harness"
    elif has("default") and malformed_optional_fields([profiles(.default)[]?]) then "default profile model and effort must be non-empty strings when present"
    else
      (configured_profiles
        | map(.harness)
        | map(select(. != null))
        | map(select(. as $h | verified($h) | not))
        | unique) as $bad_harnesses
      | if ($bad_harnesses | length) > 0 then "unverified harness: " + ($bad_harnesses | join(", "))
        elif (bad_efforts | length) > 0 then "invalid effort: " + (bad_efforts | join(", "))
        else empty
        end
    end
  ' "$file" 2>/dev/null || true)
  if [ -n "$err" ]; then
    echo "ACTOR_DISPATCH: invalid config/actor-dispatch.json - $err"
    return 0
  fi
  if [ "${MX_BOOTSTRAP_VERBOSE_FACTS:-0}" = 1 ]; then
    jq -r '
    def profile($p):
      ($p.harness | tostring)
      + (if ($p.model? != null) then "/" + ($p.model | tostring)
         elif ($p.effort? != null) then "/default"
         else "" end)
      + (if ($p.effort? != null) then "/" + ($p.effort | tostring) else "" end);
    def profile_set($value; $selector):
      if ($value | type) == "array" then
        (($selector // "quota-balanced") + "[" + ([$value[] | profile(.)] | join(", ")) + "]")
      else profile($value)
      end;
    (["BOOTSTRAP_INFO: actor dispatch active config/actor-dispatch.json"]
      + [(.rules // [])[]? | "BOOTSTRAP_INFO: actor dispatch rule: " + (.when | tostring) + " -> " + profile_set(.use; .select?)]
      + (if has("default") then ["BOOTSTRAP_INFO: actor dispatch default: " + profile_set(.default; null)] else [] end))
    | .[]
  ' "$file"
  fi
}

if [ "${1:-}" = "install" ]; then
  shift
  [ $# -gt 0 ] || { echo "usage: mx-bootstrap.sh install <tool>..." >&2; exit 1; }
  for t in "$@"; do
    if ! cmd=$(install_cmd "$t"); then
      instructions=$(manual_install_url "$t") || { echo "error: unknown tool $t" >&2; exit 1; }
      echo "error: $t requires manual installation (instructions: $instructions)" >&2
      exit 1
    fi
    cmd=${cmd%%  #*}
    echo "installing $t: $cmd"
    eval "$cmd"
  done
  exit 0
fi

# This is the first mutating sweep at a locked session boundary. It pauses an
# identity-matched watcher, holds its lock, and neutralizes legacy PR checks
# before any tool detection or later bootstrap mutation can leave old artifacts
# runnable. Detect-only sessions never touch state.
if [ "${MX_BOOTSTRAP_DETECT_ONLY:-0}" != 1 ]; then
  "$SCRIPT_DIR/mx-pr-check-migrate.sh" || true
fi

if [ "$BACKEND_VALID" -eq 0 ]; then
  echo "BACKEND_INVALID: $BACKEND (known: $MX_BACKEND_KNOWN)"
fi
for t in $BACKEND_TOOLS; do
  mx_backend_required_tool_available "$BACKEND" "$t" \
    || missing_tool_diagnostic "$t"
done
for t in $COMMON_TOOLS; do
  command -v "$t" >/dev/null || missing_tool_diagnostic "$t"
done
# Every supported backend delegates worktree acquisition to treehouse, so its
# durable-lease capability is an unconditional bootstrap requirement.
if command -v treehouse >/dev/null 2>&1 && ! treehouse_supports_lease; then
  echo "MISSING: treehouse (install: $(install_cmd treehouse))"
fi
VPLAN_SELF_CHECK=${MX_VPLAN_SELF_CHECK_OVERRIDE:-$SCRIPT_DIR/mx-vplan.sh}
if ! "$VPLAN_SELF_CHECK" --self-check >/dev/null 2>&1; then
  echo "VPLAN_INVALID: bundled mx-vplan.sh self-check failed"
elif [ "${MX_BOOTSTRAP_VERBOSE_FACTS:-0}" = 1 ]; then
  echo "BOOTSTRAP_INFO: vplan self-check passed"
fi
if ! headroom_json=$(MX_HEADROOM_IGNORE_DISPATCH_CONFIG=1 "$SCRIPT_DIR/mx-headroom.sh" --json 2>/dev/null) \
  || ! printf '%s\n' "$headroom_json" | node -e '
    let input = "";
    process.stdin.on("data", chunk => input += chunk);
    process.stdin.on("end", () => {
      const value = JSON.parse(input);
      for (const key of ["model", "capacity", "in_use", "available", "candidates", "at_limit"]) {
        if (!(key in value)) process.exit(1);
      }
    });
  ' >/dev/null 2>&1; then
  echo "HEADROOM_INVALID: bundled mx-headroom.sh self-check failed"
elif [ "${MX_BOOTSTRAP_VERBOSE_FACTS:-0}" = 1 ]; then
  echo "BOOTSTRAP_INFO: headroom self-check passed"
fi
# Worktree-tangle check: the broker primary checkout (MX_ROOT) must sit on its
# default branch, not a feature branch (see mx-tangle-lib.sh). Scoped to the
# primary only; detached-HEAD worktrees and daemon homes never trip it.
tangle_branch=$(mx_primary_tangle_branch "$MX_ROOT" 2>/dev/null || true)
if [ -n "$tangle_branch" ]; then
  tangle_default=$(mx_default_branch "$MX_ROOT" 2>/dev/null || echo main)
  if [ "${MX_BOOTSTRAP_DETECT_ONLY:-0}" = 1 ]; then
    echo "TANGLE: primary checkout on feature branch '$tangle_branch' (expected '$tangle_default'); the work is safe on that ref - read-only session must leave restore work to the session holding the system lock"
  else
    echo "TANGLE: primary checkout on feature branch '$tangle_branch' (expected '$tangle_default'); the work is safe on that ref - restore the primary with: git -C $MX_ROOT checkout $tangle_default, then re-validate the branch in a proper worktree"
  fi
fi
actor=
[ -f "$CONFIG/actor-harness" ] && actor=$(tr -d '[:space:]' < "$CONFIG/actor-harness" || true)
if [ "${MX_BOOTSTRAP_VERBOSE_FACTS:-0}" = 1 ] && [ -n "$actor" ] && [ "$actor" != "default" ]; then
  echo "BOOTSTRAP_INFO: actor harness override active: $actor"
fi
actor_dispatch_validate
if [ "${MX_BOOTSTRAP_DETECT_ONLY:-0}" != 1 ]; then
  daemon_liveness_sweep
  daemon_sync
  system_sync
fi
exit 0
