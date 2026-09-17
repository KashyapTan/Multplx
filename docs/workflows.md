# Workflow definitions and runs

This document is the single authoritative owner of the Multplx workflow-definition format.
`bin/mx-workflow.sh` is the transport-only operator entrypoint, and `mx authority mx-workflow.sh --help` exposes the exact Rust-owned command syntax.
`multplx-domain::workflow` owns the constrained parser, immutable snapshot model, and typed stage-order transitions.
The Rust `multplx-domain::workflow` parser and `multplx-cli` workflow runtime own validation, immutable launch snapshots, stage execution, and reconciliation.

## Definition location and trust

A runnable definition is a repo-tracked regular file at `workflows/<name>.workflow.md`.
`validate` also accepts an untracked draft so the create-workflow procedure can check a file before it is committed.
`run` accepts only a tracked definition under `workflows/`.
The engine copies the definition into `state/<run>.workflow/definition.workflow.md` at launch and creates a normalized `definition.json` beside it.
Every later stage reads only that launch-time snapshot.
An edit to the tracked definition therefore affects future runs and never mutates an in-flight run.
Command text is never read from a stage artifact, an agent result, or a maintainer answer.
The public entry point selects the Rust authority engine before launch or resume can publish state, and the compatibility executor is process-pinned before any retained stage composition begins.

Ordinary workflow stages use generic structured agent transport and do not instantiate deep-review policy, a gate-agent identity or credential restrictions.
Explicit deep-review or vplan command stages remain declared stages and execute only when the selected definition contains them.

`run:` is arbitrary code execution approved by accepting the tracked workflow definition.
The free-form `{input}` substitution is forbidden in `run:` because interpolating untrusted launch text into a shell command would violate the snapshot trust boundary.
Command stages may use the privacy-safe `{run}` substitution and a validated `{output}` path.
The engine supplies `MX_WORKFLOW_HOME`, `MX_WORKFLOW_RUN`, and `MX_WORKFLOW_WORKTREE` to command subprocesses.

## Constrained frontmatter

The file begins and ends its machine section with a line containing exactly `---`.
The parser accepts only the fields documented here, two-space stage-list indentation, four-space stage-field indentation, scalar values, booleans, and inline `brief_from` lists.
YAML anchors, aliases, block scalars, nested maps, and other general YAML features are intentionally unsupported.
This constrained grammar needs no YAML runtime dependency and fails closed on unfamiliar syntax.

The top-level fields are:

| Field | Required | Contract |
| --- | --- | --- |
| `workflow_version` | yes | Integer `2` for new definitions; version `1` remains a compatibility grammar |
| `name` | yes | Privacy-safe slug matching `[A-Za-z0-9._-]+` |
| `description` | yes | One non-empty line |
| `stages` | yes | One or more strictly linear stages |

Every stage requires `id`, `title`, `type`, and `gate`.
Stage ids are unique privacy-safe slugs.
The markdown body must contain exactly one non-empty `## <stage-id>` section for every frontmatter stage and no extra stage section.
The engine executes stages only in declared order.
There are no branches, loops, includes, parallel groups, or sub-workflows in version 2.

## Stage fields

| Field | Applies to | Contract |
| --- | --- | --- |
| `id` | all | Unique privacy-safe stage identity |
| `title` | all | One-line human label |
| `type` | all | Closed enum `interactive`, `agent`, or `command` |
| `gate` | all | Closed enum `approve` or `auto` |
| `output` | any | Safe relative path under the active Multplx home |
| `contract` | agent or command | Closed enum `output` or `local-commits`; a declared output is also an implicit output contract |
| `executor` | agent | Closed enum `orchestrator-context` or `sub-agent-session` |
| `assignment` | agent | Descriptive `researcher`, `implementer`, `reviewer`, or `sub-orchestrator` |
| `fresh_session` | sub-agent agent | Boolean; `true` requires a newly spawned task session |
| `brief_from` | agent | Inline list of prior stage ids that declare outputs |
| `run` | command | One-line shell command from the trusted snapshot |

