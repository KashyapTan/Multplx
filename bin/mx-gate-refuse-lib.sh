#!/usr/bin/env bash
# Fail-closed refusal for Multplx lifecycle entrypoints in a deep-review turn.
#
# Every headless validation invocation carries DEEP_REVIEW_GATE=1.
# A validation agent may inspect and edit only its task worktree; it must never
# spawn, steer, tear down, or session-start the surrounding Multplx system.
#
# The old external-gate checkout path signal deliberately does not survive.
# deep-review runs in the actor's existing worktree, so DEEP_REVIEW_GATE is the
# exact successor safeguard at the lifecycle chokepoints.
#
# MX_GATE_REFUSE_BYPASS=1 remains a test-only escape hatch so behavior suites
# can execute real lifecycle scripts while exercising gate fixtures.
# No side effects on source.

MX_GATE_REFUSE_EXIT=3

mx_is_gate_agent() {
  if [ "${MX_GATE_REFUSE_BYPASS:-}" = 1 ]; then
    return 1
  fi
  [ "${DEEP_REVIEW_GATE+x}" = x ]
}

mx_refuse_if_gate_agent() {
  mx_is_gate_agent || return 0
  echo "error: deep-review agent must not drive Multplx lifecycle (DEEP_REVIEW_GATE set)" >&2
  exit "$MX_GATE_REFUSE_EXIT"
}
