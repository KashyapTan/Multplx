# Sub-agent records and compatibility

This reference owns the lean task, execution and project identity schema introduced by Phase 02.
The [configuration guide](configuration.md) owns operational home layout and settings.
The [porting guide](../porting.md) owns the accepted cross-phase contract.
The [Phase 02 evidence](../plans/lean_redesign/phase02-implementation.md) records verification and the development release boundary.

## Separate identities

A task records the requested outcome, accepted brief, project binding and current assignment independently of its execution.
An attempt identifies one execution generation against an accepted brief revision.
A session records the runtime endpoint and observable lifetime of that execution.
A home owns durable coordination state and may persist beyond any one task or endpoint.
Researcher, implementer, reviewer and sub-orchestrator are assignment values, not separate delegation permission classes.
Persistence is independent of the assignment and requested report or implementation output.
Brief and spawn accept `--role` and `--output` independently, with `--persistent` selecting the existing isolated-home lifecycle.

Canonical state extends the existing filesystem owners.
Snapshots are derived views, journals are historical observations, and operation receipts describe recovery progress.
None is a second authoritative task store.
Missing legacy attempt, parent or native-provider identities remain unknown until reconciled; the reader never invents proof of a resumable child.

## Task metadata and accepted revisions

`state/<task-id>.meta` retains its line-oriented format and contains `schema_version=2` plus one `canonical_model=<JSON>` record.
The JSON is the canonical task model; the remaining legacy fields are bounded compatibility projections.
Readers reject duplicate keys, unsupported versions and conflicting identities.
`state/.task-writer-version` identifies the writer schema; initializing a canonical writer over unreconciled legacy tasks is refused.

| Record field group | Meaning |
| --- | --- |
| `task_id`, `owner_home`, `owner_state`, `parent_id`, `parent_home`, `parent_state`, `root_id` | Stable task and home-qualified ancestry, including explicit state-directory overrides |
| `role`, `artifact`, `persistent`, `private_home`, `persistent_home` | Assignment, requested output, isolated coordination home and independent standing-assignment lifetime |
| `attempt`, `prior_attempts`, `retained_executions` | Current generation and preserved superseded work |
| `accepted_brief_revision`, `accepted_brief_digest`, `accepted_brief_path`, `briefs`, `assignments` | Accepted scope/evidence history and explicit responsibility changes |
| `runtime` | Provider, nullable exportable session identity and endpoint |
| `schedule` | Priority, dependencies, runnable/waiting state and revision-bound human decisions |
| `project`, `allocation`, `home_allocation`, `domain`, `owning_coordinator`, `transfers` | Task-local routing and resource/domain ownership |
| `delivery` | Current commit and immutable revision-bound checks, optional review, publication and human-merge history |
| `legacy_unknown` | Historical records whose execution identity cannot be proven |

`mx task-model inspect <task-id>` prints the normalized record without converting the source.
`mx task-model validate` validates the recorded lineage and dependency graph.
`mx task-model revise` records an explicit scope, role or artifact change against `--expected-revision`, with a required reason and optional acceptance criteria, source pointers and accepted brief file.
Use its help for exact option grammar.
The legacy promotion command directs callers to this revision owner rather than granting a new permission class.

```sh
mx task-model inspect investigate-login
mx task-model revise investigate-login --expected-revision 1 \
  --role implementer --artifact implementation \
  --scope 'Implement the agreed login fix' --reason 'Research accepted' \
  --source /work/evidence/login-research.md \
  --acceptance 'The reported login regression passes' \
  --brief-file /work/briefs/login-fix.md
```

Replacement records a new attempt generation only after the old execution has been reconciled and stopped or isolated.
A proven resume retains its attempt; an endpoint label alone does not prove provider session identity.
Superseded attempts, allocation references and evidence remain historical records.
Accepted brief bytes are archived by revision, so a later source-file edit does not silently change the next launch.

