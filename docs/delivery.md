# Agent delivery and human PR merges

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
Phase 07 owns optional-tool defaults and Phase 08 owns the workflow redesign; this delivery change does not silently rewrite recorded workflows.
Use command help for the exact publication grammar and the task owner for current revision evidence.

## Review execution bounds

`bin/mx-deep-review.sh --help` owns the supported round, attempt, and wall-clock overrides.
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
