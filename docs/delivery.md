# Agent delivery and human PR merges

Default to standing managed workers and coordinators; select temporary/task-scoped lifecycle only explicitly.
While the accepted job is active, supervise normally.
Once the accepted task and full job are finished, stop contacting, polling, nudging or automatically routing additional work to that agent.
The agent remains available for the user to return to and guide; explicit user-directed follow-up is allowed.
Task completion and agent availability are separate; do not invent ongoing work, recurring supervision, automatic reuse by responsibility or automatic retirement.
Full completion means the parent has validated the full job against the agreed scope, resolved gaps and delivered it, not merely received a worker done claim.
Before that point, the parent may guide, correct mistakes, request revisions, re-engage a worker that reported done prematurely and finish missing work through the assigned workers.
Preserve task and accepted-revision evidence when scope changes.

Implementers may commit, push their assigned task branch, open or update its PR, and make ordinary follow-up fixes within the accepted scope.
Publication requires no Multplx approval, review gate, waiver or separate credentialed shell.
Humans perform PR merges.
Implementation complete, checks passing, review complete, PR ready and human merged are separate facts.

[Back to the documentation index](README.md).

## Publish and register

Work in the assigned isolated worktree and use its recorded project, starting revision and task branch.
Verify the actual commit and report the exact checks and results, limitations and original evidence pointers.
The convenience entry `bin/mx-deliver.sh` publishes an explicit task revision and reconciles its canonical PR on retry.
Its `--help` owns the current arguments and receipt locations.
Check summaries use an explicit `passed:`, `failed:`, `not-run:` or `unknown:` prefix; omission never implies passing checks.
For a clean task commit, prepare and publish with actual evidence:

```sh
bin/mx-deliver.sh prepare task --sha FULL_SHA --summary 'Describe the change' \
  --checks 'passed:Exact commands and results' --limitations 'Remaining limits, or none'
bin/mx-deliver.sh task
```

Ordinary Git and official GitHub CLI commands remain usable for branch pushes and PR creation or updates.
Register a directly opened PR through the same canonical owner:

```sh
bin/mx-pr-check.sh task https://github.com/OWNER/REPO/pull/NUMBER
```

Registration keeps the validated PR identity, read-only merge poll and later cleanup connected to the task.
Publication uses the task-bound repository, not a globally selected project.
A remote-free or local-only task returns its local branch and evidence without inventing a forge destination.
Borrowed source checkouts retain their branches, index and files; publication takes place in the assigned isolated worktree.

## Receipts and uncertain outcomes

Publication records the branch, base, exact commit, task attempt, accepted brief revision and canonical PR identity.
A command timeout or missing response does not prove that a push or PR creation failed remotely.
Retry the same publication request so the owner can reconcile the existing branch and PR before attempting another external action.
A push followed by PR-creation failure retains the work and receipt for continuation.
A PR created before local recording is recovered by its exact repository, branch and base identity.
Ambiguous or conflicting forge results remain unresolved instead of producing duplicate PRs.

The filesystem owners use private inert records and atomic publication; records are never sourced as shell code.
Canonical PR polling retains static-poll trust, identity checks and queue-before-retirement ordering.
Historical service handoffs and delivery receipts remain readable for migration.
An old pending handoff does not become a new authorized push simply because the software was upgraded.
New ordinary publication is explicit; it does not fabricate legacy approval.

## Evidence and human review

Accepted scope, attempt and commit identify which evidence is current.
A new commit or scope revision preserves earlier checks and reviews as history; they cannot establish readiness for the new revision.
Record actual results and limitations, including unrun checks, without inferring success from a role, a published branch or an open PR.
Deep-review is separate optional evidence and does not gate ordinary publication.
If explicitly selected, its actual failure remains a failure; publication does not turn it into a waiver or pass.

