#!/usr/bin/env bash
# Shared wake classifier: the common source of truth for maintainer-relevant status
# tests, declared-external-wait vocabulary, and the working/paused absorb
# classification that makes no-verb signal and stale-pane wakes safe to absorb.
# Sourced by BOTH the always-on watcher
# (bin/mx-watch.sh) and the away-mode daemon (bin/mx-supervise-daemon.sh) so the
# overlapping triage policy lives in one place instead of two copies that can
# drift apart.
#
# Signal precedence for same-moment disagreements is owned here. The three
# agent-signal tiers required by Plan 5 are:
#   native event > schema-validated self-report > text/regex heuristic
# The maintainer placed attributed validation run-step evidence between the
# native and self-report tiers, so the complete resolver order is:
#   native event > attributed run-step > schema-validated self-report
#   > text/regex heuristic
# A native blocker therefore surfaces immediately even while an attributed CI
# run continues; the current-state reader retains the concurrent run detail for
# the broker. Backends without a tier pass an empty or unknown value.
#
# Most functions are pure, side-effect-free reads of status files: each takes
# what it needs as arguments and touches no globals beyond the optional
# MX_MAINTAINER_RE override. Consumers layer their own dedup/marker state on top (the
# daemon keeps its escalation-digest seen-markers; the watcher keeps its .seen-*
# signatures).
#
# The one exception is the absorb classification (actor_absorb_class and its
# working/paused wrappers). It is NOT a pure status-file read: it reuses
# bin/mx-actor-state.sh, which may make a bounded no-mistakes call, to decide
# whether an actor that just stopped its turn or went stale is working, deliberately
# paused, or neither. Callers run it ONLY on no-verb signal handling and first
# sighting of a stale hash, never on every wake, so the per-wake triage stays
# cheap.

# Directory of this library, used to locate the sibling mx-actor-state.sh reader.
# Resolved at source time from BASH_SOURCE so it works whether sourced by a
# bin/ script (which sets its own SCRIPT_DIR) or directly by a test.
_MX_CLASSIFY_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd 2>/dev/null)" || _MX_CLASSIFY_LIB_DIR="."

# The actors current-state reader used for the "provably working" decision.
# Overridable so tests can stub the run-step/pane verdict without a real worktree
# or no-mistakes install; absent, it points at the real sibling script.
MX_ACTOR_STATE_BIN="${MX_ACTOR_STATE_BIN:-$_MX_CLASSIFY_LIB_DIR/mx-actor-state.sh}"

# Maintainer-relevant status verbs. A status line carrying any of these is work
# broker must see. Lines without these verbs are no-verb signals: the watcher
# absorbs them only with positive provably-working evidence, while the daemon uses
# its away-mode classification. MX_MAINTAINER_RE overrides the whole set when a home
# needs a custom verb vocabulary; absent, this default applies.
#
# Free-text tokens (PR ready, checks green, ready in branch, merged) exist only for
# legacy lines that lack a standard terminal verb. status_is_maintainer_relevant is
# verb-aware: a nonterminal working: or paused: line never becomes maintainer-relevant
# merely because its prose contains one of those tokens (for example
# "working: rebased onto merged #76").
MX_CLASSIFY_MAINTAINER_RE_DEFAULT='done:|needs-decision:|blocked:|failed:|PR ready|checks green|ready in branch|merged'

# The deliberate-external-wait verb. an actor (or broker steering it) appends
#   paused: <reason>
# to declare it is intentionally idling on a KNOWN external dependency - an
# upstream release, a vendor rate-limit reset, a scheduled window. Unlike
# `blocked:` (stuck, broker must help) an idle `paused:` pane is EXPECTED, so
# the stale path absorbs it instead of escalating a possible wedge. It is
# deliberately NOT in the maintainer-relevant set above: a pause is a "stop
# wedge-nagging this idle pane" signal, not work to keep surfacing. This constant
# is the ONE definition of the verb; both the watcher and the daemon read it here
# (status_is_paused) rather than hardcoding the literal, so the vocabulary cannot
# drift between the two consumers. MX_CLASSIFY_PAUSED_VERB overrides it.
MX_CLASSIFY_PAUSED_VERB_DEFAULT='paused'

# Bounded re-surface cadence for a declared pause or a dead-agent maintainer hold.
# Far longer than the wedge threshold (MX_STALE_ESCALATE_SECS, default 240s), it
# avoids nagging a deliberate wait while ensuring a forgotten hold cannot rot
# invisibly - it re-surfaces once for a recheck every window. One hour by default;
# both consumers read MX_PAUSE_RESURFACE_SECS with this default so the cadence has
# one owner.
# shellcheck disable=SC2034 # Read by the watcher and daemon (mx-watch.sh, mx-supervise-daemon.sh), not this lib.
MX_PAUSE_RESURFACE_SECS_DEFAULT=3600

