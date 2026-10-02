#!/usr/bin/env bash
# Human communication preserves evidence and questions without prescribed wording.
# shellcheck disable=SC2016
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
CONTRACT="$ROOT/AGENTS.md"
assert_present "$CONTRACT" 'operating contract missing'
assert_grep 'unresolved human questions durable, visible and bound to the task and revision' "$CONTRACT" 'unanswered questions lost'
assert_grep 'actual commit where applicable, exact checks and results, limitations and original artifacts' "$CONTRACT" 'quality evidence missing'
assert_grep 'actual current revision' "$CONTRACT" 'quality evidence lacks current revision'
assert_grep 'Evidence identifies the task, attempt, accepted brief' "$CONTRACT" 'evidence loses task and accepted brief binding'
assert_grep 'PR ready and human merged are separate facts' "$CONTRACT" 'readiness conflated with merge'
for file in "$CONTRACT" "$ROOT"/.agents/skills/*/SKILL.md; do
  assert_no_grep 'Maintainer, all clear.' "$file" 'fixed acknowledgement retained'
  assert_no_grep 'EXACTLY these four sections' "$file" 'fixed report shape retained'
  assert_no_grep "Use the maintainer's nouns:" "$file" 'fixed vocabulary retained'
done
assert_grep 'observation age' "$ROOT/.agents/skills/catchup/SKILL.md" 'snapshot freshness lost'
assert_grep 'An unrelated later message does not answer an earlier question' "$ROOT/.agents/skills/recap/SKILL.md" 'recap loses earlier open decisions'
pass 'human communication retains useful evidence without a compulsory response script'