The canonical task owner exposes delivery evidence and a dependency-aware human review queue through `mx task-model review-queue`.
Priorities and unresolved dependencies help order human attention without creating another publication approval step.
Durable outcomes carry publication, failure, evidence-change and human-merge facts through the parent chain even while a coordinator model is idle.
Coordinator summaries may explain the facts but do not replace them or make stale evidence current.

A standing worker keeps its runtime home separate from the parent task owner.
For task-model inspection/evidence, prefix the command with `MX_STATE_OVERRIDE=ABSOLUTE_PARENT_STATE` from the brief; `MX_REPORT_STATE_OVERRIDE` routes reports but does not route task-model commands.
Preserve current task, attempt and accepted revision identity when submitting evidence.

## Local completion and dependent work

A status report is separate from canonical implementation completion.
For an implementation assignment, record current typed delivery evidence and then report `done` through the task-bound reporter.
The completion check requires that evidence to match the current attempt, accepted brief and actual worktree `HEAD`.
It does not turn failed or unrun checks into passing checks, create a PR, or record a human merge.
A report or coordination assignment can instead attach its existing result file through the reporter's structured `--artifact` option.
A plain `done` message remains valid status evidence, but reports a diagnostic and does not release dependent work without the required result evidence.
After completion, a current `failed`, `blocked` or `paused` report withdraws completion and records an external wait; a keyed `needs-decision` report records a human wait.
Other current activity reports, or a new `done` report that cannot prove completion, reopen the task instead of preserving an old completed state.
Dependent work remains queued until a fresh valid completion report; unrelated tasks can continue.
A current `working` or `resolved` report clears a failure, block or pause wait created by a prior report without restoring completion; human and lifecycle-owned waits retain their own resolution paths.
Rejected stale-attempt or stale-brief reports and retries of an already committed report do not change the current completion state.

From the assigned worker's activated environment, inspect the task and submit a JSON evidence file:

```sh
mx task-model inspect TASK_ID
mx task-model evidence TASK_ID --request-file /absolute/path/evidence.json
```

Use the actual task attempt, brief revision, current full commit SHA and observed check results in the closed request format below.
The placeholder strings are not literal values to submit.
Use `null` for `expected_current_commit` only when the inspected task has no current delivery commit; otherwise use that existing commit as the concurrency token.
Keep the same evidence ID and identical JSON when retrying an uncertain submission.

```json
{
  "evidence_id": "task-result-1",
  "attempt_id": "ATTEMPT_ID_FROM_INSPECT",
  "attempt_generation": 1,
  "brief_revision": 1,
  "commit": "FULL_CURRENT_COMMIT_SHA",
  "checks": [{
    "name": "EXACT_CHECK_COMMAND",
    "outcome": "passed",
    "summary": "ACTUAL_RESULT",
    "artifact": null
  }],
  "review": null,
  "limitations": [],
  "pr_url": null,
  "outcome": "evidence-updated",
  "observed_at": "ACTUAL_OBSERVATION_TIMESTAMP",
  "mark_current": true,
  "expected_current_commit": null
}
```

After evidence is accepted, send a new task-bound `done` report using the installed status tool or `mx-report` command.
For a marked `[mx-from-parent]` request, set the exact token in `report_status`'s `correlation_id` field or `mx-report --correlation-id TOKEN`; putting `corr=TOKEN` in message prose does not bind the reply.
Set `artifact` in the tool or `--artifact PATH` in the CLI for an existing report or coordination result.
A CLI report returns a JSON acceptance receipt; the tool returns that receipt as structured content.
Check `correlation_id`, `outstanding_requests`, and `completion_proven`; unrelated outstanding requests remain pending and require separate responses.
Use `message_id` or `--message-id` with identical payload for uncertain retries.
A receipt with `replayed: true` reports historical acceptance without reapplying completion; `accepted_completion_proven` describes the original event and `completion_proven` describes current completion.

A report sent before the evidence existed needs a new message ID after evidence is recorded; replaying an already committed status event does not reinterpret its historical meaning.
Scope changes, replacement attempts, renewed working reports and a changed current delivery commit reopen completion until fresh evidence is reported.

