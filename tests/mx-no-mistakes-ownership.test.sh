#!/usr/bin/env bash
# Static contract tests for actor-owned no-mistakes validation runs.
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

test_worker_owns_synchronous_driver() {
  local contract
  contract=$(validate_contract)

  assert_contains "$contract" 'The task worker that starts a no-mistakes run drives the pipeline' \
    "Validate contract does not assign the run to its initiating task worker"
  assert_contains "$contract" "owns every \`no-mistakes axi run\` and \`no-mistakes axi respond\` call through the next gate or outcome" \
    "Validate contract does not assign every synchronous driver call to the task worker"
  assert_contains "$contract" 'process every synchronous return until completion or a genuinely new escalation' \
    "Validate contract does not require the task worker to process every synchronous return"
  pass "Validate contract assigns the complete synchronous driver loop to the initiating task worker"
}

test_broker_never_responds_for_actor_run() {
  local contract
  contract=$(validate_contract)

  assert_contains "$contract" "The broker never invokes \`no-mistakes axi respond\` for an actor-owned run." \
    "Validate contract permits Multplx to respond directly for an actor-owned run"
  pass "Validate contract forbids Multplx from responding directly for an actor-owned run"
}

test_worker_owns_synchronous_driver
test_broker_never_responds_for_actor_run