The existing `mx-system-snapshot.v1` wrapper includes version 2 `coordination` records and explicit `coordination_error` values on task rows.
Its `portfolio` member is the typed `mx-portfolio.v1` read model shared by JSON status, workspace clients and MX Viz.
`mx-status-snapshot.sh` renders one bounded restart summary of that portfolio in both JSON and TOON; `mx-system-snapshot.sh --json` retains the full canonical detail.
Portfolio tasks use `owner_home#task:<id>` as the stable cross-home key and keep task count separate from current or retained sessions and prior attempts.
They expose project and checkout identity, lineage, role, accepted brief, workflow stage, dependencies, decisions, revision-bound delivery evidence, allocation observations and latest meaningful change without becoming another state owner.
`native_observations` retains provider starts, results and interruptions with nullable child identity; known provider children are deduplicated into session counts while observations without child identity do not inflate them.
The only freshness status values are `fresh`, `partial`, `stale` and `unknown`; unavailable task or provider facts remain null with a reason instead of becoming idle, complete or passing.
Decision waiting time comes from the accepted `needs-decision` evidence envelope when that durable owner record is available; older or bounded-away evidence leaves it unknown.
Child-home task rows and domain observations are bounded, and omitted rows remain visible through counts and partial-state markers.
Domain coordinators expose `validated_home` only after the recorded runtime home passes daemon-home identity and containment validation.
MX Viz uses that field to authorize exact nested-home evidence links.

## Delivery evidence and human review

`mx task-model evidence <task-id> --request-file <JSON-path>` records evidence through the canonical task owner.
`mx task-model review-queue` returns dependency-ordered PR outcomes with priority, checks, limitations and revision freshness.
`mx task-model inspect <task-id>` retains the full evidence history, including superseded commits and scopes.

The closed request schema is owned by [`EvidenceRequest`](../crates/multplx-domain/src/lifecycle/delivery_evidence.rs).
It binds a stable `evidence_id`, exact `attempt_id`, `attempt_generation`, `brief_revision`, `commit` and RFC3339 `observed_at` to the recorded result.
`checks` contains named results (`passed`, `failed`, `not-run` or `unknown`), summaries and optional artifact pointers.
`review` is optional independent evidence, not an implicit mandatory review or publication approval.
`limitations` and optional `pr_url` preserve what the result actually establishes.
The stored `outcome` distinguishes `evidence-updated`, `published`, `publication-failed` and `human-merged`.
The public evidence command accepts only `evidence-updated` and cannot introduce another PR identity; verified publication and poll owners record the other outcomes.
Current evidence uses `mark_current` and the optimistic `expected_current_commit` token; read-only historical observations cannot replace the current revision.
Use a new evidence identity for a new observation and the same identity and facts for a retry.
Never hand-edit the embedded metadata JSON.

A subsequent attempt, accepted brief or implementation commit makes earlier evidence historical.
A branch push or PR creation does not establish passing checks, completed independent review or human merge.
The queue reports unresolved dependencies and stale revisions instead of making them ready.
Durable parent outcomes carry accepted delivery facts without requiring a coordinator summary.
[Delivery](delivery.md) owns publication, authentication and human-only merge usage.

## Reports and recovery

Canonical reports carry task, attempt ID, attempt generation and accepted brief revision.
Launch exports `MX_TASK_ID`, `MX_ATTEMPT_ID`, `MX_ATTEMPT_GENERATION`, `MX_BRIEF_REVISION` and the parent-owned report location to the existing harness/report adapters.
The report CLI accepts explicit identity arguments, stable message IDs and artifact pointers; `bin/mx-report --help` owns its exact syntax.
Stale attempts and stale brief evidence are retained as historical results and cannot advance current task status.
The message envelope also records task/parent homes, parent, validated sender/recipient, correlation ID, creation time, kind, summary, artifact and acknowledgement state.
Nullable fields distinguish non-applicable identity from fabricated native-provider children.

The core filesystem transition primitive records stable operation intent under `.transitions`, then writes preparatory records before a final authoritative record.
The final atomic replacement is the acceptance point; receipt completion records that recovery has reconciled that publication.
Repeating the same operation resumes its exact intent; conflicting identities or unfinished overlapping operations refuse and retain their records.
Locks cover local validation and publication, with no external command or human/model wait inside the transition lock.
These primitives do not promise exactly-once execution of external commands.
Launch records a durable task/attempt reservation before creating an endpoint and releases task and inheritance locks before backend commands.
It revalidates the reservation and accepted identity when publishing the result.
An interrupted launch retains its intent and refuses another external launch; Phase 04 owns automatic reconciliation and repeat-safe external actions.

## Compatibility boundary

| Legacy surface | Canonical meaning |
| --- | --- |
| `kind=delivery` | Implementation output and implementer assignment |
| `kind=scout` | Report output and researcher assignment |
| `kind=daemon` | Persistent isolated home, with assignment recorded separately |
| `mode=local-only` | Local publication destination |
| `mode=direct-PR` or `mode=deep-review` | PR publication destination; legacy deep-review does not select a new review run |
| `yolo=on`, `+yolo` | Historical compatibility input with no merge authority |
| `config/actor-harness` | Alias of `config/subagent-harness` |
| `config/actor-dispatch.json` | Alias of `config/subagent-dispatch.json` |
| `config/daemon-harness` | Alias of `config/persistent-subagent-harness` |
| `data/daemons.md`, `.mx-daemon-home` | Existing persistent home ownership, parent routes and restart facts |
| `data/projects.md` | Legacy project configuration and managed-clone evidence |