## Authentication and merge boundary

Ordinary workers inherit user-configured Git credential helpers, GitHub CLI configuration, token precedence and SSH settings.
Multplx does not overlay a blocked origin push URL or strip ordinary authentication from worker launch.
Use repository-scoped credentials where appropriate, and keep secrets out of task records, briefs, tracked files and logs.
The convenience publisher accepts the optional `MX_DELIVERY_GH_TOKEN` or absolute `MX_DELIVERY_GH_CONFIG_DIR` override; configure at most one.
Without an explicit override it uses ordinary authentication.
The old `MX_AGENT_GH_TOKEN` launch override is retired; configure the supported Git/forge environment directly.

Agents must not merge PRs, enable auto-merge, enqueue merges, ask another agent or automation to merge, or push the PR result directly to the remote target branch.
Historical yolo settings, standing authority and merge-red overrides grant no merge authority.
Local Git merge and rebase inside task worktrees remain normal work.
The human-shell helper is:

```sh
bin/mx-pr-merge.sh task https://github.com/OWNER/REPO/pull/NUMBER
```

Owned commands refuse known agent sessions and supported harness hooks reject ordinary remote merge commands.
These are operational backstops, not a security sandbox against arbitrary code with broad credentials.
Ordinary forge write credentials do not inherently separate PR creation from merging.
Where independent enforcement is required, configure and verify remote branch protection and identity controls.
No additional credential broker is required for ordinary publication.

## Work retention and local outcomes

A branch push, open PR, successful check or stopped agent is not evidence that work has landed.
Open PRs, unpushed commits, dirty files, unknown ignored content, uncertain occupants and conflicting ownership retain the worktree.
After a human merge, the poll reports the observed fact; cleanup still verifies the work disposition and exact allocation generation.
The built-in worktree owner refuses stale tokens and never treats a borrowed source checkout as disposable.
For a user-owned source checkout, local delivery retains the task branch for human integration; `mx-merge-local.sh` refuses to fast-forward that source checkout.
See [worktree ownership](worktrees.md) for release, retention and explicit prune behavior.

## Choose and complete a delivery mode

The canonical project and task records separate remote publication from local-only outcomes.
Legacy mode values remain compatibility input; they cannot require approval for ordinary branch publication or grant merge authority.
A clearly selected workflow retains every declared output and explicit interaction point.
Optional review tools are explicit and revision-bound, while Phase 08 owns the broader workflow redesign; recorded workflow stages remain binding.
Use command help for the exact publication grammar and the task owner for current revision evidence.

## Review execution bounds

`bin/mx-deep-review.sh --help` owns the supported round, attempt and wall-clock overrides for an explicitly requested run.
The run records optional review evidence against the current task attempt, accepted brief, project allocation and commit.
It does not create a publication approval or make ordinary delivery depend on a review result.
Each configured command defaults to 300 seconds and each headless invocation to 1800 seconds; positive `MX_DEEP_REVIEW_COMMAND_TIMEOUT_SECONDS` and `MX_DEEP_REVIEW_AGENT_TIMEOUT_SECONDS` override them independently.
Round and structured-output attempt limits remain separate count bounds, not a global task deadline.
Subsequent prompts include only owned structured findings and decision history, capped at 262144 bytes; excess fails closed with a pointer to the retained evidence.
Codex transport events are retained in private `.events.jsonl` files even when the invocation fails; raw transport events are never copied into round history.
Other headless adapters do not provide the same retained transport-log guarantee.
A timeout fails the invocation, reports its deadline, and kills its process group.
Headless invocations may retry within the configured attempt limit; exhausting that limit fails the gate, while a configured-command timeout stops the run.
No timeout counts as successful validation or grants an automatic waiver.
The process-group boundary covers ordinary descendants; subprocesses that deliberately detach into another session are outside this cleanup guarantee.
The default configuration keeps broad regression in CI and uses focused local verification.