# The resolution verb and durable-backlog-transfer verb that CLOSE a keyed
# status decision opened by needs-decision or blocked. See status_open_decisions
# below for the status-fold contract. The transfer verb is written only after
# mx-decision-hold.sh has verified the corresponding maintainer-held backlog item.
MX_CLASSIFY_RESOLVE_VERB_DEFAULT='resolved'
MX_CLASSIFY_MAINTAINER_HELD_VERB_DEFAULT='maintainer-held'

# Return the last non-blank line of a status file (empty if missing/blank).
last_status_line() {
  local f=$1
  [ -e "$f" ] || return 0
  grep -v '^[[:space:]]*$' "$f" 2>/dev/null | tail -1
}

# 0 if the given (last) status line's leading verb is a real terminal maintainer verb
# (done, needs-decision, blocked, failed). Free-text tokens alone never count here;
# callers that need legacy free-text matching use status_is_maintainer_relevant.
status_is_terminal_verb() {
  local line=$1 verb
  [ -n "$line" ] || return 1
  verb=$(status_line_verb "$line")
  case "$verb" in
    done|needs-decision|blocked|failed) return 0 ;;
    *) return 1 ;;
  esac
}

# 0 if the given (last) status line matches a maintainer-relevant verb.
# Verb-aware by default: terminal verbs always match; nonterminal progress verbs
# (working, resolved, maintainer-held) and paused never match from free-text prose;
# only lines without those leading verbs may still match free-text tokens for
# legacy bare lines such as "merged" or "PR ready".
status_is_maintainer_relevant() {
  local line=$1 verb
  [ -n "$line" ] || return 1
  status_is_paused "$line" && return 1
  verb=$(status_line_verb "$line")
  case "$verb" in
    working|resolved|maintainer-held|"${MX_CLASSIFY_PAUSED_VERB:-$MX_CLASSIFY_PAUSED_VERB_DEFAULT}")
      return 1
      ;;
  esac
  if [ -z "${MX_MAINTAINER_RE+x}" ]; then
    case "$verb" in
      done|needs-decision|blocked|failed) return 0 ;;
    esac
  fi
  printf '%s' "$line" | grep -qiE "${MX_MAINTAINER_RE:-$MX_CLASSIFY_MAINTAINER_RE_DEFAULT}"
}

# 0 if a status line's leading verb is the pause verb (paused: <reason>). A pure
# read of the line itself, so the daemon's classify_stale can reuse the last line
# it already read without a mx-actor-state.sh call. Matches only the verb before the
# first colon, so a reason mentioning "paused" elsewhere does not false-match.
status_is_paused() {  # <status-line>
  local line=$1 verb
  [ -n "$line" ] || return 1
  verb=$(status_line_verb "$line")
  [ "$verb" = "${MX_CLASSIFY_PAUSED_VERB:-$MX_CLASSIFY_PAUSED_VERB_DEFAULT}" ]
}

# 0 if a status line declares either an external-wait pause or a verified
# maintainer-held transfer.
# Both declarations can intentionally leave an exited actor's endpoint idle, so
# the watcher applies its bounded pause cadence when agent death confirms that
# no live decision gate is being silenced.
status_is_paused_or_maintainer_held() {  # <status-line>
  local line=$1 verb
  status_is_paused "$line" && return 0
  [ -n "$line" ] || return 1
  verb=$(status_line_verb "$line")
  [ "$verb" = "${MX_CLASSIFY_MAINTAINER_HELD_VERB:-$MX_CLASSIFY_MAINTAINER_HELD_VERB_DEFAULT}" ]
}

