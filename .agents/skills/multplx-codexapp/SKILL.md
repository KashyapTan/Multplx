---
name: multplx-codexapp
description: Optional reference for Codex Desktop host-tool coordination and honest transport limits.
user-invocable: false
metadata:
  internal: true
---

# Codex Desktop integration

[Codex App backend documentation](../../../docs/codex-app-backend.md) owns current limitations and [runtime evidence](../../../docs/verification/runtime-backends.md) records verified behavior.
Desktop host tools are an optional integration, not a shell-callable `codex-app` backend.
Use available host tools to list saved projects, create explicitly requested tasks, inspect progress, send follow-ups and archive.
Use the host's actual schemas; do not simulate those calls with shell files or invent provider identities.
For implementation, preserve the returned isolated working directory instead of directing the child to edit the saved checkout.

Bind any coordinated task to its recorded provider task id, project, cwd and report route.
Use `report_status` or the validated `bin/mx-report` interface, never raw status-file appends.
Verify that a report reaches the intended task owner before describing the integration as coordinated.
If tools or a report route are unavailable, record that limitation and treat the task as a companion with only observed visibility.
Archive through the available host tool; preserve unfinished work and distinguish archive from task completion or worktree release.
