#!/usr/bin/env bash
# Compatibility source for real-Herdr tests.
# The production owner of the isolation, refuse-default, teardown, and
# system-state tripwire contract is bin/mx-herdr-lab.sh.
set -u

# Herdr backend tests drive the real mx-spawn/mx-teardown but do not source
# tests/lib.sh, so exempt them from the gate-lifecycle refusal here too (see
# tests/lib.sh and bin/mx-gate-refuse-lib.sh for why broker's own suite,
# which the no-mistakes gate runs from a gate worktree, must be exempt).
export MX_GATE_REFUSE_BYPASS=1

HERDR_TEST_SAFETY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
. "$HERDR_TEST_SAFETY_DIR/bin/mx-herdr-lab.sh"

herdr_refuse_if_default() { # <session>
  mx_herdr_lab_refuse_if_default "$1"
}

herdr_safe_stop_and_delete() { # <session>
  mx_herdr_lab_teardown "$1"
}