Aliases remain supported through the Phase 12 cutover checks.
The compatibility window does not permit two incompatible writers in the same home.
Historical records, message correlation IDs, journals, leases and unfinished artifacts are retained.
[`mx migrate`](state-migration.md) owns inspect/apply migration, quiescence, private backups, compact restart summaries and matching rollback of operational homes.
Legacy task conversions remain `legacy_unknown` until an exact attempt and accepted brief are reconciled.

## Project and checkout routing

`mx project register PATH [--alias NAME] [--managed]` records a selected local Git checkout.
The default ownership is `user-owned`; `--managed` explicitly declares a Multplx-owned clone and cannot upgrade an already borrowed checkout.
`mx project list` prints the version 2 filesystem catalog at `data/projects.json`.
`mx project resolve SELECTOR` returns the selected project/checkout/base binding or an ambiguity error with candidate paths.
`mx project forget CHECKOUT_ID` removes one remembered location.
These commands register explicit choices; recursive discovery and workspace entry remain Phase 11 work.

```sh
mx project register /work/customer-api --alias customer-api
mx project register /work/reporting --alias reporting
mx project list
mx project resolve customer-api
```

Project identity describes a local Git repository and its shared Git metadata; checkout identity describes one selected working location.
Linked worktrees can share a project identity while retaining distinct checkout identities.
Separate clones remain separate projects even when their basename or remote URL matches.
Exact locations and aliases resolve through the project registry owner; ambiguity returns candidate paths.
Registration remembers an existing source and does not change its branch, index or working files.
Unregistering a checkout removes its registry entry and never deletes its files.

Each accepted task, attempt and queued request binds its project, checkout and exact starting commit before dispatch.
Changing display context or advancing the source checkout's branch does not retarget that binding.
A new execution verifies the returned working location's common Git identity and starting commit before submitting the harness command.
An incompatible provider result is retained and refused rather than silently changing the accepted base.
A missing, moved or replaced location requires reconciliation; a same-named repository cannot substitute for it.
Remote-free projects retain a local destination without an invented forge URL.
Borrowed checkouts remain user-owned and are excluded from managed-clone synchronization.
Teardown validates canonical source identity and refuses removal targets that overlap remembered user-owned checkouts.

## Allocation and coordinator foundations

An allocation binds project/common-Git identity, exact path, base commit, task attempt, persistence and an acquisition generation.
A stale lease cannot release a replacement allocation of the same path.
The [built-in lifecycle](worktrees.md) implements Git acquire, inspect, retain, release and scoped prune.
An optional `home_allocation` binds persistent private-home path, owner, lease and generation independently of a live process.
This is an additive canonical version 2 field; absent legacy ownership remains unproven, never authorization for cleanup.
Legacy external-provider leases retain their uncertainty until ownership is established by that lifecycle.

A coordinator domain records its scope revision, assignment generation, root home, immediate parent and owning home.
Project references do not form exclusive repository locks: separate domains may work in the same project.
A task has one current owning coordinator and acyclic lineage.
Transfer changes ownership explicitly while retaining task identity and historical assignments.
The [scoped coordinator reference](scoped-coordinators.md) describes Phase 05 provisioning and durable parent-channel composition using these identities.
`private_home` is additive in schema version 2; an older persistent record still represents a private home when that field is absent.
The coordinator task record remains authoritative, while delivery receipts and read-only domain projections reference it.

The [parent-channel module](../crates/multplx-domain/src/lifecycle/parent_channel.rs) is the integration owner for Phase 06 delivery and Phase 08 workflow producers.
Call `prepare_outcome` with the canonical `MessageEnvelope` while accepting the fact, retain its frozen `ParentOutcome` in the same transition evidence, and use `persist_prepared` for recoverable publication before notification.
Recovery preserves that accepted identity even after the originating attempt is replaced; it must not reconstruct an old fact from the current attempt.
`record_outcome` combines preparation and persistence for an already-accepted current event, while `prepare_report` selects the report states that belong in the parent channel.
`bind_human_question` and `record_human_answer` preserve exact question, brief and optional workflow revision identity.
`inspect` provides bounded channel health, and `relay` advances only the active home's owned work while preserving each foreign outcome's original envelope.