# --- durable keyed decisions ------------------------------------------------
#
# The status stream is an append-only EVENT log. Reading it last-event-wins
# (last_status_line above) cannot represent "an earlier decision is still open
# after a later, unrelated event": a subsequent done/paused/working line silently
# masks a still-open needs-decision. status_open_decisions is the ONE authoritative
# statement of the status-fold contract that fixes this - a needs-decision/blocked
# line OPENS a keyed decision, and only an explicit resolution or a verified
# maintainer-held backlog transfer referencing that key CLOSES it; a later unrelated
# terminal line never clears an open maintainer decision.
#
# Decision key grammar (backward-compatible with the existing "<verb>: <note>"
# format): an OPTIONAL "[key=<slug>]" token sits between the verb and the colon,
#   needs-decision [key=api-shape]: <summary>
#   resolved       [key=api-shape]: <how it was decided>
# A line with no token uses the key "default", preserving the historical
# one-open-decision-per-task behavior (a bare "resolved:" closes "default").
# The three parsers are pure reads of a single line; the verb parser strips any
# key token before the colon so the leading word is recovered cleanly.
status_line_verb() {  # <status-line> -> leading verb word
  local v=${1%%:*}
  v=${v%%\[key=*}
  v=${v#"${v%%[![:space:]]*}"}
  v=${v%"${v##*[![:space:]]}"}
  printf '%s' "$v"
}

# mx_signal_resolve: rank same-moment signals about one task without reading
# files or backend state.
#   $1 native      - runtime state (idle|working|blocked|done|unknown)
#   $2 run_step    - attributed validation state
#                   (working|parked|done|blocked|paused|failed|unknown)
#   $3 self_report - leading status verb from the actor-writable closed enum
#   $4 heuristic   - text/pane verdict (busy|idle|unknown)
# Prints "<tier>:<verdict>" for the strongest recognized tier, or "none".
# Unknown, empty, and malformed values contribute no tier. The self-report
# vocabulary deliberately recognizes a well-formed enum verb without adding a
# provenance marker to the append-only status wire format.
mx_signal_resolve() {  # <native> <run-step> <self-report> <heuristic>
  local native=${1:-} run_step=${2:-} self_report=${3:-} heuristic=${4:-}
  local pause=${MX_CLASSIFY_PAUSED_VERB:-$MX_CLASSIFY_PAUSED_VERB_DEFAULT}
  case "$native" in
    idle|working|blocked|done)
      printf 'native:%s' "$native"
      return
      ;;
  esac
  case "$run_step" in
    working|parked|done|blocked|paused|failed)
      printf 'run-step:%s' "$run_step"
      return
      ;;
  esac
  case "$self_report" in
    working|blocked|needs-decision|done|failed|resolved|"$pause")
      printf 'self-report:%s' "$self_report"
      return
      ;;
  esac
  case "$heuristic" in
    busy|idle)
      printf 'heuristic:%s' "$heuristic"
      return
      ;;
  esac
  printf 'none'
}

status_line_note() {  # <status-line> -> text after the first colon, trimmed
  case "$1" in
    *:*) local n=${1#*:}; printf '%s' "${n#"${n%%[![:space:]]*}"}" ;;
    *) printf '%s' "$1" ;;
  esac
}
_mx_decision_key() {  # <status-line> -> key slug, or "default" when no token
  local prefix=${1%%:*} k
  case "$prefix" in
    *\[key=*\]*)
      k=${prefix#*\[key=}
      k=${k%%\]*}
      case "$k" in
        ''|*[!A-Za-z0-9._-]*) return 1 ;;
        *) printf '%s' "$k" ;;
      esac
      ;;
    *) printf 'default' ;;
  esac
}
# Drop the record for <key> from a newline-terminated "<key>\t<verb>\t<note>" set.
# Portable (no associative arrays) so the fold runs on bash 3.2 as well as 4+.
_mx_decision_drop() {  # <open-set> <key>
  local set=$1 key=$2 line out=''
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    case "$line" in
      "$key"$'\t'*) : ;;
      *) out="${out}${line}"$'\n' ;;
    esac
  done <<EOF
$set
EOF
  printf '%s' "$out"
}
# Fold the WHOLE status stream into the set of decisions still open. Prints one
# TAB-separated "<key>\t<verb>\t<summary>" line per still-open decision, in
# most-recently-opened-last order; prints nothing when none are open. Pure read of
# the file, no globals beyond the optional MX_CLASSIFY_RESOLVE_VERB override. This
# is the durable open-set the system snapshot and any point-in-time consumer must use
# instead of trusting the last status line.
status_open_decisions() {  # <status-file>
  local f=$1 line verb key note resolve held open='' stripped
  [ -f "$f" ] || return 0
  resolve=${MX_CLASSIFY_RESOLVE_VERB:-$MX_CLASSIFY_RESOLVE_VERB_DEFAULT}
  held=${MX_CLASSIFY_MAINTAINER_HELD_VERB:-$MX_CLASSIFY_MAINTAINER_HELD_VERB_DEFAULT}
  while IFS= read -r line || [ -n "$line" ]; do
    stripped=${line//[[:space:]]/}
    [ -n "$stripped" ] || continue
    verb=$(status_line_verb "$line")
    key=$(_mx_decision_key "$line") || continue
    case "$verb" in
      needs-decision|blocked)
        note=$(status_line_note "$line")
        open=$(_mx_decision_drop "$open" "$key")
        [ -n "$open" ] && open="${open}"$'\n'
        open="${open}${key}"$'\t'"${verb}"$'\t'"${note}"$'\n'
        ;;
      "$resolve"|"$held")
        open=$(_mx_decision_drop "$open" "$key")
        [ -n "$open" ] && open="${open}"$'\n'
        ;;
    esac
  done < "$f"
  printf '%s' "$open"
}