An `interactive` stage always uses `gate: approve`.
The engine writes its substituted charter under the run's `prompts/` directory and opens a durable maintainer decision hold.
The orchestrator and human conduct the conversation outside the engine, write any declared output, and resolve the decision through `bin/mx-decision-hold.sh`.

An `agent` stage with `executor: orchestrator-context` runs one structured turn through the generic file-backed transport.
The operator supplies that transport explicitly with `MX_WORKFLOW_AGENT_COMMAND`; its command receives schema, prompt, output and session paths and owns no review policy.
The engine accepts only an exact `{status,message}` JSON result whose status is `done` or `failed`, then independently checks the declared contract.
An `agent` stage with `executor: sub-agent-session` writes a stage-specific accepted brief and spawns through `bin/mx-spawn.sh`.
Its launch and every resume bind the task, attempt, accepted brief, project, checkout and exact durable allocation from the canonical task record.
Its completion requires a reconciled `done` state plus the declared contract; a prose summary or child readiness is insufficient.
A failed sub-agent result parks that workflow and its dependents without stalling independent work.
`MX_WORKFLOW_SUBAGENT_HARNESS` may provide the already-resolved concrete harness when local dispatch profiles require an explicit choice.
`MX_WORKFLOW_ACTOR_HARNESS` remains a compatibility alias for older test and operator integrations.

An implementer always requires `sub-agent-session`, so project coding cannot run in the main orchestrator context.
A sub-orchestrator also requires `sub-agent-session` and provisions the Phase 05 bounded coordinator form with an exact request identity, project and scope.
The coordinator remains the one stage owner, may delegate within that stage, and advances only when its own declared contract passes.
Researcher and reviewer are descriptive assignments rather than privilege classes.
They default to researcher in orchestrator context and implementer in a sub-agent session when version 2 omits `assignment`.

Version 1 maps `broker` to `orchestrator-context` and `actor` to `sub-agent-session` while retaining the immutable source version.
Existing normalized snapshots deserialize both legacy spellings, so in-flight runs continue without rewriting their definition.
New definitions use version 2 and the current vocabulary.

`run --request <request-id>` consumes the durable Phase 02/A3 request binding as workflow input.
The request supplies the exact batch, task ID, project, checkout, starting revision, accepted brief, scope, dependency IDs and optional original context artifact.
Conflicting `--id`, `--input`, `--project` or `--depends` values fail before a run snapshot is created, so one conversation can launch separately tracked repository workflows without losing correlation.

A `command` stage runs as a plain subprocess with captured stdout and stderr.
Exit status zero is ground truth and is an implicit deterministic contract.
The engine runs it in the most recent implementation sub-agent allocation when one exists, otherwise in the launch repository.
A nonzero exit records the exact output paths and opens a failure hold.
When a composed lifecycle such as deep-review is durably parked, the workflow waits for that lifecycle instead of inventing a second finding channel.

## Gates and contracts

`gate: auto` advances only after a deterministic contract succeeds.
An agent auto gate without `output` or `local-commits` is invalid.
A command auto gate is valid because exit status zero is deterministic.
`gate: approve` requires the deterministic contract and a resolved maintainer hold before the stage passes.

An output contract requires the resolved file to exist and contain at least one byte.
Output paths are relative to the Multplx home, cannot contain `..`, and cannot escape through substitution.
Every existing path component must also be non-symlink, so an artifact cannot redirect a contract or command outside the home.
A local-commits contract requires the sub-agent allocation head to differ from the exact task-bound base revision recorded when the stage spawned.
A command contract requires exit status zero and any additionally declared output or local-commits contract.
The contract vocabulary is closed.
Adding a contract requires engine code, validator coverage, and behavior tests.

The only prompt substitutions are `{run}`, `{input}`, and `{output}`.
`{run}` is the privacy-safe run id.
`{input}` is the free-form launch description and is allowed only in stage bodies.
`{output}` is the resolved absolute path for that stage and is valid only when the stage declares `output`.
Output declarations themselves may use only `{run}`.

## Durable run layout

Each run owns `state/<run>.workflow/` with mode-private records:

