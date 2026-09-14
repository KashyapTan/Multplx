#!/usr/bin/env bash
# Lean scope clarification replaces role/yolo-specific reviewer authority policy.
# Existing command/record security remains covered by decision/override runtime tests.
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
CONTRACT="$ROOT/AGENTS_E.md"
assert_grep 'accepted request and its acceptance criteria' "$CONTRACT" 'accepted scope omitted'
assert_grep 'genuinely missing scope decisions' "$CONTRACT" 'real clarification omitted'
assert_grep 'keep independent work moving' "$CONTRACT" 'one decision stalls unrelated work'
assert_grep 'A delayed answer or old result remains historical' "$CONTRACT" 'stale decision target lost'
assert_grep 'not approval ranks' "$CONTRACT" 'role privileges reintroduced'
assert_no_grep 'implementation worker never answers its own finding' "$CONTRACT" 'reviewer rank policy retained'
assert_absent "$ROOT/.agents/skills/ask-user-authority/SKILL.md" 'universal finding policy still injected'
pass 'scope and revision remain binding without a universal reviewer-authority procedure'
