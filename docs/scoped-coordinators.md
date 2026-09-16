# Scoped sub-orchestrators

A sub-orchestrator coordinates one accepted project, repository or idea under the main orchestrator.
It researches, writes briefs, delegates implementation and checks, and returns findings through its recorded parent channel.
Both coordinator roles delegate project code and test changes.
Direct worker delegation remains available, and selecting a project does not create a coordinator.

The [task record contract](subagent-model.md) owns identity, the [configuration guide](configuration.md) owns settings, and [A11](../porting.md#a11-scoped-sub-orchestrators) owns the cross-phase requirements.
The [Phase 05 evidence](../plans/lean_redesign/phase05-implementation.md) records the actual verification status and limitations.

## Explicit domain and private home

The named spawn form takes one coordinator ID, an explicit scope and either project selectors or an idea ID.
Project selectors resolve through the existing registry; an idea can begin research without a Git repository.
Repeat `--project` for a bounded responsibility spanning repositories.
Project overlap does not grant exclusive ownership of a repository.

```sh
mx spawn api-design --sub-orchestrator --project customer-api \
  --scope 'Coordinate the accepted API compatibility work' \
  --request-id api-design-initial --harness codex

mx spawn discovery --sub-orchestrator --idea onboarding \
  --scope 'Research onboarding alternatives and report open questions' \
  --persistent --request-id onboarding-research --harness codex
```

`--persistent` makes the assignment a standing domain that may idle between assigned tasks.
Private coordination state is separate from that persistence choice and from worker Git allocations.
Provisioning remembers project references without copying repositories into the coordinator home.
Use `--json` for a structured result containing the coordinator, domain, home, endpoint and request identity, including queued or retained dispositions.
An idea must acquire explicit project bindings before implementation uses them.
Each task retains its own project, checkout and starting revision when domain scope or display context changes.

Inspect and revise the domain through its owner:

```sh
mx domain inspect discovery
mx teardown discovery --checkpoint
mx domain bind-project discovery --expected-revision 1 \
  --project customer-api --reason 'The research now has an accepted project'
mx domain revise discovery --expected-revision 2 \
  --scope 'Coordinate the accepted onboarding implementation' \
  --reason 'The research outcome and implementation scope were accepted'
```

Scope changes require a stopped, reconciled coordinator, preserve the domain identity and record a new accepted charter revision.
They do not rewrite child task scope or starting revisions.

## Durable communication

Task identity, attempt, accepted revision, parent identity and home identity travel with recorded results.
Transport delivery, acknowledgement, response and task completion are separate facts.
A recorded outcome reaches ancestors through runtime publication and receipts, even when an intermediate model supplies no summary.
Original event identity and artifact pointers remain available beside a coordinator's judgment.
Reasoning-only results become durable only when the agent records them through the report interface.
The report states `blocked`, `needs-decision`, `done`, `failed` and `resolved` enter the parent channel; `working` and `paused` remain local progress.

Each home owns its own inbox consumption.
The root observes child projections and routes requests instead of draining another home's queue.
Missing replies and unanswered questions remain visible, and a delayed answer cannot settle a different accepted revision.
Unknown native-provider lifetime and telemetry remain explicit limits.
A task-bound `mx-report` call with `--state needs-decision --key QUESTION_ID` records its message as a question against the accepted brief revision.
Use `--workflow-revision` when applicable; the report MCP exposes the same field as `workflow_revision`.

Use the channel commands in the owning home:

```sh
mx parent-channel inspect
mx parent-channel relay --limit 64
mx parent-channel answer --task investigate-login --question login-scope \
  --brief-revision 2 --answer-id login-scope-answer \
  --answer 'Use the accepted compatibility behavior'
```

For a coordinator's private channel, use the canonical home returned by spawn, for example `MX_HOME=/absolute/coordinator-home MX_STATE_OVERRIDE=/absolute/coordinator-home/state mx parent-channel inspect`.
These commands act on that home's state and do not accept `--authority-state`.

An answer also takes `--workflow-revision` when the question belongs to a workflow revision.
The stable answer ID makes retries identifiable; superseded answers remain historical evidence.
Channel health exposes pending inbox/outbox counts, oldest pending time, last progress and errors separately from endpoint liveness.
The watcher runs bounded relay work for its own home; a model turn is not required for each hop.
Delivered records move to durable receipts, and active queues have a finite capacity.
Relay recovers delivered records left unarchived by interruption; malformed or unarchived active records keep retirement blocked until repaired.
Capacity or inspection limits remain visible and do not discard accepted report evidence or authorize retirement from a partial scan.
If the root uses a state override outside its home, parent routing requires a live watcher lock proving that exact root home and process identity.
Without that proof, accepted child facts remain queued until the owning watcher is available.

## Lifecycle and ownership

| Command | Intended disposition |
| --- | --- |
| `mx teardown ID --stop-coordinator` | Stop the recorded coordinator endpoint and retain its private home, children and pending work. |
| `mx teardown ID --checkpoint` | Stop the coordinator so its durable context can be used for a later launch. |
| `mx teardown ID --stop-subtree` | Stop verified descendant endpoints while retaining task records, allocations, outcomes and homes. |
| `mx teardown ID --retire-home` | Retire only after child ownership, pending outcomes and unfinished work have a safe disposition. |

An uncertain stop remains unresolved and retains its resources.
Transport success is not proof of cancellation, and a stale control receipt cannot authorize stopping a newer execution.
Retirement does not infer completion from a summary or an empty parent queue.

`mx task-transfer TASK --to COORDINATOR --request-id ID --expected-generation N --reason TEXT` transfers a task through the existing state owner.
Use `--to root` to return an assignment to the main orchestrator and `--to-state` to identify a coordinator whose metadata is owned by another state directory.
Transfer retains the task ID, original evidence and starting revision, reconciles the old execution, and records the successor generation.
The canonical record and its history stay at the recorded authority state, so active children retain their recorded routes.
The successor uses `mx spawn TASK PROJECT --authority-state ABSOLUTE_STATE_PATH` to resume through that authority after validation of its current coordinator identity.
The same `--authority-state` option routes `mx send`, `mx task-model inspect`, `mx task-model revise`, `mx teardown` and subsequent `mx task-transfer` commands.
The command help and returned authority route identify where subsequent operations belong.

Known coordinators and workers share the root admission budget.
The `worker_headroom` setting in [admission configuration](configuration.md#dispatch-capacity-configapi-capacity--configadmission-capacityjson--statedispatch-queue) reserves practical worker capacity; queued work retains its request identity and priority.
An aged coordinator may use the reserve when no coordinator is active, and fresh arrivals yield to older eligible work.
Checkpoint an idle coordinator when its session capacity is needed elsewhere.
Unknown native-provider consumption remains unavailable telemetry rather than an invented zero.

## Compatibility and later integrations

Legacy home routes and `.mx-daemon-home` remain compatibility surfaces during the migration window.
A coordinating charter is evidence of responsibility; persistence alone is not evidence of a coordinator role.
Phase 09 must preserve the original charter, child routes, correlations, leases and inherited settings, and retain ambiguous scope or parentage for reconciliation.
These development interfaces do not migrate real homes automatically.

Phase 06 connects publication, PR evidence and human-merge observations to the outcome channel.
Phase 08 connects workflow outcomes and questions.
Phases 10 and 11 supply the visual domain views; Phase 12 measures complete flat and hierarchical workloads under equal resource budgets.
No throughput improvement is implied by adding a coordinator.
