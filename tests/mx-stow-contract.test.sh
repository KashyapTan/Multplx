#!/usr/bin/env bash
# Behavior tests for /stow's inspect-then-update memory contract.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

test_stow_skill_task_note_contract() {
  local stow="$ROOT/.agents/skills/stow/SKILL.md"

  assert_grep 'bin/mx-backlog.sh show <id>' "$stow" "stow skill does not require inspecting task notes first"
  assert_grep 'bin/mx-backlog.sh update <id> --body-file <path>' "$stow" "stow skill does not require task body replacement"
  assert_grep '--archive-body' "$stow" "stow skill does not document recoverable task body archival"
  assert_grep 'Never append.' "$stow" "stow skill does not forbid append-first task notes"
  assert_no_grep 'carry that context into the replacement body' "$stow" "stow skill still preserves archive-only context in the replacement body"
  pass "stow skill task-note contract includes recoverable body archival"
}

test_agents_backlog_task_note_contract() {
  local agents="$ROOT/AGENTS-PORTING.md"

  # shellcheck disable=SC2016 # Literal backticks must remain unexpanded.
  assert_grep 'header of `bin/mx-backlog-lib.sh` owns the backlog schema' "$agents" \
    "AGENTS-PORTING.md does not point exact task-note mechanics to the command owner"
  assert_grep 'Inspect the current task note before replacing its considered body' "$agents" \
    "AGENTS-PORTING.md does not require inspecting task notes before replacement"
  assert_grep 'archive the superseded body when recoverability matters rather than appending by default' "$agents" \
    "AGENTS-PORTING.md lost recoverable replacement and no-append semantics"
  assert_no_grep 'bin/mx-backlog.sh show <id>' "$agents" \
    "AGENTS-PORTING.md duplicates exact task-note read syntax from its conditional owner"
  assert_no_grep 'bin/mx-backlog.sh update <id> --body-file <path>' "$agents" \
    "AGENTS-PORTING.md duplicates exact task-note update syntax from its conditional owner"
  pass "AGENTS-PORTING.md keeps task-note hygiene inline and points exact mechanics to their owner"
}

test_stow_skill_task_note_contract
test_agents_backlog_task_note_contract
