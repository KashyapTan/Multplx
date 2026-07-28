#!/usr/bin/env bash
# tests/mx-transition-lib.test.sh - unit tests for the shared, backend-neutral
# normalized-transition shape and the single-owner status->action policy table
# (bin/mx-transition-lib.sh). Pure functions, no backend required.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# shellcheck source=/dev/null
. "$ROOT/bin/mx-transition-lib.sh"

# --- record construction + accessors ----------------------------------------

REC=$(mx_transition_record "wG:pQ" "wG" "" "blocked" "claude")
[ "$(mx_transition_pane_id "$REC")" = "wG:pQ" ] || fail "pane_id accessor wrong: $REC"
[ "$(mx_transition_workspace_id "$REC")" = "wG" ] || fail "workspace_id accessor wrong: $REC"
[ "$(mx_transition_from_status "$REC")" = "" ] || fail "from_status should be empty: $REC"
[ "$(mx_transition_to_status "$REC")" = "blocked" ] || fail "to_status accessor wrong: $REC"
[ "$(mx_transition_agent "$REC")" = "claude" ] || fail "agent accessor wrong: $REC"
pass "mx_transition_record builds a 5-field record and every accessor reads its field"

# The record is exactly TAB-separated (five fields, four tabs).
TABS=$(printf '%s' "$REC" | tr -cd '\t' | wc -c | tr -d '[:space:]')
[ "$TABS" = "4" ] || fail "record must have exactly 4 TAB separators, got $TABS"
pass "mx_transition_record uses a single TAB between each of the five fields"

# A field containing a stray TAB/newline is scrubbed to spaces so the record
# never desyncs into more than five fields.
DIRTY=$(mx_transition_record "wG:pQ" "wG" "" "blocked" $'multi\tline\nagent')
DIRTY_TABS=$(printf '%s' "$DIRTY" | tr -cd '\t' | wc -c | tr -d '[:space:]')
[ "$DIRTY_TABS" = "4" ] || fail "a field with a stray TAB must not add columns, got $DIRTY_TABS tabs"
[ "$(mx_transition_to_status "$DIRTY")" = "blocked" ] || fail "stray-field scrub desynced to_status: $DIRTY"
pass "mx_transition_record scrubs TAB/newline out of fields so the record stays exactly five columns"

# Empty optional fields are allowed (herdr leaves workspace/agent empty on the
# reconcile path).
REC2=$(mx_transition_record "w1:p3" "" "" "working" "")
[ "$(mx_transition_pane_id "$REC2")" = "w1:p3" ] || fail "pane_id wrong with empty optionals: $REC2"
[ "$(mx_transition_to_status "$REC2")" = "working" ] || fail "to_status wrong with empty optionals: $REC2"
pass "mx_transition_record tolerates empty workspace/from/agent fields"

# --- the single-owner policy table ------------------------------------------

[ "$(mx_transition_policy blocked)" = "actionable" ] || fail "blocked must be actionable"
[ "$(mx_transition_policy working)" = "absorb" ] || fail "working must be absorb"
[ "$(mx_transition_policy idle)" = "defer" ] || fail "idle must be defer"
[ "$(mx_transition_policy "done")" = "defer" ] || fail "done must be defer"
[ "$(mx_transition_policy unknown)" = "fallback" ] || fail "unknown must be fallback"
[ "$(mx_transition_policy "")" = "fallback" ] || fail "empty status must be fallback"
[ "$(mx_transition_policy some-future-status)" = "fallback" ] || fail "an unrecognized status must be fallback"
pass "mx_transition_policy is the single-owner status->action table (blocked=actionable, working=absorb, idle/done=defer, else=fallback)"

echo "# mx-transition-lib.test.sh: all assertions passed"