# Fold material routed-work phases in the same keyed event stream.
# A working or declared-pause event opens or replaces one phase for its key.
# A later done, failed, needs-decision, blocked, or resolved event carrying that
# key closes the phase, because it has moved to a terminal or separately tracked
# state.
# A bare legacy event uses the default key, preserving one-phase behavior.
# This fold is evidence about whether a parent event was explicitly superseded.
# It is never authoritative current actor state, and consumers must not let an open
# phase outrank a structured home snapshot or mx-actor-state result.
_mx_status_open_activities_stream() {
  local line verb key note resolve held open='' stripped pause
  resolve=${MX_CLASSIFY_RESOLVE_VERB:-$MX_CLASSIFY_RESOLVE_VERB_DEFAULT}
  held=${MX_CLASSIFY_MAINTAINER_HELD_VERB:-$MX_CLASSIFY_MAINTAINER_HELD_VERB_DEFAULT}
  pause=${MX_CLASSIFY_PAUSED_VERB:-$MX_CLASSIFY_PAUSED_VERB_DEFAULT}
  while IFS= read -r line || [ -n "$line" ]; do
    stripped=${line//[[:space:]]/}
    [ -n "$stripped" ] || continue
    verb=$(status_line_verb "$line")
    key=$(_mx_decision_key "$line") || continue
    case "$verb" in
      working|"$pause")
        note=$(status_line_note "$line")
        open=$(_mx_decision_drop "$open" "$key")
        [ -n "$open" ] && open="${open}"$'\n'
        open="${open}${key}"$'\t'"${verb}"$'\t'"${note}"$'\n'
        ;;
      done|failed|needs-decision|blocked|"$resolve"|"$held")
        open=$(_mx_decision_drop "$open" "$key")
        [ -n "$open" ] && open="${open}"$'\n'
        ;;
    esac
  done
  printf '%s' "$open"
}

status_open_activities() {  # <status-file-or-dash>
  local f=$1
  if [ "$f" = - ]; then
    _mx_status_open_activities_stream
    return 0
  fi
  [ -f "$f" ] || return 0
  _mx_status_open_activities_stream < "$f"
}

