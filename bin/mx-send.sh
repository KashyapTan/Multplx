#!/usr/bin/env bash
# Send one line of literal text to an actor endpoint, then Enter.
# Usage: mx-send.sh <target> <text...>
#   <target> may be an exact task id, a legacy mx-<id> task label resolved
#   through this home's state/<id>.meta, or an explicit well-formed backend
#   target. mx-send refuses unresolved guesses rather than falling back to a
#   tmux window search, because a "successful" send to the wrong endpoint is
#   worse than a loud failure.
# Special keys instead of text: mx-send.sh <target> --key Enter
# Key support is backend-specific: tmux/herdr support Escape, Enter, and C-c.
#
# Text submission is verified: the line is typed ONCE, then Enter is sent and
# retried (Enter only, never retyped) until the target backend confirms a
# submit or reports an inconclusive send. If a swallowed Enter is positively
# confirmed, mx-send exits NON-ZERO so the caller knows the steer did not land
# instead of silently leaving an unsubmitted instruction.
# Submission dispatches through the target's recorded backend; the tmux adapter
# shares its composer/submit core with the away-mode daemon via bin/mx-tmux-lib.sh.
# Tune with MX_SEND_RETRIES (default 3) / MX_SEND_SLEEP (0.4).
# Slash commands, and codex `$...` skill invocations resolved through harness
# meta, get a longer pre-Enter settle so completion popups do not swallow Enter.
#
# From-broker marker: when the resolved target is a task selector whose meta
# records kind=daemon, the text uses the live-charter-compatible
# from-broker carrier owned by bin/mx-operational-input.sh so the daemon
# routes its reply via its status file or a status-pointed doc instead of
# stranding it in chat the main broker never reads. an actor/scout target,
# an explicit backend-target escape-hatch target, and the --key path are never
# marked - their behavior is unchanged.
#
# Parent-owned pending-reply expectation: every newly marked daemon request
# also receives a privacy-safe correlation id and a durable parent record under
# state/pending-replies/ before delivery (bin/mx-pending-reply-lib.sh). Delivery
# success and reply success are separate facts: a successful submit never
# resolves the expectation. Set MX_PENDING_REPLY_EXISTING_CORR=<id> when
# re-sending a recovery request for an already-open expectation so a second
# record is not created. Direct unmarked maintainer input never creates one.
#
# After a successful text submit mx-send pauses MX_SEND_SETTLE seconds (default 1,
# 0 disables) before returning: submit confirmation only proves the text was
# accepted, but the harness needs a beat to spin up the turn before its busy
# footer appears, so an immediate peek would otherwise see the stale idle pane.
# The pause is mx-send-only; the shared submit core (used by the away-mode daemon,
# which only needs "submitted") does not pay it, and the --key path is unaffected.
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"

# shellcheck source=bin/mx-gate-refuse-lib.sh
. "$SCRIPT_DIR/mx-gate-refuse-lib.sh"
# Fail closed before any system mutation: a deep-review agent must never steer
# an actor (see bin/mx-gate-refuse-lib.sh).
mx_refuse_if_gate_agent

if [ -z "${MX_HOME+x}" ] || [ -z "${MX_HOME:-}" ]; then
  echo "error: MX_HOME is not set; mx-send refuses to resolve targets without an explicit Multplx home" >&2
  exit 1
fi

STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
if [ ! -d "$MX_HOME" ]; then
  echo "error: MX_HOME '$MX_HOME' is not a directory; mx-send cannot resolve this home's state" >&2
  exit 1
fi
if [ ! -d "$STATE" ]; then
  echo "error: state dir '$STATE' is missing; mx-send cannot resolve targets for MX_HOME '$MX_HOME'" >&2
  exit 1
fi

# shellcheck source=bin/mx-backend.sh
. "$SCRIPT_DIR/mx-backend.sh"
# shellcheck source=bin/mx-marker-lib.sh
. "$SCRIPT_DIR/mx-marker-lib.sh"
# shellcheck source=bin/mx-pending-reply-lib.sh
. "$SCRIPT_DIR/mx-pending-reply-lib.sh"

