---
name: subagent-recovery
description: Operational reference for startup diagnostics, missing endpoints and recovery of recorded sub-agent work.
user-invocable: false
metadata:
  internal: true
---

# Sub-agent recovery

Read current task state with `bin/mx-actor-state.sh <id>` and inspect only the recorded endpoint, home and worktree.
Use `bin/mx-doctor.sh --help` for focused diagnostics, including `stateless-sessions`.
A dead endpoint does not prove that its task, independently running work or recorded optional-tool run is gone.
Before replacement, reconcile the old process and mutable ownership; preserve the existing task, worktree, commits and artifacts.
Use [harness-adapters](../harness-adapters/SKILL.md) for the recorded harness's send, interrupt and resume controls.
Uncertain ownership remains retained and visible rather than triggering a second generic spawn.
Persistent homes use [persistent-subagents](../persistent-subagents/SKILL.md).

## Diagnostic owners

`bin/mx-bootstrap.sh --help` and [configuration](../../../docs/configuration.md) own current diagnostic formats.
Repair only the affected dependency or configuration; report unavailable capabilities honestly.

| Diagnostic family | Existing owner or inspection |
| --- | --- |
| MISSING / MISSING_MANUAL | Printed dependency information and `bin/mx-bootstrap.sh --help`; install only within the requested task scope. |
| BACKEND_INVALID / ACTOR_DISPATCH / HEADROOM_INVALID | Validated backend, dispatch and capacity configuration in command help. |
| Active vplan asset failure | Repair or stop only the explicitly requested vplan run; unrelated work continues. |
| TANGLE / SYSTEM_SYNC | Recorded Git state and `bin/mx-system-sync.sh --help`; retain dirty, diverged or unlanded work. |
| PR_CHECK_MIGRATION | Named migration outcome and `bin/mx-pr-check.sh --help`; quarantined polls stay unexecuted. |
| DAEMON_SYNC / DAEMON_LIVENESS / NUDGE_DAEMONS | Recorded home, pending instruction delivery and persistent-home operations. |

Use the emitted home-scoped supervision repair path; never broadly kill watchers or sweep another home's endpoints.
[A10](../../../porting.md#a10-built-in-git-worktree-lifecycle) assigns allocation inspection and retention to Phase 03's built-in manager; this reference does not invent its pending CLI grammar.
