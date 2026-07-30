#!/usr/bin/env bash
# Static contract tests for the local validation and delivery boundary.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

validate_contract() {
  awk '
    /^### Validate$/ { found = 1; next }
    found && /^### / { exit }
    found { print }
  ' "$ROOT/example_agents.md"
}

test_actor_stops_before_validation_and_delivery() {
  local contract
  contract=$(validate_contract)

  assert_contains "$contract" 'An actor ends implementation with a clean local branch and reports its full commit SHA.' \
    "Validate contract does not stop the actor at a local commit"
  assert_contains "$contract" 'neither the actor nor broker may synthesize a passing gate record' \
    "Validate contract permits an agent to forge validation"
  pass "Validate contract stops actors at the local validation handoff"
}

test_broker_never_runs_credentialed_delivery() {
  local contract
  contract=$(validate_contract)

  assert_contains "$contract" 'must not invoke the credentialed delivery or merge commands from its own agent session' \
    "Validate contract permits the broker to use remote-write credentials"
  pass "Validate contract keeps credentialed delivery outside the broker session"
}

test_actor_stops_before_validation_and_delivery
test_broker_never_runs_credentialed_delivery
