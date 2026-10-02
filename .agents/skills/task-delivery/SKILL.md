---
name: task-delivery
description: Validate returned artifacts, record revision-bound completion evidence and publish scoped task branches or PRs when delivering delegated work.
metadata:
  internal: true
---

# Deliver an accepted outcome

Workers execute their accepted deliverable; coordinators reconcile the result against scope and criteria, synthesize it and present it to the user.
Inspect `mx task-model inspect TASK_ID` for the current task, attempt, accepted brief, checkout and delivery commit before claiming completion.
Checks passing, implementation complete, review complete, PR ready and human merged are separate facts.
Record exact checks and observed results, limitations and original artifact pointers; an omitted or unrun check is not a pass.

Implementation completion requires current typed evidence matching the task attempt, brief and actual worktree `HEAD`, followed by a new task-bound `done` report.
Prepare the closed JSON request documented in [delivery guidance](../../../docs/delivery.md#local-completion-and-dependent-work), using actual inspected values rather than placeholders.
Submit it through:

```sh
mx task-model evidence TASK_ID --request-file ABSOLUTE_EVIDENCE_JSON_PATH
```

Repeat the same evidence ID and identical payload after an uncertain result.
Use the inspected existing delivery commit as the concurrency token, or `null` only when no current delivery commit exists.
Then use `report_status` when exposed, or the brief's absolute `bin/mx-report` fallback, bound to the assigned task and current attempt/generation/brief.
Report and coordination assignments attach their existing regular-file result with `--artifact` when reporting `done`.
A plain status message does not release dependencies.
A done report submitted before evidence needs a new message ID after evidence is recorded; replaying its old event does not reinterpret history.
Current failure, block, pause or renewed work withdraws completion; stale attempts and superseded briefs cannot establish readiness.

Workers may commit, push their task branch, open/update PRs and make ordinary scoped fixes without an extra Multplx publication approval.
Inspect `bin/mx-deliver.sh --help` for preparation and repeat-safe publication; directly opened PRs register through `bin/mx-pr-check.sh TASK_ID PR_URL`.
Reconcile uncertain forge outcomes by canonical repository, branch and base identity before another publication attempt.
Task-bound publication never uses a newly selected global project.
Local-only or remote-free delivery returns its branch and evidence without inventing a PR.
A changed commit preserves earlier checks and reviews as history and requires fresh current evidence.

Only humans merge PRs; never merge, enable auto-merge, enqueue merging, delegate merging or push the PR result directly to the remote target branch.
Deep-review and vplan remain explicit opt-in tools, and selected workflow stages remain binding.
Retain dirty, unpushed, unlanded or uncertain work until the worktree owner establishes safe disposition.
Present the accepted outcome, exact evidence, limitations and canonical PR or result artifact to the user.
[Delivery](../../../docs/delivery.md) owns publication, authentication, completion schemas and merge boundaries; [worktrees](../../../docs/worktrees.md) owns retention and cleanup.
