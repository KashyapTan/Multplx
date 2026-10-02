---
name: task-dispatch
description: Prepare task briefs, launch scoped assignments and communicate accepted revisions when delegating research, planning, implementation, testing or review.
metadata:
  internal: true
---

# Dispatch an assignment

Coordinators delegate requested substantive deliverables, including small tasks; workers execute their accepted assignment and may delegate bounded parts when useful.
Keep intake, discussion, synthesis and coordination planning with the coordinator.
Use narrow inspection to route work without performing the requested investigation yourself.
A well-specified fix needs only an implementer, not a compulsory research or review chain.

Resolve an explicit project and checkout using `mx project --help` and the [project-management skill](../project-management/SKILL.md).
Keep repository instructions and original artifacts with this assignment; changing selected context does not retarget existing tasks.
Use `mx brief --help` and `mx spawn --help` for the installed grammar and compatibility limits.
The brief owner writes `data/TASK_ID/brief.md`; fill the outcome, scope, acceptance criteria, constraints, sources and real dependencies before launch.
For an ordinary project assignment, the inspected command shape is:

```sh
mx brief TASK_ID PROJECT_PATH --role researcher --output report --context-file ABSOLUTE_HANDOFF_PATH
mx spawn TASK_ID PROJECT_PATH --role researcher --output report --request-id STABLE_REQUEST_ID
```

Select `implementer` with `implementation` for project changes, or `reviewer` with `report` for review.
The handoff file preserves original context verbatim; it does not override validated launch identities.
Role and output are independent metadata; a requested planning artifact can be a researcher report.
Honor explicit user model and effort choices using supported `--model` and `--effort` values, and resolve configured dispatch choices as described in [configuration](../../../docs/configuration.md).
Reconcile an uncertain dispatch by repeating its stable identity rather than launching a duplicate.
Acceptance, capacity admission, endpoint launch and actual execution are separate facts.

Inspect an existing task with `mx task-model inspect TASK_ID` before revising or replacing it.
Use `mx task-model revise --help` to record changed scope, role, criteria and artifact pointers against its current revision, then send the accepted revision to its recorded endpoint through `mx send --help`.
Do not infer that a delivered pointer proves the worker read it.
Replacement uses the spawn owner's current-attempt checks after existing mutable ownership is reconciled.
Preserve authority routes when working with transferred tasks; do not copy canonical records into another home.

For a bounded coordinator domain, use [persistent-subagents](../persistent-subagents/SKILL.md) and [scoped coordinators](../../../docs/scoped-coordinators.md); selecting a project alone does not create one.
Native delegation remains available for bounded work when the provider exposes it; durable cross-project assignments retain Multplx task identity and routing.
[Task records](../../../docs/subagent-model.md) own identity and revision details, and [durable coordination](../../../docs/durable-coordination.md) owns retry and message receipt schemas.
