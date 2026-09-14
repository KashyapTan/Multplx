#!/usr/bin/env bash
# Phase 01 instruction ownership and skill-discovery contract.
# shellcheck disable=SC2016
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
AGENTS="$ROOT/AGENTS_E.md"
assert_absent "$ROOT/AGENTS.md" 'development contract must remain dormant'
assert_grep 'Delegate project implementation, code fixes and test-code changes' "$AGENTS" 'orchestrator boundary lost'
assert_grep 'Any agent may delegate' "$AGENTS" 'nested delegation missing'
assert_grep 'A1-A11' "$AGENTS" 'shared architecture discovery lost'
assert_grep 'Discovery roots are optional' "$AGENTS" 'launch narrowed to dev root'
assert_grep 'three repositories creates three scoped tasks in the same chat' "$AGENTS" 'multi-repo intake missing'
assert_grep 'starting revision' "$AGENTS" 'checkout binding omitted'
assert_grep 'built-in worktree manager' "$AGENTS" 'allocation owner omitted'
assert_grep 'recorded parent route' "$AGENTS" 'coordinator parent omitted'
assert_grep 'lock-refused session' "$AGENTS" 'single writer boundary lost'
assert_grep 'A status line is a wake event, not current state' "$AGENTS" 'event/state distinction lost'
assert_grep 'Retain uncommitted, unlanded or uncertain work' "$AGENTS" 'recovery loses work'
assert_grep 'Operational message markers' "$AGENTS" 'message framing omitted'
assert_grep 'Only humans merge PRs' "$AGENTS" 'human merge boundary lost'
assert_grep 'Deep-review and vplan are opt-in' "$AGENTS" 'optional tools became default'
[ "$(readlink "$ROOT/.claude/skills")" = ../.agents/skills ] || fail 'Claude skill discovery broken'
for removed in multplx-coding-guidelines diagnostic-reasoning ask-user-authority maintainer-override decision-hold-lifecycle bootstrap-diagnostics stuck-actor-recovery daemon-provisioning; do
  assert_absent "$ROOT/.agents/skills/$removed" 'retired skill still discoverable'
done
for kept in harness-adapters subagent-recovery persistent-subagents project-management create-workflow afk catchup recap stow updatemultplx multplx-codexapp; do
  file="$ROOT/.agents/skills/$kept/SKILL.md"
  assert_present "$file" 'retained operational skill missing'
  assert_grep "name: $kept" "$file" 'skill name mismatched'
  assert_grep 'internal: true' "$file" 'internal skill exposed to standalone installer'
  assert_grep "\`$kept\`" "$AGENTS" 'skill not discoverable from contract'
  for removed in diagnostic-reasoning ask-user-authority maintainer-override decision-hold-lifecycle multplx-coding-guidelines; do
    assert_no_grep "$removed" "$file" 'removed procedure reintroduced through retained skill'
  done
done
assert_grep 'CONTRIBUTING.md' "$ROOT/.deep-review.yaml" 'Document step lost convention owner'
assert_no_grep 'skills/multplx-coding-guidelines' "$ROOT/.deep-review.yaml" 'Document step loads retired skill'
assert_grep 'dormant product source' "$ROOT/.cursor/rules/multplx.mdc" 'Cursor activates dormant contract'
assert_no_grep 'never through Cursor subagents' "$ROOT/.cursor/rules/multplx.mdc" 'Cursor prompt bans native delegation'
assert_grep 'Never generate a per-workflow script' "$ROOT/.agents/skills/create-workflow/SKILL.md" 'workflow schema owner lost'
pass 'lean contract, skill dispositions and cross-harness instruction discovery'
