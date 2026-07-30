#!/usr/bin/env bash
# Static contract for the tracked deep-review configuration.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

CONFIG="$ROOT/.deep-review.yaml"
[ -f "$CONFIG" ] || fail ".deep-review.yaml is missing"
git -C "$ROOT" check-ignore -q .deep-review.yaml \
  && fail ".deep-review.yaml is ignored instead of trackable"
assert_grep 'allow_repo_commands: false' "$CONFIG" \
  "repository config does not default branch commands off"
assert_grep 'disable_project_settings: true' "$CONFIG" \
  "repository config does not suppress broker identity for gate agents"
if awk '
  /^commands:/ { in_commands=1; next }
  in_commands && /^[^ ]/ { in_commands=0 }
  in_commands && /^[ ]+test:/ { print }
' "$CONFIG" | grep -Eq 'mx-test-run\.sh[[:space:]]+--all|tests/\*\.test\.sh'; then
  fail "commands.test owns a forbidden full-suite walk"
fi
pass "tracked deep-review config preserves trusted, focused validation defaults"

legacy_gate="no""-mistakes"
for retired_path in \
  ".${legacy_gate}.yaml" \
  ".github/workflows/${legacy_gate}-required.yml" \
  "tests/mx-nm-test-contract.test.sh" \
  "tests/mx-${legacy_gate}-ownership.test.sh" \
  "tests/${legacy_gate}-required-workflow.test.sh"; do
  [ ! -e "$ROOT/$retired_path" ] || fail "retired external-gate path remains: $retired_path"
done
pass "retired external-gate files are absent"

legacy_hits=$(
  git -C "$ROOT" grep -Iin -e "$legacy_gate" -e 'NO_'"MISTAKES" -- \
    bin tests docs skills .agents .github README.md CONTRIBUTING.md example_agents.md \
    ':!docs/firstmate_dependencies.md' \
    ':!tests/mx-deep-review-config-contract.test.sh' 2>/dev/null || true
)
[ -z "$legacy_hits" ] || fail "maintained surfaces still reference the retired external gate:
$legacy_hits"
pass "maintained surfaces do not reference the retired external gate"