```text
definition.workflow.md  immutable launch snapshot
definition.json         validated normalized snapshot
input.txt               exact free-form launch input
run.json                run identity, launch repo, current stage, and status
plan-history.json       explicit versioned skip/reorder changes, when any
stages/<id>.json         stage status, executor facts, contract facts, and gate facts
prompts/<id>.md          exact substituted stage charter
agents/                  structured headless outputs and session ids
commands/                captured stdout and stderr
schemas/                 structured agent-result schema
decisions/<id>.json      question, task, brief/workflow target and durable answer
```

`resume` starts from the first stage that is not durably passed.
`skip <run> <stage> --override <request>` consumes one exact `workflow.skip-stage` grant, increments the plan revision and writes a truthful `skipped` record; it never calls the stage passed.
`reorder <run> <stage> --before <stage> --override <request>` consumes one exact `workflow.reorder-stage` grant, increments the plan revision and changes only the private `stage-order.json` snapshot for that run.
Both operations bind the run, immutable definition, current order, named stage records, and exact target before mutation, and neither grant can authorize the other operation.
Neither operation is accepted while a human decision is open; resolve that revision or abort and start a new run instead of applying its delayed answer to a changed plan.
Run, resume, and abort mutations serialize through one recoverable per-run lock, so simultaneous watcher and operator actions cannot execute a stage twice.
It rejects a later passed record when an earlier stage is unmet.
It rechecks output files, current task/attempt/brief/allocation identity, worktree commits, command markers, and decision records instead of trusting the last printed event.
An aborted run remains on disk and can never resume or reuse its id.

`run --project <selector>` resolves one remembered checkout through the Phase 02 registry.
The compatibility `--repo <path>` records that selected existing checkout as user-owned and stores the same immutable project, checkout and starting-revision binding.
`--depends <run>` is repeatable, rejects self, duplicate, missing and cyclic graph entries, and prevents stage execution until every named run has completed.
A waiting dependency or human decision affects only that run, so independently bound repositories continue.
One correlated request batch may create separately identified per-repository work; launch one project-bound workflow per accepted task rather than mixing repository instructions or evidence.

## Approval routing

An approve gate creates `<run>-decision-<stage>` through `bin/mx-decision-hold.sh` and blocks the workflow backlog item on that decision.
The record binds the question, task, accepted workflow brief revision, immutable definition digest and current plan revision.
The maintainer's answer is recorded and routed through the existing decision-hold lifecycle.
For example, after saving the accepted answer in a private file, the orchestrator routes it with:

```sh
bin/mx-decision-hold.sh resolve <run> <stage> \
  --decision-file <answer-file> \
  --routed-to <run>
bin/mx-workflow.sh resume <run>
```

The decision command owns its own validation, revision target and exact retry identity.
The workflow engine copies the resolved question and answer into `decisions/<stage>.json` only when that target is still current.
A delayed answer for another definition, brief or plan revision remains historical and cannot advance the run.
This preserves one escalation mechanism and one owner for maintainer decisions.

## Reference workflow

`workflows/new-feature.workflow.md` is the version 2 proving definition.
It composes interactive approach approval, an orchestrator-context specification and fresh sub-agent implementation with exact evidence.
Deep-review and vplan run only when a user explicitly requests them or knowingly selects a definition that declares them.
That selected definition retains its declared interactive stage; ordinary branch or PR publication is part of the implementation result when requested and has no separate delivery approval stage.

## Upstream review workflow

`workflows/upstream-sync.workflow.md` is the maintained review-and-reimplement path for upstream changes.
Its fetch stage delegates all network and path classification to `bin/mx-upstream-diff.sh`, and its triage stage may propose only `port`, `skip`, or `flag`.
The maintainer reviews every classification before implementation.
An empty approved-port list is valid and produces a port-result artifact without manufacturing a source commit.
When ports exist, the implementation sub-agent reimplements them in Multplx vocabulary, reimplements their regression tests, and uses focused verification plus ordinary delivery.
An explicitly requested review remains available but is not an implicit publication stage.
The approve-gated record stage occurs before the final advance command so approval precedes the deterministic cursor mutation.
The final command advances the review cursor only after the maintainer confirms that every approved fix and relevance-map update has landed.
[`upstream.md`](upstream.md) owns the fork point, relevance map, review cursor, cadence, retirement state, and completed-review log.
