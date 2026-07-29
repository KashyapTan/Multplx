#!/usr/bin/env bash
# Static contract tests for conditional instruction owners introduced before the
# AGENTS.md reduction pass.
# shellcheck disable=SC2016
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DIAG="$ROOT/.agents/skills/diagnostic-reasoning/SKILL.md"
PROJECT="$ROOT/.agents/skills/project-management/SKILL.md"
HARNESS="$ROOT/.agents/skills/harness-adapters/SKILL.md"
CODING="$ROOT/.agents/skills/multplx-coding-guidelines/SKILL.md"
RECOVERY="$ROOT/.agents/skills/stuck-actor-recovery/SKILL.md"
DAEMON="$ROOT/.agents/skills/daemon-provisioning/SKILL.md"
CONFIG="$ROOT/docs/configuration.md"
AGENTS="$ROOT/example_agents.md"
BRIEF="$ROOT/bin/mx-brief.sh"
BOOTSTRAP="$ROOT/bin/mx-bootstrap.sh"

test_new_skill_metadata_and_triggers() {
  local skill name count
  for pair in "diagnostic-reasoning:$DIAG" "project-management:$PROJECT"; do
    name=${pair%%:*}
    skill=${pair#*:}
    assert_present "$skill" "$name skill is missing"
    assert_grep "name: $name" "$skill" "$name skill metadata has the wrong name"
    assert_grep "user-invocable: false" "$skill" "$name skill must not be user-invocable"
    assert_grep "  internal: true" "$skill" "$name skill must be internal"
    count=$(grep -Fc -- "- \`$name\` -" "$AGENTS")
    [ "$count" -eq 1 ] || fail "$name must have exactly one AGENTS.md trigger entry, found $count"
  done
  assert_grep 'Use before scoping a reported bug and before acting on a diagnostic report.' "$DIAG" \
    "diagnostic skill metadata lost its precise load trigger"
  assert_grep '`diagnostic-reasoning` - load before scoping a reported bug and before acting on a diagnostic report.' "$AGENTS" \
    "AGENTS.md lost the diagnostic-reasoning trigger"
  assert_grep 'Use before adding, creating, removing, or initializing a project.' "$PROJECT" \
    "project-management skill metadata lost its precise load trigger"
  assert_grep '`project-management` - load before adding, creating, removing, or initializing a project.' "$AGENTS" \
    "AGENTS.md lost the project-management trigger"
  pass "new internal skills have one precise AGENTS.md trigger each"
}

test_diagnostic_owner_covers_causal_procedure() {
  assert_grep "single owner of Multplx's bug-diagnosis reasoning procedure" "$DIAG" \
    "diagnostic skill does not declare ownership"
  for phrase in \
    "end-to-end reproduction aligned with the real user path" \
    "initiating trigger" \
    "masking condition" \
    "visible symptom" \
    "proven path" \
    "relevant history" \
    "smallest counterfactual" \
    "disconfirming evidence"; do
    assert_grep "$phrase" "$DIAG" "diagnostic owner is missing '$phrase'"
  done
  assert_grep "evidence, not authorization to change code" "$DIAG" \
    "diagnostic owner lost the diagnosis-only authority boundary"
  pass "diagnostic-reasoning owns the approved evidence procedure"
}

test_project_management_owner_covers_guarded_operations() {
  assert_grep "single owner of Multplx's project-management procedure" "$PROJECT" \
    "project-management skill does not declare ownership"
  for phrase in \
    'bin/mx-project-mode.sh' \
    '`no-mistakes`' \
    '`direct-PR`' \
    '`local-only`' \
    'Default it off' \
    'Creating a GitHub repository is outward-facing.' \
    "maintainer's explicit consent" \
    'Never issue a raw removal command from Multplx.' \
    'no-mistakes init && no-mistakes doctor'; do
    assert_grep "$phrase" "$PROJECT" "project-management owner is missing '$phrase'"
  done
  pass "project-management owns registry, delivery posture, consent, initialization, and removal safety"
}

test_generic_effort_fallback_respects_precedence() {
  local section
  section=$(awk '
    /^Effort precedence is / { found = 1 }
    found && /^The supported launch-profile flags / { exit }
    found { print }
  ' "$HARNESS")
  assert_contains "$section" "explicit per-task maintainer instruction first" \
    "effort rubric lost per-task maintainer precedence"
  assert_contains "$section" "standing dispatch profile or daemon pin" \
    "effort rubric lost standing configuration precedence"
  assert_contains "$section" 'Use `low` for well-understood work' \
    "effort rubric lost its low fallback"
  assert_contains "$section" '`xhigh` for ambiguous investigation or design' \
    "effort rubric lost its xhigh fallback"
  assert_contains "$section" "Choose intermediate levels proportionally" \
    "effort rubric lost proportional intermediate levels"
  assert_contains "$section" 'Never select `max` from this fallback' \
    "effort rubric permits max without an explicit maintainer preference"
  if printf '%s\n' "$section" | grep -qi sol; then
    fail "generic effort fallback must not contain Sol-specific policy"
  fi
  pass "generic effort fallback applies only below maintainer and standing configuration"
}

test_agent_owned_quota_array_dispatch_contract() {
  local phrase
  for phrase in \
    'The broker alone resolves a matched profile array' \
    'run `quota-axi --json` at that intake' \
    'evaluate every configured candidate against that current output' \
    'choose the candidate with the most real headroom' \
    'if any harness/model/provider relationship, applicable quota data, or interpretation cannot be established, stop and report that candidate' \
    'instead of omitting it, guessing, falling back, or calling the result quota-informed' \
    'Preserve malformed profile configuration as an actionable error' \
    "preserve the maintainer's strongest-reasoning class rather than silently downgrading it" \
    'Break genuine headroom ties without array-order or harness bias' \
    '`quota-axi` owns how model or product windows relate to bounding account windows' \
    'explicitly interim rule until successor `quota-axi-interpretation-hints-h3` lands'; do
    assert_grep "$phrase" "$AGENTS" "array-dispatch contract lost '$phrase'"
  done

  for phrase in \
    '| claude | Open the current interactive session' \
    '| codex | Open the current interactive session' \
    '| pi | Run `pi --list-models [search]`' \
    "For an unfamiliar harness or model namespace, establish support and provider identity from that harness's authoritative CLI help, model listing, or current documentation rather than guessing" \
    'If those sources do not establish the relationship needed for dispatch, fail loudly and report the unresolved candidate.'; do
    assert_grep "$phrase" "$HARNESS" "model discovery guidance lost '$phrase'"
  done
  assert_grep 'not as a permanent namespace or provider mapping' "$HARNESS" \
    "model discovery guidance permits a fixed provider table"
  assert_grep '`example_agents.md` section 4 owns the future dispatch and array-selection procedure.' "$CONFIG" \
    "configuration docs do not point to the agent-owned array procedure"
  assert_grep 'quota-axi is required for the' "$BOOTSTRAP" \
    "bootstrap docs lost the quota-axi dependency pointer"
  assert_grep 'agent-owned dispatch-profile array procedure in example_agents.md section 4.' "$BOOTSTRAP" \
    "bootstrap docs do not point to the agent-owned array procedure"
  pass "broker directly compares every quota candidate with authoritative model discovery"
}

test_shared_authoring_requirements_are_owned() {
  assert_grep "review every affected supported primary harness and runtime backend" "$CODING" \
    "coding guidance lost the supported compatibility matrix review"
  assert_grep "prefer deterministic and idempotent enforcement over relying on agent memory alone" "$CODING" \
    "coding guidance lost deterministic idempotent enforcement"
  assert_grep "critical safety, routing, startup, and supervision infrastructure" "$CODING" \
    "coding guidance lost the critical infrastructure scope"
  pass "multplx-coding-guidelines owns compatibility review and deterministic enforcement"
}

test_daemon_registry_contract_stays_concise() {
  local guidance routing_section schema_line
  routing_section=$(awk '
    /^## Routing table$/ { found = 1 }
    found && /^## Charter and seed$/ { exit }
    found { print }
  ' "$DAEMON")
  guidance=$(awk '
    /^## Routing table$/ { found = 1 }
    found && /^## Backlog handoff$/ { exit }
    found { print }
  ' "$DAEMON")
  schema_line="- <id> - <one-sentence charter summary> (home: <absolute-home-path>; scope: <natural-language responsibility>; projects: <project-a>, <project-b>; added <date>)"
  assert_contains "$routing_section" "$schema_line" \
    "daemon routing table lost the parser-compatible single-line schema"
  assert_contains "$routing_section" "Each registry entry stays concise and single-line" \
    "daemon routing table no longer requires concise single-line entries"
  assert_contains "$routing_section" "genuinely domain-specific hard rules" \
    "daemon routing table no longer limits extra prose to domain-specific hard rules"
  assert_contains "$routing_section" "The home-seeded \`data/charter.md\` is the sole owner of boilerplate idle-by-default behavior, the normal delegation lifecycle, and standard escalation contracts" \
    "daemon routing table lost the explicit charter ownership pointer"
  assert_contains "$routing_section" "no extra registry pointer field is needed" \
    "daemon routing table no longer explains why the existing home field is the charter pointer"
  for phrase in \
    "go idle and wait silently" \
    "Act only on tasks" \
    "never spawn a survey" \
    "run normal broker bootstrap" \
    "escalation back to the main broker status file" \
    "requests-from-main-broker contract" \
    "waits for routed tasks, never self-initiating a survey or audit" \
    "marked supervisor requests return through status" \
    "unmarked maintainer messages stay conversational"; do
    if printf '%s\n' "$guidance" | grep -F "$phrase" >/dev/null; then
      fail "daemon provisioning guidance restated charter boilerplate: $phrase"
    fi
  done
  pass "daemon registry guidance keeps concise routes and points to the charter"
}

test_state_startup_and_ordinary_recovery_placement() {
  assert_grep "single owner of the top-level operational-home layout" "$CONFIG" \
    "configuration docs do not own the operational state layout"
  assert_grep "header is the single owner of session-start ordering" "$CONFIG" \
    "session-start mechanism is not assigned to the script header"
  assert_grep "Ordinary dead-direct-report recovery is owned by \`stuck-actor-recovery\`" "$CONFIG" \
    "D05 ordinary recovery placement is missing"
  assert_grep "## Session-start reconciliation for a dead actor" "$RECOVERY" \
    "stuck-actor-recovery lacks the dead ordinary direct-report procedure"
  assert_grep "treehouse status" "$RECOVERY" \
    "ordinary recovery lost treehouse inventory inspection"
  assert_grep "session-start digest reports an actor endpoint dead or its metadata has no window" "$AGENTS" \
    "AGENTS.md does not trigger ordinary dead-report recovery"
  pass "state, startup, and ordinary recovery have focused owners and triggers"
}

test_compressed_agents_owner_map() {
  assert_grep '`docs/configuration.md` is the single owner of the top-level operational-home layout' "$AGENTS" \
    "AGENTS.md lost the state-layout owner pointer"
  assert_grep 'header is the single owner of composed commands, ordering, and digest contents' "$AGENTS" \
    "AGENTS.md lost the session-start owner pointer"
  assert_grep '`docs/configuration.md` owns dispatch-profile and runtime-backend schemas' "$AGENTS" \
    "AGENTS.md lost the dispatch-schema owner pointer"
  assert_grep 'That skill owns registry syntax, delivery-mode selection' "$AGENTS" \
    "AGENTS.md lost the project-management owner pointer"
  assert_grep 'The delivery lifecycle is an always-loaded operational contract' "$AGENTS" \
    "AGENTS.md no longer owns the delivery lifecycle"
  assert_grep 'System monitoring is an always-loaded operational contract' "$AGENTS" \
    "AGENTS.md no longer owns system monitoring"
  assert_grep '`.tasks.toml`, `docs/configuration.md`, and current `tasks-axi --help` own the backlog schema' "$AGENTS" \
    "AGENTS.md lost the backlog-mechanics owner pointer"
  assert_grep '`bin/mx-brief.sh` and its help own scaffold syntax' "$AGENTS" \
    "AGENTS.md lost the brief-mechanics owner pointer"
  pass "compressed AGENTS.md records the approved one-owner map"
}

test_intake_reuses_evidence_and_parallelizes_safe_work() {
  for phrase in \
    'consult existing reports and established evidence' \
    'remaining bounded research inside it' \
    'unresolved uncertainty could materially change whether or what to build' \
    'relay it without a design-only scout' \
    'ask one concise implementation question when useful' \
    'Never both present a likely-enough solution' \
    'overlap as a risk signal rather than an automatic reason to wait' \
    'independently implemented and validated' \
    'selected delivery path can reconcile ordinary rebases or conflicts' \
    'Serialize only for a true semantic dependency' \
    'shared mutable external state' \
    'incompatible concurrent migration' \
    'same-file editing alone is insufficient' \
    'genuine blockers remain durable'; do
    assert_grep "$phrase" "$AGENTS" "intake contract lost '$phrase'"
  done
  assert_grep 'dispatch isolated work immediately with no concurrency cap' "$AGENTS" \
    "intake contract lost unbounded safe parallel dispatch"
  assert_grep 'maintainer explicitly requests a separate knowledge or design deliverable' "$AGENTS" \
    "intake contract lost maintainer-requested separate scouts"
  assert_grep 'When implementation is separately authorized, promote the existing scout' "$AGENTS" \
    "intake contract lost genuine scout promotion"
  pass "intake reuses evidence, reserves scouts for uncertainty, and parallelizes safe work"
}

test_compressed_agents_retains_authority_and_supervision_safety() {
  for phrase in \
    'A lock-refused session must not spawn, steer, merge, drain the wake queue' \
    'A diagnostic request, report, recommendation, or implementation-ready finding is evidence, not authorization to change code.' \
    'The selected delivery path owns its own rigor.' \
    'When no-mistakes is selected, no-mistakes alone owns review, fixes, tests, documentation, push, PR, and CI; otherwise follow the faster path without adding an independent reviewer.' \
    'Never hold work outside no-mistakes for a manual clean verdict, stack serial manual reviews, or infer authority for one from security, architecture, or risk alone.' \
    'A separate review or audit is allowed only when the maintainer explicitly requests that deliverable or the authorized task is a knowledge-only review; one named question remains scoped to that question.' \
    'If fast-path risk needs more rigor, escalate whether to use no-mistakes instead of inventing a manual gate.' \
    '**local-only** has the worker stop with a clean ready branch, then waits for the configured merge authority' \
    'A status line is a wake event, not current state' \
    'keep exactly one live monitoring cycle' \
    'Never broadly kill watchers' \
    'While `state/.afk` exists, the daemon owns supervision'; do
    assert_grep "$phrase" "$AGENTS" "compressed AGENTS.md lost safety phrase '$phrase'"
  done
  assert_no_grep 'Multplx does not personally review code or deliverables' "$AGENTS" \
    "AGENTS.md retained the weaker duplicate review prohibition"
  assert_no_grep 'broker reviews your branch' "$AGENTS" \
    "AGENTS.md retained a personal branch-review requirement"
  assert_no_grep 'broker reviews, maintainer approves' "$BRIEF" \
    "generated brief retained a stacked personal-review requirement"
  if grep -q "$(printf '\342\200\224')" "$AGENTS"; then
    fail "AGENTS.md contains an em dash"
  fi
  pass "compressed AGENTS.md retains authority, supervision, AFK, and X safety"
}

test_new_skill_metadata_and_triggers
test_diagnostic_owner_covers_causal_procedure
test_project_management_owner_covers_guarded_operations
test_generic_effort_fallback_respects_precedence
test_agent_owned_quota_array_dispatch_contract
test_shared_authoring_requirements_are_owned
test_daemon_registry_contract_stays_concise
test_state_startup_and_ordinary_recovery_placement
test_compressed_agents_owner_map
test_intake_reuses_evidence_and_parallelizes_safe_work
test_compressed_agents_retains_authority_and_supervision_safety
