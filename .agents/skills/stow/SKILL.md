---
name: stow
description: Optionally persist useful session knowledge when requested or when preserving context is useful.
user-invocable: true
metadata:
  internal: true
---

# Stow

Inspect the destination before updating useful knowledge; revise duplicated or superseded material instead of accumulating contradictory notes.
Private preferences and local learnings belong in the active home's `data/maintainer.md` and `data/learnings.md`; inherited shared preferences remain parent-owned.
Task evidence and source pointers belong with `data/<id>/`; machine-owned task and decision state use their CLI owners.
For task notes, inspect `bin/mx-backlog.sh show <id>` and use `bin/mx-backlog.sh update <id> --body-file <path>` with `--archive-body` when prior context should remain recoverable.
Project facts may remain in a task artifact or be incorporated into appropriate project documentation as part of scoped project work.
A memory sweep does not require a new delivery task, project AGENTS.md edit, review gate or PR.
Do not mix private knowledge into shared runtime skills or contributor files.
Summarize what was saved and any context still outstanding without claiming everything is durable when it is not.
The public [standalone stow skill](../../../skills/stow/SKILL.md) remains independent.
