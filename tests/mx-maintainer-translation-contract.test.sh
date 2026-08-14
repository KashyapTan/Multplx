#!/usr/bin/env bash
# Static regression tests for the maintainer-facing plain-English translation
# contract owned by AGENTS.md section 9.
# shellcheck disable=SC2016
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

AGENTS="$ROOT/AGENTS.md"
BOOTSTRAP="$ROOT/.agents/skills/bootstrap-diagnostics/SKILL.md"
AFK="$ROOT/.agents/skills/afk/SKILL.md"
DECISION="$ROOT/.agents/skills/decision-hold-lifecycle/SKILL.md"
RECOVERY="$ROOT/.agents/skills/stuck-actor-recovery/SKILL.md"
HARNESS="$ROOT/.agents/skills/harness-adapters/SKILL.md"
CODEXAPP="$ROOT/.agents/skills/multplx-codexapp/SKILL.md"
UPDATE="$ROOT/.agents/skills/updatemultplx/SKILL.md"
RECAP="$ROOT/.agents/skills/recap/SKILL.md"
README="$ROOT/README.md"

section_9() {
  awk '
    /^## 9\. Escalation and maintainer etiquette$/ { found = 1 }
    found && /^## 10\. / { exit }
    found { print }
  ' "$AGENTS"
}

test_section_9_owns_positive_translation_contract() {
  local contract
  contract=$(section_9)
  assert_contains "$contract" "Every maintainer-facing message must translate internal state into the project outcome, consequence, and next decision." \
    "section 9 does not own the positive maintainer-facing translation contract"
  assert_contains "$contract" "Use the maintainer's nouns:" \
    "section 9 does not require maintainer-owned nouns"
  assert_contains "$contract" "When evidence uses an internal label, rewrite it before sending:" \
    "section 9 does not own the rewrite mapping list"
  pass "section 9 owns the positive maintainer-facing translation contract"
}

test_scout_remains_allowed_house_vocabulary() {
  local contract
  contract=$(section_9)
  assert_contains "$contract" "Scout and daemon are accepted Multplx workflow terms and do not need translation" \
    "section 9 does not preserve scout as allowed Multplx vocabulary"
  assert_not_contains "$contract" "scout -> investigation" \
    "section 9 must not map scout to investigation"
  assert_not_contains "$contract" "scout, delivery" \
    "section 9 must not add scout to the internal-vocabulary ban"
  assert_not_contains "$contract" "daemon -> domain supervisor" \
    "section 9 must not map daemon to domain supervisor"
  pass "scout remains allowed in private maintainer chat"
}

test_compressed_safety_labels_have_plain_renderings() {
  local contract
  contract=$(section_9)
  for phrase in \
    "fail-closed" \
    "fails closed" \
    "fail-open" \
    "fails open" \
    "fail loudly"; do
    assert_contains "$contract" "$phrase" "section 9 does not cover compressed safety label '$phrase'"
  done
  assert_contains "$contract" "stops safely when something goes wrong" \
    "fail-closed behavior lacks a concrete plain rendering"
  assert_contains "$contract" "refuses rather than proceeding" \
    "fail-closed behavior lacks refusal wording"
  assert_contains "$contract" "steps aside and lets work continue when the check cannot complete" \
    "fail-open behavior lacks a concrete plain rendering"
  pass "compressed safety labels require concrete plain renderings"
}

test_mapping_list_covers_high_risk_internal_families() {
  local contract
  contract=$(section_9)
  for phrase in \
    "worktree, checkout, primary checkout, or local-main -> local copy" \
    "teardown -> cleanup" \
    "wake, watcher, heartbeat, stale, signal, or check -> notification" \
    "hold, gate, ask-user, needs-decision, blocked, or paused -> the concrete decision" \
    "done, failed, fix-review, checks-passed, cancelled, validation step, or pipeline state -> the concrete result" \
    "brief -> instructions" \
    "actor -> worker" \
    "harness, backend, runtime, or adapter -> worker runtime or tool" \
    "status file, metadata, state, task id, or raw path -> durable record"; do
    assert_contains "$contract" "$phrase" "section 9 mapping list is missing '$phrase'"
  done
  pass "section 9 maps high-risk internal vocabulary families"
}

