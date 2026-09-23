# Multplx user guide

Multplx gives one main orchestrator a durable way to coordinate work across several repositories and independent sub-agents.
You can open the workspace from any directory, submit work in chat or from the command line, and leave long-running work attached to its original project and revision.

This guide starts after installation.
Use [Getting started](getting-started.md) for package installation, source installation, and supported tool requirements.
Use `multplx --help` and each command's `--help` output for the exact grammar in your installed version.

## Release status

The lean redesign described here does not yet have a published public release.
These commands describe the current release candidate and should not be assumed to exist in an older installed `multplx` command.
Until release cutover, [build and install the current candidate](getting-started.md#build-the-current-candidate-from-source) in separate directories or use an isolated candidate package supplied for validation.
The development checkout deliberately keeps its root operating contract dormant, so do not use that checkout for normal orchestration startup.
Use an isolated candidate package with the release contract restored when validating the complete installed experience.
After a public release is published, prefer its verified platform package and matching checksum.

Candidate acceptance is provider-specific.
The current Codex package passed four five-task live trials covering flat and hierarchical delegation, independent repositories and coupled dependencies, with all 20 worker checks and integrated results passing.
The validation record also documents runtime repairs and observer assistance; these results do not establish unattended operation or live 10/20-task scalability.
Cursor evidence is limited to its pinned version, authentication, and adapter checks, while Claude, Pi, and cmux still lack current candidate-package live acceptance.
Treat deterministic adapter tests and older live evidence separately from a successful run of this candidate; the candidate validation record carries the exact evolving limits.

## The working model

One Multplx home owns one main orchestrator conversation.
That conversation can coordinate direct sub-agents and optional scoped sub-orchestrators across many repositories.
Each accepted task keeps its project, checkout, starting commit, scope, dependencies, and evidence even when you switch the project shown in the terminal.

Multplx remembers existing Git checkouts without taking ownership of their contents.
Implementation runs in isolated managed worktrees created from committed revisions.
Uncommitted files in your checkout stay where they are and are not included in a task unless you commit them and select that commit.

A task receipt means the request was durably accepted.
It does not mean an agent has started or completed the work.
The workspace, status surfaces, and final outcome show those later transitions separately.

## Open the workspace and conversation

Run Multplx from any directory:

```sh
multplx
```

The terminal workspace shows known projects, tasks, decisions, scoped domains, and the connection to the main conversation.
The directory where you launched Multplx supplies context but is never scanned or registered automatically.

The main workspace keys are:

| Key | Action |
| --- | --- |
| `Tab` | Move between projects, tasks, decisions, and domains. |
| Arrow keys or `j` and `k` | Move through the current list. |
| `/` | Search projects. |
| `t` | Enter a task for the selected project. |
| `r` | Refresh discovery and task state. |
| `c` | Enter the main chat. |
| `v` | Open MX Viz. |
| `q` | Close the workspace while independent work continues. |

Use a plain snapshot when you do not want the interactive interface:

```sh
multplx workspace --plain
```

Choose a supported harness explicitly the first time:

```sh
multplx chat codex
# Alternatives: claude, cursor, or pi
```

Multplx remembers that choice, so a later `multplx chat` reconnects or starts the remembered harness.
If another terminal cannot attach to the provider's live conversation, Multplx preserves its route and keeps durable task submission available.

Use these commands to confirm which installation and operational home are active:

```sh
multplx --version
multplx paths
```

## Remember a project

Register an existing Git checkout and give it a convenient alias:

```sh
multplx projects register ~/dev/my-app --alias my-app
multplx projects list
multplx projects resolve my-app
```

Registration does not fetch, switch branches, modify the index, change remotes, or move files.
An alias, stable project or checkout ID, or exact registered path can select the project.
Ambiguous names return candidates instead of guessing.

Open the workspace with that project selected:

```sh
multplx my-app
```

This changes the context for the next request and does not move existing tasks to another project.
For a moved checkout, repair its stored location only when it is the same repository identity:

```sh
multplx projects repair CHECKOUT_ID /new/path/to/my-app
```

Use `multplx projects forget CHECKOUT_ID` to remove a registry entry.
Forgetting a checkout never deletes it.

## Discover nested repositories

Discovery roots are optional and independent of your current directory.
Configure a bounded root when you keep many repositories under one folder:

```sh
multplx projects roots add ~/dev --depth 6
multplx projects roots list
multplx projects discover --refresh
```

Discovery results stay candidates until you select or register one.
Discovery does not execute repository content or load repository instructions.
It reports unversioned folders and missing roots with their limits instead of treating them as task-ready Git projects.

You can replace the excluded directory names and choose whether to follow symlinks:

```sh
multplx projects configure --no-follow-symlinks \
  --exclude node_modules --exclude target
```

Use `multplx projects roots remove PATH` to stop scanning a root.

## Request work

The most natural entry is the main chat.
Name the repository for each independent item when one request spans several projects:

```text
Fix login in my-app, investigate the flaky test in repo-two, and research feature C in repo-three.
```

The orchestrator can clarify one ambiguous item while other independent work proceeds.
It creates separate task identities rather than treating the active project as a global target.

For scriptable or restart-safe intake, submit directly:

```sh
multplx task --project my-app --request-id login-1 "Fix login"
```

Keep the printed request ID.
After an uncertain timeout, repeat the same ID and identical input to converge on the original receipt:

```sh
multplx task --project my-app --request-id login-1 "Fix login"
```

Changed input under an accepted request ID is rejected so a retry cannot silently become different work.

Use one batch ID to correlate independent tasks and dependencies to hold work until prerequisite tasks complete:

```sh
multplx task --project api --request-id api-change --batch-id release-1 \
  "Add the accepted API field"
multplx task --project web --request-id web-change --batch-id release-1 \
  --depends api-change "Use the new API field"
```

By default the exact current `HEAD` commit becomes the task's starting revision.
Use `--start REVISION` to choose another named clean commit:

```sh
multplx task --project my-app --start main "Investigate the release behavior"
```

Working-tree changes are deliberately excluded from intake.
Commit the intended changes first or discuss their disposition with the orchestrator.

## Follow progress and answer decisions

The workspace combines current task state, project identity, open decisions, domain ownership, and observation age.
Refresh it with `r`, or run the plain view for a terminal-friendly snapshot:

```sh
multplx workspace --plain
```

Open the main chat to answer a product question or change a task's scope.
Multplx records accepted brief revisions so an answer to an older question cannot settle a newer revision by accident.
When changing direction, identify the task and the desired scope rather than relying on whichever project is currently selected.

The task state distinguishes accepted intake, active execution, blocked or decision-needed work, delivery evidence, and completion.
A missing live provider transcript does not erase durable task, request, worktree, or outcome records.

## Use a scoped sub-orchestrator

A scoped sub-orchestrator is useful when one project, several related repositories, or a research idea needs ongoing coordination beneath the main orchestrator.
Project selection alone never creates one.

The named `spawn --sub-orchestrator` form creates its charter directly from `--scope`.
Use a fresh coordinator ID and include its bounded assignment in that scope.
Ordinary worker assignments use the separate `brief` scaffolding flow before `spawn`.
Finish the task text before spawning, even when admission will queue it; a queued request freezes the accepted brief.
Editing that brief afterward does not update the queued assignment and correctly prevents launch against changed instructions.

Create a project-backed coordinator explicitly:

```sh
multplx spawn api-design --sub-orchestrator --project customer-api \
  --scope "Coordinate the accepted API compatibility work" \
  --request-id api-design-initial --harness codex
```

Create a persistent research domain before it has a repository:

```sh
multplx spawn onboarding --sub-orchestrator --idea onboarding \
  --scope "Research onboarding alternatives and report open questions" \
  --persistent --request-id onboarding-research --harness codex
```

`--persistent` makes the assignment available between tasks.
It does not grant broader authority or imply that its private home owns a repository.

Inspect a domain and route a new request to it:

```sh
multplx domain inspect api-design
multplx task --project customer-api --domain api-design \
  --request-id api-followup "Investigate the compatibility failure"
```

Domain routing succeeds only when the selected project is already in that domain.
Direct tasks and domain-routed tasks can coexist in the same main conversation.
See [Scoped sub-orchestrators](scoped-coordinators.md) for domain revisions, project binding, task transfer, checkpointing, and durable parent-channel operations.

## Run a workflow

Workflows preserve an explicit sequence of orchestrator, sub-agent, command, review, delivery, and human-interaction stages.
The selected definition is copied into an immutable run snapshot, so editing the source definition does not rewrite a running workflow.
In an installed workspace, ask the orchestrator to run a named workflow for the selected project.
For direct operator use, enter the activated shell and change to the selected runtime root:

```sh
multplx shell
cd "$MX_ROOT_OVERRIDE"
```

The same commands run directly from a source-install root.

Validate and preview a definition before running it:

```sh
bin/mx-workflow.sh validate workflows/new-feature.workflow.md
bin/mx-workflow.sh dry-run workflows/new-feature.workflow.md \
  --input "Add the accepted export format"
```

Start and inspect a project-bound run:

```sh
bin/mx-workflow.sh run workflows/new-feature.workflow.md \
  --project my-app --input "Add the accepted export format"
bin/mx-workflow.sh status RUN_ID
```

Use `resume` after the recorded wait condition is satisfied.
Stages are not silently skipped or reordered.
Those changes require an explicit versioned override, and dependency runs release only after their prerequisites complete.
See [Workflow definitions and runs](workflows.md) for the definition schema and gate behavior.

Deep-review and vplan remain optional tools.
They run only when you request them directly or select a workflow that includes them.

## Review delivery

An implementation outcome should identify its exact commit, checks and results, limitations, and any PR.
Multplx treats implementation complete, checks passing, review complete, PR ready, and merged as separate facts.

Implementers can commit, push their task branches, open or update PRs, and make ordinary follow-up fixes within the accepted scope.
The human performs the merge.
An open PR or successful push is never displayed as merged work.

Dependent work waits for completion evidence bound to the current task attempt and accepted brief.
For implementation work, the agent records typed evidence for the exact current worktree commit before reporting done.
A plain done message remains a status report and does not by itself release dependencies.
Research and coordination outcomes can use their recorded result artifact.
Changing the accepted scope or replacing the attempt requires fresh completion evidence.

Tasks without a forge remote can finish with a local branch and evidence.
Publication occurs from the assigned isolated worktree, leaving the registered checkout's files, branch, and index alone.
See [Agent delivery and human PR merges](delivery.md) for publication receipts, uncertain retries, evidence freshness, and local-only outcomes.

## Inspect the dashboard

MX Viz is a disposable read-only view of the same canonical task, domain, decision, workflow, freshness, and delivery state used by the terminal workspace.
Switch between **Tasks** for detailed task records and **Agents** for the orchestrator, coordinator and worker hierarchy.
The graph distinguishes assignment state from observed sessions and flags unresolved ownership or partial data.
The terminal workspace key `v` opens it when available.
You can also manage the local server directly from an activated shell after changing to `$MX_ROOT_OVERRIDE`:

```sh
bin/mx-viz.sh serve
bin/mx-viz.sh status
bin/mx-viz.sh stop
```

The server binds only to loopback and prints its URL.
Closing the dashboard does not stop task execution.
See [Live system dashboard](viz.md) for polling, stale-state display, artifact access, and port configuration.

## Diagnose and recover

Run the read-only doctor when state looks inconsistent or after an interrupted lifecycle:

```sh
multplx doctor
multplx doctor --check watcher-lock
multplx doctor --json
```

Exit code `0` means all selected checks are healthy, `1` means at least one warning, and `2` means at least one failure.
Doctor reports the evidence and the command that owns recovery.
It does not infer permission to stop work or resolve decisions.

`multplx doctor --fix` has a narrow repair boundary for a provably stale watcher lock and wake rows whose task metadata is definitely absent.
Uncertain records remain in place.
See [System doctor](doctor.md) before applying a repair.

For an existing pre-redesign home, inspect migration before changing it:

```sh
multplx shell
mx migrate inspect --home /absolute/multplx-home --operation lean-phase09-v1
mx migrate apply --home /absolute/multplx-home --operation lean-phase09-v1
mx migrate summary --home /absolute/multplx-home --limit 20
```

The activated shell exposes the installed `mx` command while preserving the caller directory.
Migration refuses a live writer and creates an exact private backup before changing supported records.
It preserves queues, messages, decisions, workflow runs, receipts, and unresolved uncertainty.
See [Operational-home migration](state-migration.md) for rollback requirements and explicit legacy worktree transfer.

## Common situations

### The checkout has uncommitted work

Multplx leaves it untouched and starts the task from a committed revision.
Commit what belongs in the task, select another clean revision with `--start`, or ask the orchestrator to help decide how to split the work.

### A submission timed out

Retry with the same explicit request ID and identical project, revision, scope, dependency, and artifact inputs.
Do not invent a second ID until you know you want a distinct task.

### A project is missing

Run `multplx projects list` to find its checkout ID and recorded location.
Use `projects repair` only if the repository moved and its identity still matches.
Register a replacement clone as a new checkout.

### Work is waiting

Check the tasks and decisions views for dependencies, a human question, admission capacity, or an unavailable runtime endpoint.
Queued work retains its request identity rather than being dropped.
If a declared prerequisite has no canonical task binding yet, its dependent request stays accepted but spawn refuses with a retry instruction.
Have the orchestrator establish the prerequisite first, then retry the dependent task with the same identity.
A newly launched Codex session may also be waiting at its folder-trust or hook-review prompt before the model can work.
Inspect that exact session, review the installed runtime or assigned worktree and hooks, and accept them when they are the ones you intend to run.
The live Codex 0.156 trials required these first-use confirmations; a launched endpoint alone does not prove the worker has begun.
A completed task can still have an open provider window that consumes admission capacity.
After verifying its current result and recorded endpoint, the orchestrator can close that exact worker with `mx backend kill TARGET` and run `mx headroom --queue-drain` to reconcile capacity.
This preserves the task records and worktree; `/quit` alone may leave the window open, and persistent coordinators have their own explicit lifecycle controls.

### The provider session ended

Open `multplx chat` and inspect the workspace or doctor output.
Multplx can reconcile durable coordination state, but it cannot promise to restore a transcript that the provider itself cannot resume.

### You want to stop watching the terminal

Close the terminal workspace with `q` or use the `afk` skill for explicit away-mode supervision.
Independent work continues according to its recorded lifecycle.

## Where to go next

| Need | Guide |
| --- | --- |
| Install or upgrade | [Getting started](getting-started.md) |
| Configure homes, harnesses, backends, and capacity | [Configuration](configuration.md) |
| Understand project discovery and one-chat routing | [Workspace entry and local projects](workspace-entry.md) |
| Understand isolation and retained work | [Built-in Git worktree lifecycle](worktrees.md) |
| Define or operate workflows | [Workflow definitions and runs](workflows.md) |
| Publish branches and inspect review evidence | [Agent delivery and human PR merges](delivery.md) |
| Diagnose runtime state | [System doctor](doctor.md) |
| Understand the system design | [Architecture](architecture.md) |