MX_GUARD_CONTINUE_LINE='This is a supervision warning only; the requested message WILL still be sent.' "$SCRIPT_DIR/mx-guard.sh" || true

mx_send_id_from_meta() {  # <meta-file>
  local base
  base=${1##*/}
  printf '%s' "${base%.meta}"
}

mx_send_meta_for_key_value() {  # <state-dir> <key> <value>
  local state=$1 key=$2 value=$3 meta got
  for meta in "$state"/*.meta; do
    [ -e "$meta" ] || continue
    got=$(mx_meta_get "$meta" "$key")
    [ "$got" = "$value" ] || continue
    printf '%s' "$meta"
    return 0
  done
  return 1
}

mx_send_count_colons() {  # <string>
  local s=$1 no_colons
  no_colons=${s//:/}
  printf '%s' $(( ${#s} - ${#no_colons} ))
}

mx_send_resolve_target() {  # <raw-target>
  local raw=$1 meta pane_meta target backend assumed colons id session hint

  RESOLVED_TARGET=""
  TARGET_BACKEND=""
  TARGET_HARNESS=""
  EXPECTED_LABEL=""
  TARGET_META=""
  TARGET_SELECTOR=""
  RESOLUTION_TRIED=""

  meta=$(mx_backend_meta_for_selector "$raw" "$STATE" 2>/dev/null || true)
  if [ -n "$meta" ]; then
    RESOLUTION_TRIED="meta=$meta; backend=from-meta"
    target=$(mx_backend_target_of_meta "$meta")
    if [ -z "$target" ]; then
      echo "error: no backend target recorded in $meta (tried $RESOLUTION_TRIED)" >&2
      return 1
    fi
    backend=$(mx_backend_of_meta "$meta")
    RESOLVED_TARGET=$target
    TARGET_BACKEND=$backend
    TARGET_META=$meta
    TARGET_HARNESS=$(mx_meta_get "$meta" harness)
    EXPECTED_LABEL=$(mx_backend_expected_label_of_selector "$raw" "$STATE")
    TARGET_SELECTOR=1
    return 0
  fi

  case "$raw" in
    mx-*)
      RESOLUTION_TRIED="meta=$STATE/$raw.meta; legacy-meta=$STATE/${raw#mx-}.meta; backend=none"
      echo "error: no metadata for $raw in $STATE (tried $RESOLUTION_TRIED); pass a well-formed explicit backend target only when targeting outside this Multplx home" >&2
      return 1
      ;;
  esac

  pane_meta=$(mx_send_meta_for_key_value "$STATE" herdr_pane_id "$raw" 2>/dev/null || true)
  if [ -n "$pane_meta" ]; then
    session=$(mx_meta_get "$pane_meta" herdr_session)
    hint="${session:-<herdr-session>}:$raw"
    id=$(mx_send_id_from_meta "$pane_meta")
    echo "error: target '$raw' matches herdr_pane_id in $pane_meta but is missing its herdr session prefix; expected <herdr-session>:<pane-id> such as '$hint' or use 'mx-$id' (tried meta=$STATE/$raw.meta; backend=herdr)" >&2
    return 1
  fi

  meta=$(mx_backend_meta_for_window "$raw" "$STATE" 2>/dev/null || true)
  if [ -n "$meta" ]; then
    target=$(mx_backend_target_of_meta "$meta")
    if [ -z "$target" ]; then
      echo "error: no backend target recorded in $meta (tried explicit target '$raw' via recorded window/terminal; backend=from-meta)" >&2
      return 1
    fi
    RESOLVED_TARGET=$target
    TARGET_BACKEND=$(mx_backend_of_meta "$meta")
    TARGET_META=$meta
    TARGET_HARNESS=$(mx_meta_get "$meta" harness)
    RESOLUTION_TRIED="explicit target '$raw' matched $meta; backend=$TARGET_BACKEND"
    return 0
  fi

  case "$raw" in
    *:*)
      colons=$(mx_send_count_colons "$raw")
      if [ "$colons" -ge 2 ]; then
        assumed=herdr
      else
        assumed=tmux
      fi
      if ! mx_backend_target_exists "$assumed" "$raw"; then
        echo "error: explicit target '$raw' is not a live $assumed endpoint (tried meta=$STATE/$raw.meta; metadata window/terminal lookup; backend=$assumed). Use mx-<id> for a recorded task/lane, or pass a target whose backend endpoint can be verified." >&2
        return 1
      fi
      RESOLVED_TARGET=$raw
      TARGET_BACKEND=$assumed
      RESOLUTION_TRIED="meta=$STATE/$raw.meta; metadata window/terminal lookup; backend=$assumed; endpoint=verified"
      return 0
      ;;
  esac

  echo "error: target '$raw' is not resolvable (tried meta=$STATE/$raw.meta; metadata window/terminal lookup; backend=none). Use mx-$raw for a recorded task/lane, or pass a well-formed explicit backend target such as session:window." >&2
  return 1
}

RAW_TARGET=$1
mx_send_resolve_target "$RAW_TARGET" || exit 1
T=$RESOLVED_TARGET
shift

mx_backend_validate "$TARGET_BACKEND" || exit 1

# Classify a from-broker -> daemon request. Only a task selector resolved
# through this home's meta whose authoritative kind is daemon is marked: the
# daemon then routes its reply via the status path (see mx-marker-lib.sh).
# An explicit backend target (the escape hatch for endpoints outside this home)
# and any actor/scout target are left unmarked, and so is the --key path.
MARK_FROM_MULTPLX=0
PENDING_REPLY_CORR=
PENDING_REPLY_CREATED=0
TARGET_TASK_ID=
if [ -n "$TARGET_SELECTOR" ] && [ -n "$TARGET_META" ] && [ "$(mx_meta_get "$TARGET_META" kind)" = daemon ]; then
  MARK_FROM_MULTPLX=1
  TARGET_TASK_ID=$(mx_send_id_from_meta "$TARGET_META")
fi

# Resolve the target's harness from its meta (recorded by mx-spawn), used only to
# scope the codex `$<skill>` popup-settle below. A task selector carries
# meta; an explicit backend-target escape hatch has none, so its harness is
# unknown and treated as non-codex (the safe default that keeps the fast path).
# The target's BACKEND comes from selector meta, from matching an explicit target
# back to recorded meta, or from strict explicit-target shape validation.
# Do not add a separate passive liveness preflight here. Active send paths own
# backend readiness: herdr, for example, must route through its session-aware
# target_ready path before sending. A failed backend send is still surfaced
# below as a hard error with the attempted resolution attached.

if [ "${1:-}" = "--key" ]; then
  if ! mx_backend_send_key "$TARGET_BACKEND" "$T" "$2" "$EXPECTED_LABEL"; then
    echo "error: key '$2' not sent to $T ($TARGET_BACKEND send failed; tried $RESOLUTION_TRIED)" >&2
    exit 1
  fi
else
  MESSAGE=$*
  if [ "$MARK_FROM_MULTPLX" = 1 ]; then
    # Reuse an existing correlation id for recovery resends; otherwise create a
    # durable parent expectation before delivery. Transport success never
    # resolves that expectation (see mx-pending-reply-lib.sh).
    existing_corr=${MX_PENDING_REPLY_EXISTING_CORR:-$(mx_pending_reply_extract_corr "$MESSAGE")}
    if [ -n "$existing_corr" ] \
      && mx_pending_reply_corr_reusable "$STATE" "$existing_corr" "$TARGET_TASK_ID"; then
      PENDING_REPLY_CORR=$existing_corr
    else
      if [ -z "$TARGET_TASK_ID" ]; then
        echo "error: cannot create pending-reply expectation without a resolvable daemon task id" >&2
        exit 1
      fi
      PENDING_REPLY_CORR=$(mx_pending_reply_create "$MX_HOME" "$STATE" "$TARGET_TASK_ID" "$MESSAGE") \
        || { echo "error: failed to create parent pending-reply expectation for $TARGET_TASK_ID" >&2; exit 1; }
      PENDING_REPLY_CREATED=1
    fi
    mx_pending_reply_embed_corr "$MESSAGE" "$PENDING_REPLY_CORR" MESSAGE
    if [ "$PENDING_REPLY_CREATED" = 1 ] \
      && ! mx_pending_reply_prepare_delivery "$STATE" "$PENDING_REPLY_CORR"; then
      mx_pending_reply_discard_undelivered "$STATE" "$PENDING_REPLY_CORR" || true
      echo "error: failed to durably prepare pending-reply delivery for $TARGET_TASK_ID" >&2
      exit 1
    fi
  fi
  # Slash commands open a completion popup in some TUIs (verified on codex);
  # submitting too fast selects nothing, so give the popup time to settle before
  # the (retried) Enter. Codex opens the same kind of popup for a `$<skill>`
  # invocation, so a `$...` message to a codex target gets the same settle. That
  # `$` case is scoped to codex on purpose: unlike `/`, a leading `$` commonly
  # starts ordinary text ("$5/month", "$HOME"), so a universal `$` rule would
  # needlessly slow plain text to claude/pi. The target backend's
  # verified submit retry still backs the settle up either way.
  case "$*" in
    /*) settle=1.2 ;;
    \$*)
      if [ "$TARGET_HARNESS" = codex ]; then settle=1.2; else settle=0.3; fi
      ;;
    *) settle=0.3 ;;
  esac
  retries=${MX_SEND_RETRIES:-3}
  sleep_s=${MX_SEND_SLEEP:-0.4}
  # Type once, submit, verify. Lenient: only a positively-confirmed swallow
  # (text still in the composer) is an error; an unreadable pane is assumed sent.
  if ! verdict=$(mx_backend_send_text_submit "$TARGET_BACKEND" "$T" "$MESSAGE" "$retries" "$sleep_s" "$settle" "$EXPECTED_LABEL"); then
    if [ "$PENDING_REPLY_CREATED" = 1 ] && [ -n "$PENDING_REPLY_CORR" ]; then
      mx_pending_reply_discard_undelivered "$STATE" "$PENDING_REPLY_CORR" || true
    fi
    echo "error: text not sent to $T ($TARGET_BACKEND send failed; tried $RESOLUTION_TRIED)" >&2
    exit 1
  fi
  case "$verdict" in
    pending)
      if [ "$PENDING_REPLY_CREATED" = 1 ] && [ -n "$PENDING_REPLY_CORR" ]; then
        mx_pending_reply_discard_undelivered "$STATE" "$PENDING_REPLY_CORR" || true
      fi
      echo "error: text not submitted to $T (Enter swallowed; text left in composer; tried $RESOLUTION_TRIED)" >&2
      exit 1
      ;;
    send-failed)
      if [ "$PENDING_REPLY_CREATED" = 1 ] && [ -n "$PENDING_REPLY_CORR" ]; then
        mx_pending_reply_discard_undelivered "$STATE" "$PENDING_REPLY_CORR" || true
      fi
      echo "error: text not sent to $T ($TARGET_BACKEND send failed; tried $RESOLUTION_TRIED)" >&2
      exit 1
      ;;
  esac
  # Delivery confirmed. Mark the pending expectation delivered without resolving
  # it: only a correlated parent report acknowledges the request.
  if [ -n "$PENDING_REPLY_CORR" ]; then
    if mx_pending_reply_confirm_delivery "$STATE" "$PENDING_REPLY_CORR"; then
      :
    else
      delivery_commit_status=$?
      if [ "$delivery_commit_status" = 2 ]; then
        echo "error: text was delivered to $T, but its pending-reply delivery commit failed; a durable recovery marker was stored and the watcher will reconcile it. Do not resend." >&2
      else
        echo "error: text was delivered to $T, but its pending-reply delivery commit and recovery marker both failed. Do not resend; inspect $STATE manually." >&2
      fi
      exit 1
    fi
  fi
  # Submit landed (verdict was not pending/send-failed). Confirmation only proves
  # the text was accepted; the harness still needs a beat to spin up the
  # turn before its busy footer shows. Pause so an immediate peek catches the
  # actor actually working instead of the stale idle pane. MX_SEND_SETTLE=0
  # disables it. Scoped to this path only, never the shared submit core.
  [ "${MX_SEND_SETTLE:-1}" = 0 ] || sleep "${MX_SEND_SETTLE:-1}"
fi
