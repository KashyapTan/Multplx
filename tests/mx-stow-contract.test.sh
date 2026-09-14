#!/usr/bin/env bash
# Optional memory persistence keeps inspect-before-update and archive mechanics.
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
STOW="$ROOT/.agents/skills/stow/SKILL.md"
assert_grep 'Inspect the destination before updating' "$STOW" 'memory overwrite lacks inspection'
assert_grep 'bin/mx-backlog.sh show <id>' "$STOW" 'task-note inspection missing'
assert_grep 'bin/mx-backlog.sh update <id> --body-file <path>' "$STOW" 'owned task-note mutation missing'
assert_grep '--archive-body' "$STOW" 'recoverable body replacement missing'
assert_grep 'does not require a new delivery task' "$STOW" 'forced memory delivery retained'
assert_grep 'machine-owned task and decision state use their CLI owners' "$STOW" 'memory bypasses machine state'
assert_present "$ROOT/skills/stow/SKILL.md" 'standalone public stow removed'
pass 'optional stow retains useful persistence mechanics without delivery ceremony'
