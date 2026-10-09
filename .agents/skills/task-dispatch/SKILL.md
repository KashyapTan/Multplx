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
Standing managed workers are the dispatch default, independently of assignment role or output.
The low-level CLI remains task-scoped when `--persistent` is omitted for existing callers; dispatch must select it explicitly.
For a standing project worker, scaffold and fill the accepted brief, then seed its home and launch with the same role/output:

```sh
mx brief TASK_ID PROJECT_PATH --persistent --role researcher --output report --context-file ABSOLUTE_HANDOFF_PATH
# Fill every {TASK} placeholder before provisioning.
mx home-seed TASK_ID - PROJECT_PATH
mx spawn TASK_ID --persistent --role researcher --output report --request-id STABLE_REQUEST_ID
```

Select `implementer` with `implementation` for project changes, or `reviewer` with `report` for review.
For a standing implementer, use those values in brief/spawn and add `--project PROJECT_PATH --base FULL_ACCEPTED_COMMIT` to spawn; it acquires the exact project worktree before launch.
The handoff file preserves original context verbatim; it does not override validated launch identities.
Role and output are independent metadata; a requested planning artifact can be a researcher report.
Honor explicit user model and effort choices using supported `--model` and `--effort` values, and resolve configured dispatch choices as described in [configuration](../../../docs/configuration.md).
Reconcile an uncertain dispatch by repeating its stable identity rather than launching a duplicate.
Acceptance, capacity admission, endpoint launch and actual execution are separate facts.

Inspect an existing task with `mx task-model inspect TASK_ID` before revising or replacing it.
Use `mx task-model revise --help` to record changed scope, role, criteria and artifact pointers against its current revision, then send the accepted revision to its recorded endpoint through `mx send --help`.
Include the exact binding instructions printed by `revise` in the parent change message and require the worker to inspect/read the changed brief before accepting it.
After accepting within the same attempt, the worker explicitly applies `export MX_BRIEF_REVISION=N` for subsequent shell reports and nested spawns.
Existing MCP processes keep their original environment; use the exact `attempt_id`, `generation` and `brief_revision` together on later `report_status` calls.
Do not infer that a delivered pointer proves the worker read it.
For a pre-execution recovery observation, use the parent-authored `mx task-model outcome --help` path with current identity and retained evidence, rather than fabricating a worker `MX_TASK_ID`.
That operation supports only failed, blocked and paused observations; it neither completes work nor stops an endpoint.
Replacement uses the spawn owner's current-attempt checks after existing mutable ownership is reconciled.
Preserve authority routes when working with transferred tasks; do not copy canonical records into another home.

For a bounded coordinator domain, use [persistent-subagents](../persistent-subagents/SKILL.md) and [scoped coordinators](../../../docs/scoped-coordinators.md); selecting a project alone does not create one.
All delegation, including nested delegation, uses Multplx-managed agents by default.
Native delegation requires an explicit human request for that scope and is never an automatic fallback when managed spawning fails.
Use the standing lifecycle and verified-completion contact boundary in [AGENTS.md](../../../AGENTS.md#9-persistent-coordinators-and-parent-reporting); completion does not authorize automatic reuse.
For an explicitly temporary worker, omit `--persistent` from brief/spawn and pass its project to spawn without home seeding.
Persistent launch does not accept task delivery mode/yolo overrides, and cmux persistent-home launch remains unsupported.
Report the concrete unsupported combination or failed managed launch; do not change transport or lifecycle silently.
[Task records](../../../docs/subagent-model.md) own identity and revision details, and [durable coordination](../../../docs/durable-coordination.md) owns retry and message receipt schemas.

Persistent implementation spawn takes `--project PROJECT --base COMMIT`, where the project is already referenced in the seeded home and COMMIT is the exact full accepted starting commit.
The spawn owner binds the project and acquires a separate persistent project worktree; the private home and its availability remain independent of implementation completion.
