# Workflow definitions and runs

This document is the single authoritative owner of the Multplx workflow-definition format.
`bin/mx-workflow.sh` is the operator entrypoint, and its header owns exact command syntax.
`bin/mx-workflow-lib.sh` owns parsing, validation, snapshots, run records, contracts, and execution mechanics.

## Definition location and trust

A runnable definition is a repo-tracked regular file at `workflows/<name>.workflow.md`.
`validate` also accepts an untracked draft so the create-workflow procedure can check a file before it is committed.
`run` accepts only a tracked definition under `workflows/`.
The engine copies the definition into `state/<run>.workflow/definition.workflow.md` at launch and creates a normalized `definition.json` beside it.
Every later stage reads only that launch-time snapshot.
An edit to the tracked definition therefore affects future runs and never mutates an in-flight run.
Command text is never read from a stage artifact, an agent result, or a maintainer answer.

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
| `workflow_version` | yes | Integer `1`; every other value is rejected |
| `name` | yes | Privacy-safe slug matching `[A-Za-z0-9._-]+` |
| `description` | yes | One non-empty line |
| `stages` | yes | One or more strictly linear stages |

Every stage requires `id`, `title`, `type`, and `gate`.
Stage ids are unique privacy-safe slugs.
The markdown body must contain exactly one non-empty `## <stage-id>` section for every frontmatter stage and no extra stage section.
The engine executes stages only in declared order.
There are no branches, loops, includes, parallel groups, or sub-workflows in version 1.

## Stage fields

| Field | Applies to | Contract |
| --- | --- | --- |
| `id` | all | Unique privacy-safe stage identity |
| `title` | all | One-line human label |
| `type` | all | Closed enum `interactive`, `agent`, or `command` |
| `gate` | all | Closed enum `approve` or `auto` |
| `output` | any | Safe relative path under the active Multplx home |
| `contract` | agent or command | Closed enum `output` or `local-commits`; a declared output is also an implicit output contract |
| `executor` | agent | Closed enum `broker` or `actor` |
| `fresh_session` | actor agent | Boolean; `true` requires a newly spawned task session |
| `brief_from` | agent | Inline list of prior stage ids that declare outputs |
| `run` | command | One-line shell command from the trusted snapshot |

An `interactive` stage always uses `gate: approve`.
The engine writes its substituted charter under the run's `prompts/` directory and opens a durable maintainer decision hold.
The broker and maintainer conduct the conversation outside the engine, write any declared output, and resolve the hold through `bin/mx-decision-hold.sh`.

An `agent` stage with `executor: broker` runs one structured headless turn through the verified Plan 10 adapter.
The adapter suppresses target-project settings so branch-local instructions cannot replace the stage charter or expand broker authority.
The engine accepts only an exact `{status,message}` JSON result whose status is `done` or `failed`, then independently checks the declared contract.
An `agent` stage with `executor: actor` writes a stage-specific brief and spawns through `bin/mx-spawn.sh`.
Its completion requires a reconciled `done` state from the validated task status path plus the declared contract.
A `failed` actor result parks the workflow as failed.
`MX_WORKFLOW_ACTOR_HARNESS` may provide the already-resolved concrete harness when local dispatch profiles require an explicit choice.

A `command` stage runs as a plain subprocess with captured stdout and stderr.
Exit status zero is ground truth and is an implicit deterministic contract.
The engine runs it in the most recent actor worktree when one exists, otherwise in the launch repository.
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
A local-commits contract requires the actor worktree head to differ from the exact fork point recorded when the stage spawned.
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
stages/<id>.json         stage status, executor facts, contract facts, and gate facts
prompts/<id>.md          exact substituted stage charter
agents/                  structured headless outputs and session ids
commands/                captured stdout and stderr
schemas/                 structured agent-result schema
```

`resume` starts from the first stage that is not durably passed.
`skip <run> <stage> --override <request>` consumes one exact `workflow.skip-stage` grant and writes a truthful `skipped` record; it never calls the stage passed.
`reorder <run> <stage> --before <stage> --override <request>` consumes one exact `workflow.reorder-stage` grant and changes only the private `stage-order.json` snapshot for that run.
Both operations bind the run, immutable definition, current order, named stage records, and exact target before mutation, and neither grant can authorize the other operation.
Run, resume, and abort mutations serialize through one recoverable per-run lock, so simultaneous watcher and operator actions cannot execute a stage twice.
It rejects a later passed record when an earlier stage is unmet.
It rechecks output files, actor state, worktree commits, command markers, and approval holds instead of trusting the last printed event.
An aborted run remains on disk and can never resume or reuse its id.

## Approval routing

An approve gate creates `<run>-decision-<stage>` through `bin/mx-decision-hold.sh` and blocks the workflow backlog item on that hold.
The maintainer's answer is recorded and routed through the existing decision-hold lifecycle.
For example, after saving the accepted answer in a private file, the broker routes it with:

```sh
bin/mx-decision-hold.sh resolve <run> <stage> \
  --decision-file <answer-file> \
  --routed-to <run>
bin/mx-workflow.sh resume <run>
```

The decision command owns its own validation and exact retry identity.
The workflow engine merely observes whether the durable hold is resolved.
This preserves one escalation mechanism and one owner for maintainer decisions.

## Reference workflow

`workflows/new-feature.workflow.md` is the version 1 proving definition.
It composes interactive approach approval, a broker-authored specification, fresh actor implementation, deep-review, and credentialed delivery.
The delivery stage remains interactive because remote writes must run from a maintainer shell or separately credentialed scheduler outside every agent session.

## Upstream review workflow

`workflows/upstream-sync.workflow.md` is the maintained review-and-reimplement path for upstream changes.
Its fetch stage delegates all network and path classification to `bin/mx-upstream-diff.sh`, and its triage stage may propose only `port`, `skip`, or `flag`.
The maintainer reviews every classification before implementation.
An empty approved-port list is valid and produces a port-result artifact without manufacturing a source commit.
When ports exist, the actor reimplements them in Multplx vocabulary, reimplements their regression tests, and uses the ordinary deep-review and delivery path.
The approve-gated record stage occurs before the final advance command because version 1 command gates execute the command before requesting approval.
The final command advances the review cursor only after the maintainer confirms that every approved fix and relevance-map update has landed.
[`upstream.md`](upstream.md) owns the fork point, relevance map, review cursor, cadence, retirement state, and completed-review log.
