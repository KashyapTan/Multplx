---
name: persistent-subagents
description: Operational reference for persistent homes, inherited configuration, parent routing and safe retirement.
user-invocable: false
metadata:
  internal: true
---

# Persistent sub-agent operations

Persistence is independent of assignment role.
A sub-orchestrator owns a bounded charter, delegates project coding and test changes, and reports through its recorded parent channel.
[A11](../../../porting.md#a11-scoped-sub-orchestrators) owns the accepted domain contract; Phase 05 publishes its named spawn and report grammar.
The common lifecycle supports persistent assignments; named coordinator provisioning and outcome relays remain Phase 05 work.

| Operation | Existing command reference |
| --- | --- |
| Charter scaffold | `mx brief <id> --persistent <project>...` or `--no-projects`; legacy `--daemon`, `MX_DAEMON_CHARTER` and `MX_DAEMON_SCOPE` remain supported. |
| Provision / validate | `bin/mx-home-seed.sh --help`; validate recorded homes before launch. |
| Launch / recover | `mx spawn <id> --persistent`; reuse the recorded home and reconcile existing children; `--daemon` remains an alias. |
| Inherited settings | `bin/mx-config-push.sh --help`; owned propagation preserves generations and pending reread delivery. |
| Queued-work handoff | `bin/mx-backlog-handoff.sh --help`; move through the owner rather than copying two authoritative backlogs. |
| Retirement | `bin/mx-teardown.sh --help`; retain unfinished work and uncertain ownership. |

[Configuration](../../../docs/configuration.md) and the [inheritance implementation](../../../crates/multplx-domain/src/inheritance.rs) own the legacy registry, configuration allowlist and propagation schema.
The [sub-agent record contract](../../../docs/subagent-model.md) distinguishes persistence, assignment, accepted brief and attempt identity.
The seeded `data/charter.md` owns the assignment text; `.mx-daemon-home` binds the home identity.
Home validation rejects duplicate, nested or overlapping registered homes.
Provisioning rolls back only artifacts it created; a persistent reservation survives zero live processes and ordinary restarts.
[A10](../../../porting.md#a10-built-in-git-worktree-lifecycle) replaces the legacy allocation dependency in Phase 03.

Keep parent task/report binding separate from the child's own operational home.
Preserve correlation tokens on replies and original artifact pointers on detailed outcomes.
Each home reconciles its own children; a parent does not drain a child's queue or hand-edit its records.
Inherited shared maintainer preferences are parent-authoritative and read-only in children; never copy a child's version back over the parent.
Guarded tracked-file updates and inherited-local-material propagation are separate operations; neither makes a delivered reread pointer proof of model acknowledgement.
An empty queue is healthy for a persistent assignment and does not initiate work or retirement.
Retirement must account for children, outstanding replies and retained work; a force option is not permission to discard them.