# task id from a recorded window target, falling back to the tmux-shaped
# "<session>:mx-<id>" form when no metadata state is available.
window_to_task() {
  local w=$1 state=${2:-${STATE:-${MX_STATE_OVERRIDE:-}}} meta mw mt t
  if [ -n "$state" ]; then
    for meta in "$state"/*.meta; do
      [ -e "$meta" ] || continue
      mw=$(grep '^window=' "$meta" 2>/dev/null | tail -1 | cut -d= -f2- || true)
      mt=$(grep '^terminal=' "$meta" 2>/dev/null | tail -1 | cut -d= -f2- || true)
      [ "$mw" = "$w" ] || [ "$mt" = "$w" ] || continue
      t=$(basename "$meta")
      t=${t%.meta}
      printf '%s' "$t"
      return 0
    done
  fi
  t="${w##*:}"; t="${t#mx-}"; printf '%s' "$t"
}

# 0 (actionable) if ANY status file listed in a "signal:" wake carries a
# maintainer-relevant last line; 1 otherwise. Pass the space-separated file list that
# follows the "signal:" prefix. Non-.status arguments (e.g. .turn-ended markers,
# which never carry a verb) are skipped. A 1 here is NOT "benign" on its own: a
# no-verb signal (a bare turn-end, a working: note) is only benign when the actor is
# also provably working (signal_actor_provably_working below); otherwise it surfaces.
signal_reason_is_actionable() {  # <file> ...
  local f last
  for f in "$@"; do
    [ -e "$f" ] || continue
    case "$f" in *.status) ;; *) continue ;; esac
    last=$(last_status_line "$f")
    [ -n "$last" ] || continue
    status_is_maintainer_relevant "$last" && return 0
  done
  return 1
}

# Classify WHY an idle/stale actors MIGHT be safely absorbed instead of surfaced,
# from bin/mx-actor-state.sh's one authoritative current-state line
# ("state: <s> · source: <src> · <detail>"). Prints exactly one token:
#   working - a native working verdict, an actively-running no-mistakes step
#             (running/fixing/ci), or a busy pane; the actor is legitimately
#             mid-work on a static-looking pane (e.g. waiting on CI);
#   paused  - the actor's authoritative current state is a declared external-wait
#             pause (paused:), which is EXPECTED to idle;
#   none    - neither, so the wake must surface (a stopped/finished/parked/failed/
#             torn-down/unknown actors, or an unreadable verdict).
# One mx-actor-state.sh read serves BOTH absorb reasons at once. Reading the state
# authoritatively (not the status log) is what applies the shared precedence:
# native working and an attributed run outrank an older paused report.
# NOT a pure read: mx-actor-state.sh may make a bounded no-mistakes call, so callers
# run it only on no-verb signal and first-sighting stale paths, never every wake.
# MX_ACTOR_STATE_BIN lets tests stub the verdict.
actor_absorb_class() {  # <id>
  local id=$1 line state src
  [ -n "$id" ] || { printf 'none'; return; }
  line=$("$MX_ACTOR_STATE_BIN" "$id" 2>/dev/null) || true
  case "$line" in state:*) ;; *) printf 'none'; return ;; esac
  state=${line#state: }; state=${state%% *}
  if [ "$state" = paused ]; then printf 'paused'; return; fi
  if [ "$state" = working ]; then
    src=${line#*source: }; src=${src%% *}
    case "$src" in native-event|run-step|pane) printf 'working'; return ;; esac
  fi
  printf 'none'
}

# 0 if actors <id> shows POSITIVE evidence it is still working (actor_absorb_class
# reports `working`). This is the "provably working" predicate at the heart of
# absorb-only-when-provably-working: a no-verb turn-end or stale wake is absorbed
# ONLY when this returns 0, and SURFACED otherwise (the actors may be done, waiting
# on a decision, or wedged). For stale panes it is checked before trusting the
# status log so a pre-validation maintainer-relevant line does not override an active
# run. See actor_absorb_class for the exact working/paused/none decision.
actor_is_provably_working() {  # <id>
  [ "$(actor_absorb_class "$1")" = working ]
}

# 0 if actors <id>'s authoritative current state is a declared external-wait pause.
# The stale path absorbs such an actor (on a long re-surface cadence) instead of
# escalating a possible wedge.
actor_is_paused() {  # <id>
  [ "$(actor_absorb_class "$1")" = paused ]
}

# 0 (benign/absorb) if EVERY task referenced by a no-verb "signal:" wake is provably
# working; 1 (actionable/surface) if any is not, or no task can be resolved. Pass the
# same space-separated file list as signal_reason_is_actionable. Files are mapped to
# task ids by stripping the .status / .turn-ended suffix; a no-verb wake with nothing
# provably working must surface, so an empty/unresolvable list returns 1.
signal_actor_provably_working() {  # <file> ...
  local f base task seen=""
  for f in "$@"; do
    base=${f##*/}
    case "$base" in
      *.status)     task=${base%.status} ;;
      *.turn-ended) task=${base%.turn-ended} ;;
      *)            continue ;;
    esac
    [ -n "$task" ] || continue
    case " $seen " in *" $task "*) continue ;; esac
    seen="$seen $task"
    actor_is_provably_working "$task" || return 1
  done
  [ -n "$seen" ] || return 1
  return 0
}

# 0 (terminal/actionable) if a stale window's last status line is
# maintainer-relevant; 1 otherwise, including the no-status case. A 1 only means
# "non-terminal"; the always-on watcher then applies actor_is_provably_working,
# while the away-mode daemon applies its persistence recheck.
stale_is_terminal() {  # <window> <state>
  local win=$1 state=$2 last
  last=$(last_status_line "$state/$(window_to_task "$win" "$state").status")
  [ -n "$last" ] && status_is_maintainer_relevant "$last"
}

# Print "<file>\t<task>\t<last-line>" for every state/*.status whose last line is
# maintainer-relevant. This is the cheap system-scan both supervisors run as a
# catch-all backstop for a maintainer-relevant status the per-wake path might miss.
# No dedup is applied here: each consumer dedupes against its own seen-state (the
# daemon against .subsuper-seen-status-*, the watcher against .seen-* signatures).
scan_maintainer_relevant_statuses() {  # <state>
  local state=$1 f last task
  for f in "$state"/*.status; do
    [ -e "$f" ] || continue
    last=$(last_status_line "$f")
    status_is_maintainer_relevant "$last" || continue
    task=$(basename "$f"); task="${task%.status}"
    printf '%s\t%s\t%s\n' "$f" "$task" "$last"
  done
  return 0
}
