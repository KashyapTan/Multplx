#!/usr/bin/env bash
# Compatibility source for real-Herdr tests.
# The production owner of the isolation, refuse-default, teardown, and
# system-state tripwire contract is bin/mx-herdr-lab.sh.
set -u

# Herdr backend tests drive the real mx-spawn/mx-teardown but do not source
# tests/lib.sh, so exempt them from the gate-lifecycle refusal here too (see
# tests/lib.sh and bin/mx-gate-refuse-lib.sh for why broker's own suite,
# which the deep-review gate runs from a gate worktree, must be exempt).
export MX_GATE_REFUSE_BYPASS=1

# Real-Herdr suites exercise backend lifecycle, not dispatch-capacity policy.
# Pin explicit ample signals so host load cannot turn a spawn assertion into a
# queued-dispatch assertion; mx-headroom.test.sh owns capacity behavior.
export MX_HEADROOM_CPU_COUNT=8
export MX_HEADROOM_LOAD1=0
export MX_HEADROOM_MEM_AVAILABLE_BYTES=34359738368
export MX_HEADROOM_IN_USE=0
export MX_HEADROOM_API_CAPACITY=8

HERDR_TEST_SAFETY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
. "$HERDR_TEST_SAFETY_DIR/bin/mx-herdr-lab.sh"

herdr_refuse_if_default() { # <session>
  mx_herdr_lab_refuse_if_default "$1"
}

herdr_safe_stop_and_delete() { # <session>
  mx_herdr_lab_teardown "$1"
}
