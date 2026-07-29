#!/usr/bin/env bash
# tests/mx-signal-precedence.test.sh - the pure conflict matrix for the shared
# signal resolver in bin/mx-classify-lib.sh.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
# shellcheck source=/dev/null
. "$ROOT/bin/mx-classify-lib.sh"

resolve_is() {  # <expected> <native> <run-step> <self-report> <heuristic>
  local expected=$1
  shift
  local actual
  actual=$(mx_signal_resolve "$@")
  [ "$actual" = "$expected" ] \
    || fail "mx_signal_resolve $* returned '$actual', expected '$expected'"
}

# Native runtime evidence outranks every lower tier, including an attributed
# validation run that is still active.
resolve_is native:done done "" "" busy
resolve_is native:blocked blocked working paused ""
resolve_is native:blocked blocked working working busy
resolve_is native:working working done done idle

# Attributed validation evidence outranks reports and pane text when no native
# runtime verdict is present.
resolve_is run-step:working "" working blocked idle
resolve_is run-step:done "" done working busy

# A schema-valid report outranks the regex/pane heuristic.
resolve_is self-report:blocked "" "" blocked busy
resolve_is self-report:done "" "" done busy
resolve_is self-report:paused "" "" paused idle

# The heuristic remains useful when every stronger tier is empty or unknown.
resolve_is heuristic:busy "" "" "" busy
resolve_is heuristic:idle unknown unknown malformed idle
resolve_is none "" "" "" ""
resolve_is none unknown unknown malformed unknown

pass "mx_signal_resolve applies native > run-step > validated report > heuristic across the conflict matrix"

grep -F "native event > schema-validated self-report > text/regex heuristic" \
  "$ROOT/bin/mx-classify-lib.sh" >/dev/null \
  || fail "mx-classify-lib.sh does not document the Plan 5 signal order"
pass "the classifier header documents the Plan 5 signal order"

echo "all mx-signal-precedence tests passed"