test_verbatim_internal_evidence_is_rejected_from_chat() {
  local contract
  contract=$(section_9)
  assert_contains "$contract" "Never relay worker reports, status lines, tool output, validation-state labels, or decision records verbatim into maintainer chat." \
    "section 9 does not reject verbatim internal evidence in maintainer chat"
  assert_contains "$contract" "Private evidence reports may retain exact identifiers, paths, status lines, validation labels, and internal terms" \
    "section 9 does not preserve private evidence precision"
  assert_contains "$contract" "the maintainer-facing chat summary that points to the report still follows this translation rule" \
    "section 9 does not keep chat summaries plain English"
  pass "maintainer chat rejects verbatim internal evidence while private reports stay precise"
}

test_routine_no_action_response_is_event_scoped() {
  local contract
  contract=$(section_9)
  assert_contains "$contract" 'reply exactly `Maintainer, all clear.` without characterizing the visible session' \
    "section 9 does not require the exact event-scoped routine no-action response"
  assert_not_contains "$contract" 'Maintainer, no decision is needed.' \
    "section 9 implies the visible session has no unrelated open decisions"
  pass "routine no-action response is exact and scoped to its event"
}

test_outward_facing_skill_points_reference_section_9_owner() {
  assert_grep "using \`AGENTS.md\` section 9's maintainer-facing translation contract" "$BOOTSTRAP" \
    "bootstrap diagnostics do not reference section 9 at maintainer handoff"
  assert_grep "Acknowledge** in \`AGENTS.md\` section 9 language" "$AFK" \
    "afk acknowledgement does not reference section 9"
  assert_grep "Maintainer, away mode is active; I will batch routine updates" "$AFK" \
    "afk acknowledgement lacks a local plain-English example"
  assert_grep "as decisions from Catchup' Maintainer's Call section under \`AGENTS.md\` section 9" "$DECISION" \
    "decision relay does not reference section 9"
  assert_grep "using \`AGENTS.md\` section 9; do not mention metadata, harness, window, or worktree" "$RECOVERY" \
    "stuck-worker failure does not reference section 9"
  assert_grep "under \`AGENTS.md\` section 9 that the requested worker runtime is not verified yet" "$HARNESS" \
    "runtime fallback does not reference section 9"
  assert_grep "use broker's own verified runtime for current work" "$HARNESS" \
    "runtime fallback does not require the current-work fallback"
  assert_grep "Do not pause current work for that future-verification choice, and never launch an unverified adapter." "$HARNESS" \
    "runtime fallback permits waiting on future verification or launching an unverified adapter"
  assert_grep "translate status prefixes and return-channel evidence through \`AGENTS.md\` section 9" "$CODEXAPP" \
    "Codex Desktop result reporting does not reference section 9"
  assert_grep "under \`AGENTS.md\` section 9 without broker's internal vocabulary" "$UPDATE" \
    "Multplx update reporting does not reference section 9"
  pass "outward-facing skill handoffs point to the section 9 owner"
}

test_section_9_owner_is_not_duplicated_into_skills() {
  local duplicate_count file
  duplicate_count=0
  for file in "$BOOTSTRAP" "$AFK" "$DECISION" "$RECOVERY" "$HARNESS" "$CODEXAPP" "$UPDATE"; do
    if grep -Fq "When evidence uses an internal label, rewrite it before sending:" "$file"; then
      duplicate_count=$((duplicate_count + 1))
    fi
  done
  [ "$duplicate_count" -eq 0 ] || fail "skills duplicated section 9's mapping owner"
  pass "skills cross-reference section 9 instead of duplicating the mapping list"
}

test_recap_is_an_internal_user_invocable_skill() {
  assert_present "$RECAP" "recap skill is missing"
  assert_grep 'name: recap' "$RECAP" "recap skill metadata has the wrong name"
  assert_grep 'user-invocable: true' "$RECAP" "recap skill is not user-invocable"
  assert_grep '  internal: true' "$RECAP" "recap skill is not internal"
  [ ! -e "$ROOT/skills/recap" ] || fail "recap must not exist in the public installer-facing skills directory"
  pass "recap is internal, user-invocable, and absent from public skills"
}

test_recap_readme_uses_cross_harness_convention() {
  assert_grep 'Claude uses the slash form shown here; codex uses the same names with `$`' "$README" \
    "README lost the cross-harness slash and dollar convention"
  assert_grep '| `/recap`' "$README" "README built-in skills table does not list /recap"
  pass "README lists recap under the shared cross-harness invocation convention"
}

