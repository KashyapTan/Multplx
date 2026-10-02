---
name: persistent-subagents
description: Provision, recover, configure or retire persistent homes and scoped coordinators when a standing assignment or parent route is needed.
user-invocable: false
metadata:
  internal: true
---

# Persistent sub-agent operations

Persistence is independent of assignment role.
All delegation, including nested delegation, uses Multplx-managed agents by default.
Native delegation requires an explicit human request for that scope and is never an automatic fallback when managed spawning fails.
Standing agents remain available to the user after verified full-job delivery; do not automatically reuse, supervise or retire them.
Follow the completion and corrective-follow-up boundary in [AGENTS.md](../../../AGENTS.md#9-persistent-coordinators-and-parent-reporting).
The dispatch policy selects `--persistent`; low-level CLI omission retains task-scoped compatibility.
For workers, scaffold with explicit `--role` and `--output`, fill the brief, seed through `mx home-seed`, and launch with those same values; home seeding preserves the worker charter rather than changing its role.
Cmux persistent launch and task delivery mode/yolo overrides on persistent spawn remain unsupported.
A sub-orchestrator owns a bounded charter, delegates requested research, planning deliverables, implementation, testing and reviews, and reports through its recorded parent channel.
[A11](../../../porting.md#a11-scoped-sub-orchestrators) owns the accepted domain contract.
The [scoped coordinator reference](../../../docs/scoped-coordinators.md) owns named provisioning, parent outcomes and the verification boundary.

| Operation | Existing command reference |
| --- | --- |
| Scoped coordinator | `mx spawn <id> --sub-orchestrator --project <selector> --scope <text>`; repeat `--project`, or use `--idea <id>` for repository-free research. Select `--persistent` by default for a standing domain; omit it only for an explicitly temporary domain. Use `--request-id <id>` for repeat-safe creation. |
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
Interrupted provisioning restores recorded parent artifacts and retains the reserved home; a persistent reservation survives zero live processes and ordinary restarts.
[Worktree operations](../../../docs/worktrees.md) describe built-in allocation and private home reservations.
Home seed remembers project references without copying repositories; retirement retains private home material in an explicit archive.

Keep parent task/report binding separate from the child's own operational home.
Preserve correlation tokens on replies and original artifact pointers on detailed outcomes.
Each home reconciles its own children; a parent does not drain a child's queue or hand-edit its records.
Inherited shared maintainer preferences are parent-authoritative and read-only in children; never copy a child's version back over the parent.
Guarded tracked-file updates and inherited-local-material propagation are separate operations; neither makes a delivered reread pointer proof of model acknowledgement.
An empty queue is healthy for a persistent assignment and does not initiate work or retirement.
Retirement must account for children, outstanding replies and retained work; a force option is not permission to discard them.

Persistent implementation spawn takes `--project PROJECT --base COMMIT`, where the project is already referenced in the seeded home and COMMIT is the exact full accepted starting commit.
The spawn owner binds the project and acquires a separate persistent project worktree; the private home and its availability remain independent of implementation completion.