test_recap_owns_only_the_visible_session_recap() {
  assert_grep '[`../catchup/SKILL.md`](../catchup/SKILL.md)' "$RECAP" \
    "first-message fallback does not delegate to Catchup by relative pointer"
  assert_grep 'If no prior real maintainer message exists' "$RECAP" \
    "recap does not limit Catchup fallback to the first real maintainer message"
  assert_grep 'Catchup alone owns its gathering, artifact, and response contract.' "$RECAP" \
    "recap first-message fallback does not delegate to Catchup alone"
  assert_grep 'A maintainer boundary is an ordinary user-role message unless it matches one of the narrow operational exclusions below.' "$RECAP" \
    "recap lacks an explicit maintainer-authored boundary rule"
  assert_grep 'Exclude messages that begin with the current U+2063 `MULTPLX_OP:` injection prefix.' "$RECAP" \
    "recap does not exclude current marked operational injections"
  assert_grep 'Exclude legacy bare-marker away-mode injections only when U+2063 is immediately followed by `Supervisor escalate (`.' "$RECAP" \
    "recap does not narrowly exclude the legacy away-mode injection shape"
  assert_grep 'Exclude the exact legacy unmarked session-start payload ``Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.``' "$RECAP" \
    "recap does not exclude the legacy unmarked session-start payload"
  assert_grep 'quotes or embeds a current operational message after ordinary maintainer text' "$RECAP" \
    "recap lacks quoted-current near-miss protection"
  assert_grep 'Apply the current exclusion only when U+2063 `MULTPLX_OP:` begins at the first character of the whole message' "$RECAP" \
    "recap does not pin the current-prefix whole-message boundary"
  assert_grep 'contains ASCII `MULTPLX_OP:` without a leading U+2063' "$RECAP" \
    "recap lacks ASCII-only near-miss protection"
  assert_grep 'Apply the legacy startup exclusion as a literal whole-message match: ``Maintainer quote: Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.`` is a maintainer boundary.' "$RECAP" \
    "recap does not pin the altered-startup behavioral near miss"
  assert_grep 'System, developer, tool, watcher, guard, away-mode, and other injected operational messages are not maintainer messages.' "$RECAP" \
    "recap incorrectly treats synthetic operational messages as maintainer messages"
  assert_grep 'The normal recap branch is session-history-only.' "$RECAP" \
    "later recap invocation is not explicitly session-history-only"
  assert_grep 'Do not call Catchup, shell commands, system snapshots, status readers, GitHub or browser APIs, tools, or file reads or writes.' "$RECAP" \
    "normal recap does not prohibit fresh system, file, and tool reads"
  assert_grep 'Create no report, persist nothing' "$RECAP" \
    "normal recap does not prohibit artifacts and storage"
  assert_grep 'do not guess current live state beyond the last visible event' "$RECAP" \
    "normal recap may falsely claim a live snapshot"
  assert_grep 'The current `/recap` message is outside the recap interval.' "$RECAP" \
    "current recap invocation is not excluded from the recap interval"
  assert_grep 'If context compaction makes the prior boundary unavailable' "$RECAP" \
    "recap does not disclose an unavailable compacted boundary"
  assert_grep 'summarize only visibly supported events' "$RECAP" \
    "compacted fallback may invent unsupported events"
  assert_no_grep 'mx-status-snapshot.sh' "$RECAP" \
    "recap copied Catchup gathering mechanics instead of referencing its owner"
  assert_no_grep "Maintainer's Call" "$RECAP" \
    "recap copied Catchup response contract instead of referencing its owner"
  pass "recap delegates first-message fallback and keeps later recaps visible-session-only"
}

test_recap_scans_visible_history_for_open_decisions() {
  assert_grep 'preserve the ordinary recap interval: recap what happened after that message and before the current invocation.' "$RECAP" \
    "recap no longer preserves its ordinary recap interval"
  assert_grep 'inspect the entire session history visible to the current broker before the current invocation for every explicit maintainer decision that remains unanswered' "$RECAP" \
    "recap does not scan globally visible session history for open decisions"
  assert_grep 'including decisions raised before the ordinary recap boundary.' "$RECAP" \
    "recap does not include open decisions from before the recap boundary"
  assert_grep 'A later unrelated maintainer message establishes a recap boundary but does not close an earlier decision.' "$RECAP" \
    "recap lets unrelated maintainer messages close earlier decisions"
  assert_grep 'Treat a decision as closed only when a later visible response substantively resolves it, chooses an option, declines it, grants or denies the requested approval, or otherwise directly addresses that decision.' "$RECAP" \
    "recap lacks substantive-answer closure semantics"
  assert_grep 'Include every visibly supported open decision once, and deduplicate by the decision' "$RECAP" \
    "recap does not include and deduplicate visibly open decisions"
  assert_grep "substance when the ordinary interval recap already represents it or its wording differs." "$RECAP" \
    "recap deduplicates decisions by wording instead of substance"
  assert_grep 'If no ordinary events occurred after the previous maintainer message but an older visibly open decision exists, report that decision instead of claiming nothing happened.' "$RECAP" \
    "recap can incorrectly claim nothing happened while an older decision is open"
  assert_grep 'Compacted history supports an open decision only when both its request and its still-unanswered status are visible' "$RECAP" \
    "recap does not limit compacted decision reporting to visible support"
  assert_grep 'report uncertainty instead of reconstructing hidden requests or answers.' "$RECAP" \
    "recap may reconstruct hidden decision history after compaction"
  pass "recap adds visibly open decisions without changing the ordinary recap boundary"
}

test_recap_user_role_injections_share_one_marker() {
  local pi_guard pi_watch owner nudge_body nudge kind
  pi_guard=$(cat "$ROOT/.pi/extensions/mx-primary-turnend-guard.ts")
  pi_watch=$(cat "$ROOT/.pi/extensions/mx-primary-pi-watch.ts")
  owner=$(cat "$ROOT/bin/mx-operational-input.sh")

  assert_contains "$owner" 'MX_OPERATIONAL_PREFIX="${MX_OPERATIONAL_MARK}MULTPLX_OP: "' \
    "canonical owner lost the landed Recap prefix"
  nudge_body='Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.'
  nudge=$(printf '%s' "$nudge_body" | "$ROOT/bin/mx-operational-input.sh" encode session-start) \
    || fail "canonical owner could not construct the session-start message"
  kind=$(printf '%s' "$nudge" | "$ROOT/bin/mx-operational-input.sh" kind) \
    || fail "canonical owner could not classify the constructed session-start message"
  [ "$kind" = session-start ] || fail "constructed session-start message had kind $kind"
  [ "$(printf '%s' "$nudge" | "$ROOT/bin/mx-operational-input.sh" body)" = "$nudge_body" ] \
    || fail "constructed session-start message changed its body"
  assert_grep 'native supervisor did not submit a typed away-supervisor escalation' \
    "$ROOT/tests/mx-supervise-daemon-native.test.sh" \
    "native away-supervisor producer lost its black-box typed-input contract"
  assert_contains "$pi_guard" 'encodeMultplxOperationalInput(' \
    "Pi guard does not use the cross-language constructor"
  assert_contains "$pi_guard" '"turn-end-guard"' \
    "Pi guard does not retain its exact current kind"
  assert_contains "$pi_watch" '"watcher"' \
    "Pi watcher does not retain its exact current kind"
  assert_grep 'encode launch-brief' \
    "$ROOT/tests/mx-spawn-dispatch-profile.test.sh" \
    "cross-harness launches lost their black-box typed launch-input contract"
  for producer in "$pi_guard" "$pi_watch"; do
    assert_not_contains "$producer" 'MULTPLX_OP: ' \
      "a current producer copied the canonical marker grammar"
  done
  pass "recap: one canonical owner constructs typed operational input for every Multplx-controlled user-role producer"
}

test_section_9_owns_positive_translation_contract
test_scout_remains_allowed_house_vocabulary
test_compressed_safety_labels_have_plain_renderings
test_mapping_list_covers_high_risk_internal_families
test_verbatim_internal_evidence_is_rejected_from_chat
test_routine_no_action_response_is_event_scoped
test_outward_facing_skill_points_reference_section_9_owner
test_section_9_owner_is_not_duplicated_into_skills
test_recap_is_an_internal_user_invocable_skill
test_recap_readme_uses_cross_harness_convention
test_recap_owns_only_the_visible_session_recap
test_recap_scans_visible_history_for_open_decisions
test_recap_user_role_injections_share_one_marker
