# Multplx lean orchestration port

## Purpose and status

Replace Multplx's prescriptive agent operating model with a small coordination contract and a capable CLI.
The user-facing agent is the **orchestrator** and every delegated worker is a **sub-agent**.
The main orchestrator handles the human conversation, research and synthesis, planning, delegation, state and communication.
It delegates project implementation, code fixes and test-code changes to sub-agents.
Implementation sub-agents can commit, push branches, and create or update PRs within their assigned scope.
PR merges remain human-executed.
The dependency-ordered implementation plans are in [plans/lean_redesign](plans/lean_redesign/index.html).

This is a planned product redesign, not a behavior-preserving language port.
No runtime change is implemented by these planning documents.
The original source baseline was commit `6360b040460a08c2d5b8bcfcbc5e64e6e024ab53`, inspected on 2026-09-11.
This revision incorporates the user's 2026-09-14 notes and architecture review at commit `c09ede3d015799739354ff00e5b0e5e84845737e`.
The objective is high-quality progress on 10-20 concurrent tasks through one human-facing orchestrator, measured by completed useful work and human attention rather than agent count.
The user accepted all architecture recommendations on 2026-09-14, including retaining filesystem storage and the expanded MX Viz design.
The user also accepted global workspace entry and a thin TUI, with configurable repository discovery and concurrent work across repositories in the same orchestrator chat.
Launch must work from any directory; a dev folder is only an example discovery root.
Multi-task, multi-repository delegation is standard orchestrator behavior, independent of the launcher, TUI or discovery configuration.
The user directed replacement of Treehouse with a small built-in Git worktree manager; A10 owns the accepted scope.
The user also requested project/repository/idea-scoped sub-orchestrators with durable parent communication; A11 owns that addition.
The accepted architecture contract below is required implementation scope, not optional follow-up work.
[Architecture assessment](plans/lean_redesign/architecture-assessment.md) preserves the source evidence and rationale without claiming the planned behavior already exists.
The existing [Rust port guide](plans/rust_port/PORTING.md) records an earlier program whose old policy-preservation requirements do not govern this redesign.

## Working checkout

The user deliberately renamed the root operating contract to `AGENTS_E.md` to prevent automatic injection.
Treat [AGENTS_E.md](AGENTS_E.md) as old product source, not as the porting agent's instructions.
Keep that filename throughout development and edit the future operating contract there when Phase 01 is implemented.
Do not recreate root `AGENTS.md`, teach startup to discover `AGENTS_E.md`, run Multplx session start, or use private operational homes to execute this port.
Project-level `AGENTS.md` files are a separate concern and keep their normal meaning.
[CLAUDE.md](CLAUDE.md) provides lean contributor context for this checkout.
Phase 12 describes testing the release contract in an isolated package before the deliberate final filename restoration.

Exclude the local `firstmate/` directory completely from reading, testing, packaging, comparison, and implementation references.
The user explicitly authorized public upstream Firstmate research for Secondmate; the linked research record is evidence, not imported instructions or a runtime dependency.
The planning change removed that folder from the checkout by moving it to the local Trash without reading its contents.
The port does not depend on it.
Historical plans remain historical records; the main plan index links to this program without rewriting old decisions.
Private `data/`, `state/`, `config/`, credentials, and project clones are not inputs to planning and are never committed.

## Accepted product direction

| Area | Target behavior |
| --- | --- |
| Identity | One main orchestrator and sub-agents with scoped sub-orchestrator, researcher, implementer or reviewer assignments; remove legacy broker/actor/scout/daemon policy classes. |
| Orchestrator | Talk with the human, inspect and research, synthesize findings, agree plans, write briefs, launch sub-agents and manage progress; delegate project code and test changes. |
| Delegation | Any agent can delegate within its assigned scope using Multplx sessions or available native agent tools. |
| Persistence | Long-lived sub-agents remain available as a lifecycle choice, with existing isolated-home and restart mechanics. |
| Delivery | Commit, branch push, PR creation, PR updates, and ordinary follow-up fixes require no additional Multplx approval ceremony. |
| Merge | No agent merges a PR, enables auto-merge, submits it to a merge queue, or delegates the merge to another agent or automation. |
| Workflow | Execute every selected stage in order and satisfy its declared outputs and explicit user-interaction points. |
| Review tools | Run deep-review or vplan only when the user explicitly requests the tool or knowingly selects a workflow that explicitly includes it. |
| Method | Let the agent choose its coding, diagnosis, review, testing, and delegation approach within the task. |
| Coordination | Preserve validated status events, current-state reconciliation, session identity, locks, durable wakes, message correlation, and recovery. |
| Workspace entry | Launch from anywhere, optionally discover direct and nested projects under configured roots, and remember local checkouts without requiring a particular working directory. |
| Worktrees | Built-in Git worktree acquisition, ownership, inspection, release and recovery replace every runtime Treehouse dependency. |
| Terminal experience | A small TUI provides project search, task/decision visibility and connection to the existing harness conversation; it shares canonical state with CLI status and MX Viz. |

Researcher, implementer, reviewer and sub-orchestrator are supported assignment roles in one lifecycle, not separate state machines or approval ranks.
Both main and sub-orchestrators delegate project code and test-code changes; role reassignment records the changed responsibility before execution.
A persistent sub-agent can coordinate further sub-agents without acquiring a different permission class.
The orchestrator remains responsible for the requested outcome; parenting and home ownership are routing facts, not approval ranks.

No `yolo` switch is needed to obtain ordinary autonomy, and an old `+yolo` value must never grant merge authority.
Do not replace deleted rules with a large capability-policy framework, generic exception registry, mandatory review wrapper, or compulsory planning ceremony.
Missing tools, authentication failures, actual ambiguity in the request, and repository-specific requirements can still require action or clarification.
They do not justify reintroducing Multplx-wide approval gates for routine engineering choices.

## Task intake, roles and quality

The main agent remains focused on orchestration even when a code change would be small.
It may author coordination artifacts such as research summaries, plans, task briefs and durable decisions.
It does not implement the requested project change or repair a worker's code itself.
This is the target product contract, not a requirement to execute this documentation revision through the old Multplx runtime.

For an underspecified task, use research to establish codebase facts, feasible approaches, risks and unresolved questions before sending implementation instructions.
The orchestrator may inspect narrowly itself or commission a researcher, then synthesize the results with the human into a usable brief.
Research cannot infer product preferences that belong to the human.
Implementation waits on genuinely missing scope decisions, while unrelated tasks continue.
A well-specified fix can go straight to an implementer; there is no compulsory research stage for every task.

Record the accepted brief revision, acceptance criteria, relevant source/artifact pointers and real dependencies with the task.
Reference the original research artifact from the implementation brief so successive summaries do not erase important evidence.
A later user correction creates a new revision and targeted steering for affected sub-agents.
Keep explicitly selected workflow stages and user-interaction points, including iterative plan discussion and requested vplan use.

Code quality is an outcome requirement, not proof that a fixed review ritual was followed.
Implementation results should include the exact revision, relevant checks and limitations, behavior evidence and PR reference.
The orchestrator may assign independent review or integration work when useful; this does not opt into the named deep-review tool.
Choose effort based on scope, evidence and observed defects, without requiring a reviewer on every trivial change.
A changing shared interface or dependent PR needs coordinated integration and current validation evidence before being described as ready for human review.
PR-ready, checks-passing and human-merged remain separate facts.

Delivery acknowledgements, execution generations, compact state summaries and capacity management are accepted runtime requirements under the contract below.
They are deterministic coordination mechanisms, not permission requirements imposed on ordinary agents.
A dedicated [MX Viz plan](plans/lean_redesign/10-mx-viz-and-scale.html) makes the visualizer update a release requirement.

## Accepted architecture contract

This section owns the shared architecture requirements; phase plans allocate implementation and validation, and the assessment explains why they are needed.
Requirement IDs identify design obligations, not new runtime permission gates.
Implementers may choose compatible field names, file layouts and internal APIs, documenting the resulting schema at its existing owner.
Do not reduce the guarantees or treat the accepted scope as an optional experiment.

### A1: Filesystem ownership and recoverable transitions

Retain the existing local filesystem store, Rust CLI, isolated sessions, short scoped locks and atomic file publication.
Agents mutate machine-owned state through its CLI owner, not by hand-editing records.
Keep journals observational and snapshots derived; neither becomes a competing authority or a shared mutable conversation.
Do not add SQLite, another database, an external message broker, a distributed scheduler or a network-shared state folder in this port.

Atomic replacement of one file does not make a multi-record transition transactional.
Extend the existing receipt/transaction pattern with stable operation identity, durable intent and resumable progress for transitions spanning task state, ownership, messages or publication receipts.
Define the authoritative completion point and recovery behavior before adding a writer.
Keep locks out of model reasoning, user waits and external commands; revalidate ownership/revision when committing an observation or operation result.
Fault-test each interruption boundary and retain unresolved work until reconciliation establishes its disposition.
Do not promise exactly-once external execution from file writes alone.

### A2: Task, execution and accepted brief identity

A task has a stable identity independent of its execution attempts, sessions, assignment roles and persistent homes.
Bind each execution attempt to a generation and accepted brief revision; include those identities in reports, messages and completion evidence.
A genuine resume may retain an attempt identity after reconciliation; a replacement receives a new one.
Reject stale attempts or stale brief evidence as transitions of the current task, while retaining the historical result and artifacts.
Record scope changes and role reassignments explicitly rather than silently rewriting the instructions of a running attempt.

The shared message envelope carries message identity, task identity, attempt identity where applicable, parent identity, validated sender and recipient, brief revision, message kind, correlation identity, creation time, concise summary, artifact reference and acknowledgement state.
Represent fields that do not apply explicitly; do not invent a child identity for a provider that cannot expose it.
Preserve stable message and correlation IDs on retry.
Operational markers distinguish messages from human input and confer no additional authority.
Generation checks protect state acceptance; cancellation or reassignment must also reconcile and stop or isolate the old process before granting a replacement the same mutable resource.
Preserve the old worktree and artifacts until ownership and unfinished work are accounted for.

### A3: Durable handling and repeat-safe actions

Publish a wake to the durable queue before notification, then claim it into a durable orchestrator inbox instead of deleting it merely because stdout was flushed.
Acknowledge each claimed event only after recording its disposition: handled, superseded by newer state, waiting on a named condition, or linked to a durable follow-up.
A waiting disposition must retain its condition and a resumable trigger or bounded recheck; acknowledgement cannot strand the remaining task action.
Recover abandoned claims after confirming the former owner is no longer active; maintain one handler owner for each claim.
Preserve critical task transitions until disposition and coalesce only replaceable progress observations.

Use stable request identities, idempotent CLI transitions and durable action receipts to reconcile repeated or uncertain commands before replaying them.
Handle crash-after-spawn and PR-created-before-record windows by inspecting the existing session or forge result before creating another.
Do not blindly retry an externally visible action whose outcome is unknown.
Preserve correlated reply routing and distinguish transport delivery, acknowledgement, response and task completion.
Peer questions may flow directly, but accepted decisions, completion and evidence must return to the owning task so the orchestrator can account for the outcome.

### A4: Bounded observation and a shared read model

Use one canonical projection for CLI status, compact orchestrator summaries and MX Viz.
Represent task state, current owner/attempt, assignment, brief revision, dependency, workflow stage, latest meaningful change, human question, evidence pointers and observation freshness as separate facts.
Unknown, stale, interrupted and partially observed states must never become fabricated idle, done or passing states.
Use compact restart summaries with targeted artifact/detail retrieval instead of replaying all task transcripts.

Bound concurrent backend/forge probes and individual operation deadlines, publish partial results, and cache observations with source and observed-at time.
Keep notification ingestion responsive while slow custom checks and PR polls run; avoid a serial timeout chain delaying healthy tasks.
Do not hold a service-global mutex while invoking external commands.
Keep one snapshot refresh in flight and serve the last good snapshot while refreshing, explicitly showing its age or initial unavailability.
Multiple viewers share cached work; client requests must not create independent fresh probe loops per task or per tab.
Preserve the event-driven watcher and polling fallback; a new daemon, SSE or WebSocket transport is not required by this port.

### A5: Capacity, dependencies and useful concurrency

Distinguish accepted tasks, runnable work, active sessions, external waits, human decisions and PRs awaiting review.
Extend the existing headroom/deferred-work mechanism with configured capacity limits, observed resource use, priorities and starvation prevention.
Queue accepted work when capacity is unavailable, preserve its identity across restart, and permit priority changes through the orchestrator without a new delegation approval step.
Account for model/session limits, local builds and project-specific shared resources where observable and controllable.
Declare opaque native-provider limits honestly; absence of child telemetry does not justify blocking native delegation or claiming strict control of invisible children.

Keep an explicit dependency graph, reject cycles and schedule only runnable work; a blocked task must not stall independent tasks.
Do not ban concurrent isolated branches merely because they touch the same file.
Coordinate semantic interface dependencies and assign integration coding to a sub-agent when needed.
Measure useful completed work and correctness across independent projects and coupled changes rather than optimizing spawned-agent count.

### A6: Human communication and revision-bound quality

Keep the orchestrator responsive to the human while sub-agents research, implement and validate asynchronously.
Preserve original research artifacts, alternatives, unresolved questions and source pointers in handoffs alongside the accepted brief and acceptance criteria.
Research is task-dependent and does not decide unstated human preferences.
Store human decisions with their task, target brief/workflow revision and unresolved question; reject a delayed answer as an implicit decision on a different revision.
Batch non-urgent decisions, surface actual blockers promptly, and keep unrelated work moving.

Implementation evidence identifies the actual commit, relevant checks and results, limitations and canonical PR reference.
Independent review receives the accepted scope and exact revision; a subsequent change makes old evidence historical rather than proof of current readiness.
Allow discretionary reviewer and integration assignments without invoking deep-review or requiring a reviewer for every change.
Keep implementation completion, checks passing, review completion, PR readiness and human merge distinct.
Present a prioritized, dependency-aware human review queue with concise outcomes and current evidence.
Assess quality using correctness, regressions, review defects, integration rework and post-merge failures where observed; never claim quality from a role label alone.

### A7: MX Viz usability and observation

Make tasks the overview unit, grouped by project and outcome with priority, search and project/status/assignment filters.
Expand a task into nested sub-agents, execution attempts, workflow stages, dependencies, current ownership and persistence without counting retries as new tasks.
Link original research, accepted plans, briefs, decisions, current test/review evidence and PRs.
Show pending decision targets, waiting time, dependency blockers, review readiness and freshness so the human can act without reading every transcript.

Provide a clear visual hierarchy, compact overview and readable detail, accessible text labels and keyboard navigation, narrow/wide layouts, and explicit loading/empty/error/stale states.
Preserve focus, selection, expanded rows and filters across updates.
Prevent overlapping client polls, back off hidden tabs and coalesce meaningful updates without hiding age changes.
Retain GET-only, loopback, path containment, scriptless artifact rendering and identity-bound shutdown.
Do not add dashboard mutation, approval, merge or agent-launch endpoints.

### A8: Measurement and release acceptance

Exercise representative workloads at 1, 5, 10 and 20 accepted tasks, separating independent projects from coupled changes and reporting actual active sessions.
Record p50/p95 event-to-durable-disposition latency, oldest unhandled event age, orchestrator response delay, runnable-to-start delay, snapshot age/latency, resource saturation, repeated-message rate and model usage where available.
Also record completion throughput, behavior correctness, review defects, integration rework, human decision/review time and post-merge failures where observed.
Report unavailable measurements explicitly and do not simulate human merges to claim live evidence.
Use the small-concurrency baseline to evaluate whether increased concurrency improves useful velocity and where resource limits should be set.

MX Viz acceptance targets are cached API p95 below 250 ms, healthy 20-task refresh p95 below 2 seconds, and the initial 20-task overview interactive within 2 seconds after data arrives on a documented reference machine.
Record machine, workload, backend/harness versions, cache state, sample count and measurement procedure with the results.
Exercise multiple tabs and a stalled provider alongside healthy reports; demonstrate continued healthy progress and no independent backend probe loop per viewer.
These are accepted test targets, not claims of current performance.
If a target or required trial fails or cannot run, keep the affected acceptance item open and record the limitation rather than silently lowering the requirement or marking release complete.

### A9: Launch anywhere, project discovery and one shared chat

Launch `multplx` from any directory, including a repository, its subdirectory, a dev folder, the home directory or an unrelated directory.
The caller's directory is optional request context; it is not an installation location, required discovery root or operational home.
A dev folder containing immediate and nested projects is one supported example, not the mandatory entry point.
One configured operational home and its orchestrator conversation coordinate tasks across every selected repository.
Opening another project, changing directory, filtering the TUI or launching a second terminal must not create another main orchestrator or silently change the root home.
An explicitly delegated A11 sub-orchestrator has its own child home under that root; project selection alone does not create one.
Keep the main no-coding boundary and existing free sub-agent delegation intact.

#### Discovery and local project reuse

Persist configurable discovery roots, recursive traversal/depth settings, exclusions and symlink policy in private filesystem configuration.
Discovery roots are optional; first use may configure them once, but the user can enter chat and specify a project path or URL without a discovery setup ceremony.
Do not automatically register or recursively scan an arbitrary launch directory, especially the home directory or filesystem root.
Subsequent launches reuse configured roots regardless of the current directory.
Support additional roots and explicit paths outside them without requiring every project to move into one managed directory.
Use bounded, incremental discovery with cached results, visible scan progress and explicit refresh so large folder trees do not block entering the existing chat or viewing known projects.
Skip Git internals and configured dependency/build/cache directories; do not scan the entire home or follow cycles by default.
Discover direct and nested Git roots, linked worktrees and configured nested repositories/submodules using Git identity, not only a `.git` directory-name test.
Represent parent/child repository boundaries explicitly; selecting a parent folder does not silently authorize tasks in every nested repository.
Discovery inspects metadata without executing project scripts, loading every project's agent instructions, changing checkouts or registering every result as accepted work.
Unversioned project folders remain discoverable with their actual status; selecting one does not silently initialize Git, create a remote or claim worktree support.

Selecting an existing repository for work records its location idempotently; no clone, move or repeated URL entry is required.
Store stable project identity, aliases/display name, validated local checkout locations, repository/worktree identity, optional remote identities and managed-versus-user-owned status in the filesystem registry.
Resolve an explicit path/project selector before contextual suggestions; use exact aliases or validated known paths automatically and show candidate paths for ambiguous names.
Outside a repository, show the workspace overview instead of silently targeting the last-used project.
Inside a repository, offer its project as the next-request context while all other tasks remain active in the same chat.
Do not conflate distinct clones merely because their remote URL or basename matches.
Handle overlapping roots and symlink aliases without duplicate discovery entries; preserve distinct checkouts and shared Git identity where applicable.
Revalidate moved, missing or replaced paths before dispatch; retain task history and offer a location repair rather than silently cloning or binding to an unrelated directory.
Use URLs for intentional cloning when no selected local checkout supplies the task, not as a prerequisite for using an existing checkout.

#### Concurrent intake and repository ownership

One message such as "feature A in repo 1, feature B in repo 2, feature C in repo 3" creates separately tracked work in that same orchestrator conversation as standard delegation behavior.
This behavior does not depend on launching from a shared parent folder, using the TUI, or registering discovery roots.
Bind each task and queued request to stable project identity, selected checkout, explicit starting revision, scope, dependencies and its own artifacts/workflow/delivery record.
Bind those facts before dispatch and preserve them through retries, restart, selected-project changes and nested delegation.
Keep per-project instructions and evidence scoped to the corresponding task; do not merge all repository instructions into the main conversation.
Use parent request correlation and per-task operation identities so a retried multi-task submission does not duplicate already accepted tasks.
Report partial intake clearly: one ambiguous repository or blocked task does not prevent the unambiguous independent tasks from proceeding.
Cross-repository dependencies may be recorded explicitly without coupling unrelated task states or requiring separate chats.

Reuse user-owned checkouts as project sources while placing implementation in isolated sub-agent worktrees through the built-in manager specified by A10.
Keep user-owned branches, index and working files intact during discovery, refresh, registration and cleanup; adapt managed-clone synchronization and retirement accordingly.
Coordinate shared Git metadata mutations and record the starting commit rather than relying on whichever branch is later selected in the user's checkout.
Surface how uncommitted work is treated before implementation: use a named clean revision or explicitly requested captured working changes without silently stashing, committing, resetting or discarding the user's files.
Removing a remembered project unregisters it; deletion of user-owned files is a separate explicit operation.
Remote-free repositories remain usable for local tasks without inventing a forge or forcing a URL.

#### Installation, connection and the TUI

Provide a global installation with verified platform binaries and matching runtime assets, instructions and adapters in a managed application location, separate from persistent state.
Ordinary users should not need a source checkout, a Rust build or a change into the Multplx directory to start work.
Retain source installation and an explicit activation shell for development and advanced usage.
Detect installed supported harnesses and remember the user's selection; preserve actual authentication, trust and backend prerequisites rather than pretending a download supplies them.
Capture launch directory/project intent before changing to any trusted runtime context needed by a harness.

Bare `multplx` opens the terminal workspace entry with a direct path to the one orchestrator conversation and remembered projects/tasks.
Provide equivalent noninteractive CLI entry points for explicit path/alias selection, project listing/configuration and task submission; shell activation remains explicit.
The intended command examples below express accepted behavior; publish exact implemented grammar in command help and preserve documented legacy harness launch forms.

| Entry example | Required experience |
| --- | --- |
| `multplx` from any directory | Open the configured workspace and its existing orchestrator; launch location does not restrict which repositories can receive tasks. |
| `cd ~/dev` then `multplx` | Open the configured workspace, discover nested projects incrementally, and enter the same multi-repository orchestrator chat. |
| `multplx ~/dev/group/my-app` | Remember/use that existing checkout as next-request context without cloning or starting another orchestrator. |
| `multplx my-app` | Resolve a remembered alias, showing paths only if identity is ambiguous. |
| `multplx projects` | Search known/discovered projects and configure roots, exclusions and refresh through the same registry owner. |
| `multplx task --project my-app "Fix login"` | Persist a correlated request to the existing orchestrator and return its receipt without directly bypassing orchestration. |
| `multplx shell` | Open the explicit legacy-style activation shell. |

Connect to an existing live owner through its recorded endpoint where the provider supports attachment; otherwise route requests durably and show the supported route to that conversation.
Do not steal a live lock or start a second writer because a terminal cannot attach.
When no owner is live, reconcile the previous session and start or resume one orchestrator under the home lock with compact state recovery.
Concurrent launch races must converge on one owner; signal handling and closing the TUI must not cancel independently running tasks.
State recovery and continuation must work without claiming a provider can restore an unavailable transcript or endpoint.

The TUI supplies searchable projects, recent/active tasks across repositories, pending decisions, current context, connection state and entry into the harness conversation, plus an MX Viz shortcut.
It is a thin client, not a second chat engine, model session, workflow engine or authoritative task store.
Use the same canonical projection as CLI status and MX Viz; forward mutations and task/decision requests through existing validated owners and the durable inbox.
Keyboard use, resize, empty/loading/error states, incremental discovery and stable selection must remain usable while work proceeds.
Keep MX Viz read-only; the TUI does not add browser mutation endpoints or a merge path.
Configuring discovery, registering a chosen local path and submitting normal tasks must not revive mode/yolo interviews or engineering approval ceremonies.

### A10: Built-in Git worktree lifecycle

Remove Treehouse as an external dependency, installer, automatic download, shell command and required configuration surface.
Implement the required lifecycle inside the existing Rust workspace and `mx` CLI using Git, the existing filesystem/lock primitives and canonical task ownership.
Keep runtime session backends separate from worktree management; tmux, Herdr and cmux receive the same validated allocation result within their currently supported combinations.
Do not vendor Treehouse or reproduce its standalone product, updater, interactive shell lifecycle, arbitrary hook engine, prewarmed pools, Jujutsu support or broad process-killing behavior.
[Treehouse replacement research](plans/lean_redesign/treehouse-replacement-assessment.md) records the pinned upstream behavior and actual Multplx call sites; it is rationale, not another schema owner.

#### Small operation surface

Provide acquire, inspect/list, release/retain and explicit scoped prune operations under the existing CLI.
An acquisition resolves a project and recorded starting commit, reserves an isolated path, creates or safely reuses a clean idle managed worktree, and returns the exact path plus allocation identity.
Keep reuse conservative and simple: only verified idle, unleased, fully disposed work can be reused; otherwise allocate a new path or queue for capacity.
Do not add heuristic eviction or prewarming to make reuse mandatory.
Preserve verified reusable caches where safe, and measure checkout/setup time; do not automatically erase ignored data or treat unknown ignored content as disposable cache.
If cache/content safety cannot be established, retain that slot and use a fresh allocation rather than importing unknown state into the next task.

Use explicit task-bound base commits from A9; an acquire retry must not silently fetch and select a different latest branch.
Default detached worktree creation and subsequent task-branch creation may preserve the existing launch behavior, while existing branches, collisions and resumes are reconciled explicitly.
Never reset the user's primary checkout, infer repository identity from a basename, or allocate from the Multplx runtime just because the orchestrator runs there.
Support local repositories without remotes and inspect linked-worktree/common-Git identity correctly.
An unborn repository, unavailable base, locked resource or unsupported Git operation yields a specific retained/queued/error result rather than silently initializing or destroying data.

All allocations have durable ownership, including ordinary task worktrees; persistent-home reservations remain valid with zero live processes.
Return machine-readable allocation, project/checkout, path, base commit, owner/attempt, persistence and per-acquisition lease identity through a typed API or JSON, with diagnostics separate from machine output.
Use stable request IDs to reconcile retries, and a fresh allocation/lease generation when a path is acquired again.
Release checks the exact recorded allocation and generation; a stale caller cannot release a newer allocation of the same path.
Path-only unconditional release is not the internal cleanup contract.

#### Launch, lifetime and cleanup

Acquire the worktree before launching the agent endpoint, then start the harness at that exact path and publish the allocation-to-task binding transactionally.
Remove `treehouse get` injection and the sixty-second cwd-polling discovery loop from all session adapters.
Verify the actual Git top level and shared repository identity before starting project work; terminal observation is no longer the allocation authority.
Reuse the operation receipts from A1/A3 to recover acquire-before-record, endpoint-before-record, partial home seeding and launch-failure rollback.
For packaged runtime assets that are not a Git checkout, provision a persistent sub-agent home as a private directory using existing home-seed/configuration semantics; do not fabricate a Git repository merely to imitate an old Treehouse home lease.
Task worktrees still come from their assigned project repository, and a deliberately Git-backed persistent home uses the same durable allocation API.

Ending a shell, losing an endpoint or receiving a done report does not prove the worktree is disposable.
Reconcile recorded processes and stop only identity-verified task-owned endpoints/processes through existing lifecycle helpers before release.
Unknown occupants or uncertain liveness retain the worktree; do not kill arbitrary processes based solely on their cwd.
Separate stopping execution, retaining unfinished work, releasing ownership and deleting/reusing files as explicit recorded outcomes.
Preserve dirty/untracked/unknown ignored files, unpushed or unlanded commits, open-PR work and persistent homes until the existing truthful delivery/recovery contract establishes safe disposition.
Reuse the existing landing evidence, including supported squash-merge checks, without turning PR publication into proof of landing or granting agents merge authority.
Use Git-supported worktree removal and exact managed targets; no blanket `rm -rf`, forced reset/clean or indiscriminate pruning of all worktrees in a shared repository.
Prune is explicit, scoped and previewable, and excludes leased, retained, unknown and caller-owned paths.

#### State and migration

Use one filesystem allocation owner integrated with task/session records; list views and MX Viz consume its canonical projection.
Serialize reservations and lifecycle transitions by repository/allocation, maintain a consistent lock order and avoid home-wide locks across Git commands.
Where a Git operation requires exclusion, use a bounded repository operation reservation and revalidate the generation before committing its result.
Recover interrupted Git mutations by reconciling receipts with Git's worktree inventory; corrupt or missing allocation metadata never makes a found worktree free for reuse.
Keep stale-lock proof and PID-start identity checks where applicable instead of removing Git locks by age alone.

Migration inventories only recorded Multplx-owned Treehouse paths and leases, using fixtures and explicit source metadata, not a global takeover of the user's Treehouse pools.
The pinned v2.0.1 lease records lack the later per-acquisition lease identity; preserve that uncertainty and issue a new internal identity only after validating ownership.
Quiesce old agent endpoints and Treehouse wrapper shells before transfer so their exit cleanup cannot reset transferred work.
Do not adopt a path in place while the external pool can still reallocate or reset it.
Where supported, move a verified owned worktree out of the old pool using Git, with recoverable updates to all home/task/path references and retained old-to-new mapping.
If a move or ownership proof is unsupported, preserve the original and its evidence as retained legacy work pending a supported continuation; never silently delete it or declare cutover complete.
Do not mutate unrelated Treehouse state, uninstall the user's global Treehouse binary or require Treehouse to run the new normal lifecycle.
Retire installer/probe/config/CI dependencies and replace fake-Treehouse success tests with real temporary Git worktrees plus focused fault injection.
Historical references and versioned migration fixtures may retain the old name; new runtime/help/brief paths must describe the built-in operations.

### A11: Scoped sub-orchestrators

Add a sub-orchestrator assignment and spawn command for one bounded project, repository or idea under the main orchestrator.
This is a coordination responsibility in the common sub-agent model, not a new privilege class, state engine or mandatory delegation gate.
The main orchestrator remains the user's normal conversation and owns cross-domain priorities and human decisions.
A sub-orchestrator handles its domain's research, briefs, delegation, integration planning, progress and escalation; it delegates project implementation and test-code changes just as the main orchestrator does.
It can freely launch researchers, implementers and reviewers within its accepted scope, including native delegation where available.
Simple work can still go directly to an implementer; never create a coordinator per repository merely because that repository is selected.
[Secondmate research and assessment](plans/lean_redesign/sub-orchestrator-assessment.md) records the upstream lessons and current Multplx gaps; Phase 05 owns the integration below.

#### Spawn and domain identity

Expose the planned public command `mx spawn <id> --sub-orchestrator --project <project-id> --scope <text>` through the existing spawn owner.
Support repeatable `--project` for an explicitly bounded idea spanning repositories, or `--idea <idea-id>` for research before a repository exists; require an explicit domain instead of silently choosing the runtime repository.
Support `--request-id <id>` for repeat-safe creation and `--persistent` for a standing domain assignment, alongside the common harness/model/effort options.
These are target CLI forms to implement and test, not commands the current binary already supports.
The CLI provisions the private home, concise charter, parent route and session in one recoverable operation; it must not require hand-editing a registry or manually copying projects before spawn.
Ordinary task-scoped coordinators may finish their assignment; persistent coordinators remain registered and idle between assigned work without inventing audits, surveys or new tasks.
Idle runtime sessions may yield capacity through a checkpoint/restart operation that preserves the domain, child state and pending messages.
Persistence is independent of the role, and restart never deletes a charter or claims that live children finished.

Represent stable domain, root-home, coordinator, immediate-parent, owner-home, assignment-generation and scope-revision identities using A2 records.
Keep a concise charter with outcome, scope, project references, constraints and evidence pointers, not another generic engineering playbook.
An idea can begin without a Git repository; explicitly bind selected projects before implementation without resetting existing tasks' identities or starting revisions.
Multiple coordinators may share a project when their responsibilities differ; a project list is not an exclusive repository lock.
Assign each task to one current owning coordinator; task identity and lineage survive transfer, and parent references must form an acyclic graph.
Main-to-sub-orchestrator-to-workers is the normal shape; ordinary nested delegation stays available without a role approval, mandatory depth or newly privileged class.
Any additional coordinating layer uses the same recorded scope, lineage, capacity and return-channel contracts.
Cross-domain work is routed or negotiated through the owning coordinators; do not silently expand a scope or let two coordinators issue conflicting instructions for the same task.

#### Parent channel and durable outcomes

Compose A2/A3 envelopes, inbox claims, correlation and receipts at each parent-child boundary; do not build a parallel messaging system.
Persist a reply expectation before sending a request, bind it to the recipient generation and scope/task revision, and distinguish delivered, acknowledged, answered and completed.
The report operation resolves its destination from validated parent identity; callers do not select arbitrary status-file paths or depend on a pane label or basename.
Keep lifecycle control separate from conversational message framing, with verified endpoint identity and explicit observed postconditions.

Runtime writers publish accepted task outcomes to a durable parent outbox at the canonical state transition, or reconcile that publication through a recoverable receipt.
This covers recorded research/report availability, failures, implementation/PR readiness, declared human decisions, human-merge observations and final disposition.
Forward their original task, attempt, revision and event identities through the ancestor chain into the root projection, with per-hop receipts and deduplicated presentation.
Facts reach the root even while an intermediate model is idle or fails to summarize; the existing supervision path relays them without a new service or mandatory model turn per event.
Do not claim a reasoning-only result exists until the agent records it through the result/report interface.
The sub-orchestrator adds concise judgment, options and domain summaries pointing to those facts; summaries do not replace task truth or completion evidence.
Missing correlated replies receive bounded recovery and then a durable escalation, not repeated prompt injection, silent expiry or a false completion inferred from transport success.
Unanswered questions remain visible even when newer progress arrives; route human answers back to the exact question, task and revision, preserving superseded answers as history.
Peer communication stays available, but accepted scope changes and task outcomes return to the owning task and parent chain.

#### Ownership, recovery and capacity

Each home has one active coordinator/writer owner and its own inbox consumption boundary; the root reads projections and issues routed commands instead of draining child queues or editing child records directly.
Use canonical project references and the A10 worktree owner for worker allocations across homes; a sub-orchestrator home itself can be a private non-Git directory in a packaged installation.
Do not clone all project repositories or add a second installation just to create the coordinator.
Reuse A1 transactions for home creation, assignment acceptance and transfer, parent outbox delivery, relaunch and retirement.
A transfer fences the prior assignment generation, reconciles old execution and mutable ownership, then activates the new owner without replaying completed external work.
Keep canonical state at its recorded owner through transfer or migrate it transactionally; never leave two authoritative backlog/task copies.
Late messages remain historical, and independently running children are reconciled after parent restart rather than launched twice.
If the parent is unavailable, accepted independent work may continue within scope and capacity; queue replies and decision requests durably and wait only for dependencies actually requiring the parent or human.
Expose both endpoint liveness and inbox/outbox progress so an alive but non-handling coordinator is diagnosable through bounded A4 observation without consuming its queue.
Stopping a coordinator is distinct from stopping its subtree and from retiring its home; commands must state which operation is intended and report uncertain cancellation honestly.
Retirement requires all child ownership and pending outcomes to be delivered or durably transferred, with A10 retention of unfinished work; a final summary or an empty parent queue is insufficient.

Use A5 capacity accounting across the whole root workspace, including coordinators and known descendants, so each home cannot spend the same session/build allowance independently.
Reserve practical headroom for workers and parent responsiveness, queue accepted work fairly across domains and avoid coordinators occupying every available worker slot while waiting for children.
Keep the shared admission record small and short-locked; it is resource accounting, not a second task scheduler or home-wide lock held across model turns.
Reconcile admissions after crashes using execution identity; unknown native-provider usage is visible rather than invented as zero or claimed to be strictly controlled.
Do not impose an arbitrary small agent-count gate or require human approval for routine domain delegation.

#### Presentation, migration and evaluation

CLI status, MX Viz and the TUI use the shared A4 projection to show the root, domains, coordinators, child tasks, current owners, human questions, undelivered outcomes and observation age.
Offer a compact domain overview and expandable task lineage; count useful tasks separately from coordinator sessions and attempts.
Show partial/unavailable domains explicitly, preserve filters and keyboard focus, and keep the single main chat as the normal human entry point.
Task/coordinator inspection may open a detail view without creating another main orchestrator or retargeting work.

Migrate legacy daemon homes by recorded responsibility: a coordinating charter maps to a sub-orchestrator assignment; persistence alone does not prove that role.
Preserve domain routes, children, parent correlation, charters, leases and inherited configuration; ambiguous scope or parentage remains recorded for reconciliation rather than guessed.
Do not import upstream Firstmate code, Treehouse dependencies, remote SSH placement, extra runtime backends or its engineering approval policies as part of this feature.
Use existing supported local persistent-home/provider combinations and document native-provider recovery limits honestly.
Compare flat and domain-delegated 1/5/10/20-task workloads at equal total resource budgets, including several coordinators over three repositories and overlapping responsibilities in one repo.
Measure root response delay, per-hop and end-to-end outcome latency, queue age, coordinator/model cost, useful completion throughput and quality/rework; retain all A8 acceptance targets.
Hierarchy is expected to reduce root context load for sustained domain work, but an extra model hop can slow small tasks; publish measurements before claiming a scalability improvement.

| Requirement | Primary implementation owner | Integration and acceptance |
| --- | --- | --- |
| A1 filesystem transitions | 02 schema and reusable receipt primitives; 04 action receipts | 06 publication; 09 migration and interruption recovery; 12 release |
| A2 attempt and brief identity | 02 records and validation | 04 reports; 06 evidence; 08 decisions/workflows; 09 legacy reads |
| A3 durable handling | 04 queue, inbox, correlated replies and retry reconciliation | 06 forge reconciliation; 09 restart; 12 failure injection |
| A4 bounded observation | 04 watcher/check isolation; 10 snapshot and service collection | 02 typed projection; 09 compact startup; 12 stalled-provider trial |
| A5 capacity and dependencies | 04 deferred work and resource admission; 08 dependency execution | 02 dependency schema; 10 visibility; 12 comparative workloads |
| A6 human communication and quality | 06 revision-bound delivery evidence; 08 research, decisions and handoffs | 01 concise prompts; 07 optional-tool integration; 10 human queue |
| A7 MX Viz | 10 shared read model, visual design and browser behavior | 12 packaged UI and scale evidence |
| A8 measurement | Each phase's focused acceptance; 10 dashboard measurements | 12 combined 1/5/10/20-task and failure trials |
| A9 workspace entry | 02 project/checkout schema; 11 installation, discovery, connection and TUI | 04 request routing; 06 checkout/delivery ownership; 08 multi-repo intake; 09 migration; 10 shared visibility; 12 release |
| A10 built-in worktrees | 03 lifecycle, caller replacement and dependency retirement | 02 identity; 04 delegation; 06 delivery; 09 legacy migration; 10 snapshots; 11 TUI/package; 12 release |
| A11 scoped sub-orchestrators | 05 domain spawn, parent outcomes, recovery and shared capacity | 01 instructions; 02 identity; 03 homes/worktrees; 04 messaging primitives; 06-08 task integration; 09 migration; 10-11 views; 12 trials |

## Read-through findings and implementation map

The runtime is already Rust: one Cargo workspace, six crates, and the `mx` multicall binary.
Most `bin/` files are transport adapters, so changing their prose alone will not change the product.
The restrictions have several independent enforcement points.

| Surface inspected | Current coupling | Owning phase |
| --- | --- | --- |
| `AGENTS_E.md`, `CLAUDE.md`, `.agents/skills/`, generated briefs | No direct project work, fixed escalation recipes, mandatory skill loads, role-specific definitions of done | 01 |
| `crates/multplx-domain/src/project_registry.rs` | Mode-only lookup assumes flat managed clones; add stable project/checkout identity and external local paths while removing review defaults | 02, 07, 11 |
| `crates/multplx-domain/src/lifecycle/brief.rs` | Delivery/scout/daemon templates prohibit pushes and lifecycle calls and prescribe review and project-memory procedures | 01, 02 |
| `crates/multplx-domain/src/lifecycle/spawn.rs`, `crates/multplx-cli/src/lib.rs` | Kind/mode/yolo metadata, daemon-specific launch branches, credential stripping and non-writable origin push URL | 02, 04, 06 |
| `crates/multplx-domain/src/supervision.rs`, `.claude/settings.json`, `.cursor/hooks.json`, `.cursor/rules/multplx.mdc` | Native delegation denial and generated operating instructions | 04 |
| `.codex/hooks.json`, `.pi/extensions/`, `docs/supervision-protocols/` | Harness-specific startup and watcher continuity | 04, 08 |
| `crates/multplx-domain/src/review_delivery.rs`, `crates/multplx-cli/src/review.rs` | Agent-session delivery refusal, approved-SHA handoffs, PR registration, polling, merge and local landing | 06 |
| `crates/multplx-cli/src/deep_review.rs`, `.deep-review.yaml`, services and `share/vplan/` | Optional tools with dependencies that currently reach default behavior | 07 |
| `crates/multplx-domain/src/workflow.rs`, `crates/multplx-cli/src/workflow_runtime.rs`, `workflows/` | Broker/actor executors, headless-stage coupling to deep-review, fixed stage contracts and approval points | 08 |
| `crates/multplx-domain/src/maintainer_override.rs`, `decision_hold.rs`, `crates/multplx-cli/src/authority.rs` | General exception machinery and report-completion attestations mixed with useful durable decisions | 01, 08, 09 |
| `crates/multplx-core/src/`, backend adapters, lifecycle recovery | Reusable coordination infrastructure and identity-bound cleanup | 04, 09 |
| `crates/multplx-domain/src/inheritance.rs`, `handoff.rs`, home seeding and system sync | Persistent-home provisioning, inherited settings and correlated parent replies | 02, 05, 09 |
| Snapshot readers, `share/viz/`, docs, tests and CI | Roles and policy reappear in presentation, health checks, startup and regression expectations | 09, 10, 12 |
| `crates/multplx-cli/src/launcher.rs`, `crates/multplx-backend/src/harness_launch.rs`, `share/shell/` | Shell-first entry ignores caller project, requires a runtime checkout and refuses a live owner instead of reconnecting | 11 |
| `crates/multplx-cli/src/lib.rs`, home seed/teardown, probes, installer and CI | Treehouse acquisition, cwd polling, durable-home leases, destructive return and dependency checks require coordinated replacement | 03, 09, 12 |
| Project registration, `system_sync.rs`, home seeding and retirement | Flat clone discovery and managed checkout maintenance do not model borrowed local repositories | 02, 06, 09, 11 |

Preserve the Rust architecture, existing backend transports and static browser assets where their behavior still fits.
Replace the Treehouse worktree provider with A10's built-in Git lifecycle.
This port does not add a session backend or turn Codex Desktop host tools into a shell-callable backend.
Inspect connected callers and tests fully when implementing each source slice; this map identifies the redesign boundaries, not a substitute for that implementation read.

## Small operating contract

The released contract should explain only the following everyday facts.

- You are the main orchestrator; own the human conversation and task coordination, and delegate project implementation to sub-agents.
- Research and synthesis may happen in the orchestrator; use a researcher sub-agent when uncertainty or workload warrants it.
- Implementation sub-agents may commit, push branches, open and update PRs; the human performs PR merges.
- Selected workflow stages and explicit task constraints remain binding.
- Deep-review and vplan require an explicit user request; ordinary testing and discretionary sub-agent review remain available.
- Use the CLI for Multplx-owned state and preserve the status, ownership, locking, and recovery contracts.
- Reconcile existing work at startup and retain unlanded work during recovery and cleanup.
- Locate command mechanics through help and a small optional operational skill index.

Remove prescribed salutations, escalation templates, fixed retry counts, forced investigative methods and mandatory independent reviewers.
Retain the main orchestrator's no-coding boundary without preventing its research, planning or coordination-state updates.
Do not copy these removed procedures into generated prompts, CONTRIBUTING, or a renamed umbrella skill.
Project-specific build and test facts remain useful contributor documentation.
Repository conventions should help someone change this repository, not dictate how every managed project is engineered.
Measure the actual root-contract and ordinary-brief size before and after; aim for a short root contract of roughly 100 lines or less, without adding a new token-budget enforcement subsystem.

## Skill disposition

This table is the complete disposition of the 17 current internal skill directories.
Keep operational mechanics only where they offer value beyond command help.
Consolidation names below describe responsibilities, not a commitment to add one skill per row.

| Current internal skill | Disposition | What survives |
| --- | --- | --- |
| `multplx-coding-guidelines` | Remove as an agent runtime skill | Brief repository conventions and validation commands in contributor context; existing documentation ownership tooling can remain. |
| `diagnostic-reasoning` | Remove | No replacement mandatory diagnosis methodology. |
| `ask-user-authority` | Remove | Ordinary task scope and real missing-input clarification, without reviewer-specific approval ranks. |
| `maintainer-override` | Remove as the universal procedure | Only mechanics still required by deliberately retained commands; retire records through Phase 09. |
| `decision-hold-lifecycle` | Remove mandatory completion procedure | Durable explicit user decisions and workflow interactions, without mandatory `complete --none` attestations. |
| `harness-adapters` | Keep and shorten | Actual launch, send, interrupt, resume, reporting and supervision mechanics; remove effort-selection policy and native-delegation prohibitions. |
| `bootstrap-diagnostics` | Consolidate with recovery reference | Interpret CLI diagnostics and target the owning repair command. |
| `stuck-actor-recovery` | Consolidate and rename for sub-agents | Recover the recorded endpoint and worktree without losing work or duplicating ownership. |
| `daemon-provisioning` | Consolidate into persistent sub-agent operations | Home leases, transactional provisioning, inherited config, routing and safe retirement. |
| `project-management` | Keep a short CLI reference | Project lookup, registration, cloning and removal mechanics; remove mode/yolo interviews and policy escalation recipes. |
| `create-workflow` | Keep and shorten | Schema, validation, stage inputs/outputs and preview; infer provided requirements instead of requiring an interview. |
| `afk` | Keep and shorten | Explicit away-mode commands and supervision ownership; move extensive implementation details to their code/docs owners. |
| `catchup` | Keep and shorten | Canonical bounded state retrieval and optional saved report; remove mandatory response sections and wording. |
| `recap` | Keep as optional convenience | Summarize visible work when requested, without a fixed response script. |
| `stow` | Keep as optional convenience | Useful persistence locations and inspect-before-update mechanics; remove forced worker delivery of memory changes. |
| `updatemultplx` | Keep and shorten | Safe fast-forward update and instruction refresh for registered homes. |
| `multplx-codexapp` | Keep only as optional integration reference | Verified host-tool operations and honest transport limitations; no invented CLI backend. |

The separate public `skills/stow/SKILL.md` is not an internal role skill.
Keep its standalone behavior unless a concrete stale link or naming change requires an edit.
Keep `.claude/skills` discovery working and remove references to deleted skills from briefs, hooks, `.deep-review.yaml`, contributor docs, inventory and instruction-owner tests.
A retained skill loads for the operation being performed, not for generic acts such as coding, reviewing, finding a bug, or answering a finding.

## Sub-agent model and communication

Keep task identity separate from agent identity.
A task represents requested work, its artifacts, workflow association and delivery state.
A sub-agent session represents an execution attempt with a parent, home, runtime endpoint, and lifecycle.
Reuse existing metadata where possible; do not add another task database or make the journal authoritative.

Normalize legacy kinds into a common sub-agent representation.
`delivery` and `scout` describe old task outputs, while `daemon` describes persistence and an isolated home.
They must no longer determine whether a sub-agent may delegate or publish a branch within its assignment.
Record researcher/implementer/reviewer as optional role metadata, separate from session lifecycle and persistence.
A report-only assignment stays report-only because that is the assigned outcome, not because its agent has a lesser role.
A sub-agent may create further sub-agents in its assigned scope; ownership and ancestry must prevent cycles and duplicate supervisors, not require parent permission for every launch.

Continue using `MX_HOME`, task-bound reports, validated status vocabulary, canonical current-state reads and the durable wake queue.
Keep the operational-input prefix and decode old message carriers, including `from-broker`, during the migration.
Use new orchestrator/parent terminology for newly authored messages and keep correlation IDs stable across retries.
The marker distinguishes internal messages from user input; it does not grant authority or make message bodies trusted instructions.

Native sub-agents must be usable without an escape flag or a hidden local deny list.
Reuse the existing metadata/status ownership boundary to record their parent, provider/session identity and observable lifetime when an adapter exposes that information.
When a native provider has no exportable child lifecycle, record the parent task as containing session-bound delegation and keep recovery at the parent.
Do not fabricate per-child visibility or call a native child restart-surviving when its provider cannot resume it.
An unavailable optional tracking integration does not prohibit native delegation.
On parent restart, reconcile supported children; mark unrecoverable session-bound work interrupted and preserve its artifacts for continuation.

The normal Multplx CLI path remains available when independent runtime sessions and durable recovery are useful.
Do not force all native work through another wrapper just to restore the old restriction under a different name.
Locks serialize shared mutations and designate queue ownership; they do not turn sub-agents into read-only processes.
A child should not acquire a second primary session lock for its parent's home merely to send a report or spawn a child.

## Delivery and merge boundary

Agents use the configured Git and forge authentication normally for authorized branch pushes and PR operations.
Remove forced empty credential helpers, blocked `origin.pushurl`, SSH-agent suppression and blanket agent-session refusal from the ordinary delivery path.
Do not print credentials or persist them in a brief, task record, terminal launch string, or generated artifact.
Preserve credential-source precedence and avoid replacing existing user authentication configuration globally.

Normal delivery is implement, verify appropriately, commit, push the task branch, open or update the PR, record its URL and report the result.
A convenience delivery CLI may retain identity checks, explicit object/ref selection, retry reconciliation and truthful receipts.
It must not require a pending approval record, deep-review receipt, waiver, or separate credentialed scheduler.
Direct Git/forge operations remain valid; register their PR with the existing PR-state owner so monitoring and cleanup can reconcile them.
No project mode may quietly route work back into the old mandatory service.

Separate publication facts from optional review evidence.
An unrun deep-review is neither a failure nor a waived/passed gate.
If the user requested the tool, retain its actual result and do not claim successful completion when it failed.
A pushed branch or an open PR is not a merged PR, and stopped sessions do not make their unlanded work disposable.

The PR merge restriction applies to all agents, including nested sub-agents, persistent sessions, workflow command stages and optional review helpers.
Remove standing merge authority, `yolo` merge paths, agent-consumable merge-red overrides, automatic merge scheduling and instructions that ask another agent to merge.
Keep a human-shell merge helper if useful and keep read-only merge polling.
A user request to merge gets a ready-to-use PR link or human-shell command; it does not reactivate a product agent-merge mode.
Do not implement landing by directly pushing the PR contents to the protected target branch instead.
Local rebases, integrating branches within a task, and explicitly requested local-only work are not remote PR merges.
Do not invent a new global approval layer for those actions.

A merge command check is an operational backstop, not a security sandbox against an agent that can execute arbitrary code with broad credentials.
Do not claim ordinary forge write credentials intrinsically separate PR creation from merging.
Where the operator requires independently enforced separation, use existing remote protection and identity controls and verify their actual behavior during release validation.
Do not make a new credential broker or remote-policy service a prerequisite for ordinary use.

## Optional tools and workflows

Deep-review and vplan stay available as explicitly selected tools.
Do not delete their implementation solely because they stop being defaults.
Remove automatic invocation from task intake, project defaults, generated briefs, memory maintenance, upstream-sync and generic workflow templates.
An HTML plan request does not by itself request a vplan server or a review-confirmation ceremony.
Missing or broken optional assets must not block unrelated launch, project work, delegation, delivery or health checks.
Validate their dependencies when the tool is requested or an active recorded run needs recovery.

Ordinary checks required by the target repository and tests the agent judges useful remain normal work.
Freely delegating a review question is allowed; using the named deep-review pipeline is opt-in.
Extract any generic headless agent invocation used by workflow execution from the deep-review policy path so ordinary workflows do not implicitly run the gate or inherit its restrictions.

Keep immutable workflow snapshots, per-run locks, stage ordering, output validation and restart reconciliation.
Replace legacy role-based executors with execution choices such as current orchestrator context or a sub-agent session.
The orchestrator context can host discussion, research, synthesis and planning; implementation stages always use sub-agents.
These are placement choices, not permission classes.
A stage may delegate as needed, but later stages cannot pass before its declared contract is met.
Preserve an explicitly chosen `fresh_session` requirement.

New general-purpose workflow examples must not add review tools, approval stages or credentialed delivery merely because the old template had them.
A user-defined interaction or approval stage remains binding once the workflow is selected.
Selecting a clearly disclosed workflow that includes deep-review or vplan counts as requesting that tool for that run.
An old opaque/default configuration is not evidence of that request.
For a legacy workflow containing one of those tools, surface its planned stages before a new run; clarify only when the user's selection did not establish that intent.
Do not silently remove the stage or secretly run the tool.
Do not auto-resume legacy gated runs during migration; retain their immutable evidence and establish which continuation the user wants when required.

## Compatibility and migration

Prefer new canonical terminology with a small, explicitly bounded compatibility reader over a blind search-and-replace of live state.
Phase 02 owns the schema mapping and Phase 09 owns the executable migration.
Do not renumber tasks, regenerate message correlation IDs, lose worktree leases or rewrite journals to make names look current.
A10 permits explicit, recoverable relocation of verified legacy worktrees to remove external-pool ownership; preserve old-to-new path mappings and update every recorded reference through its owner.

| Legacy surface | Migration treatment |
| --- | --- |
| Flat `data/projects.md` registry and managed `projects/<name>` paths | Assign stable project/checkout identity, preserve existing clone ownership and task bindings; do not move user repositories or infer ownership from a new discovery root. |
| `kind=delivery`, `kind=scout`, `kind=daemon` | Read into common sub-agent identity plus output/persistence facts; preserve raw originals in migration evidence. |
| `mode=deep-review`, `direct-PR`, `local-only` | Separate destination from explicitly requested review; legacy defaults do not silently opt in new work. |
| `yolo`, `+yolo`, merge exceptions | Read for compatibility only; never restore agent merge authority. |
| `config/actor-harness`, `actor-dispatch.json`, `daemon-harness` | Map to sub-agent defaults/profiles and persistence-specific overrides without losing model/effort selections. |
| `data/daemons.md`, `.mx-daemon-home`, inherited settings | Preserve home ownership, routes, leases and pinned settings as persistent sub-agent facts. |
| Old endpoint/container labels and command aliases | Resolve recorded identities during transition; new launches use canonical names. |
| `from-broker` carriers and pending replies | Accept existing carriers, preserve correlation and reply destinations, emit canonical new forms only to compatible peers. |
| `.ready-to-push`, `.delivered`, `.gate/`, decisions, overrides | Preserve evidence and distinguish review from publication; pending legacy records do not trigger a push, a merge, or fabricated approval. |
| `.workflow/` definitions and stage records | Preserve snapshots and completed-stage evidence; use compatibility decoding without silently rewriting the selected process. |
| `.wake-queue`, locks, `.status`, journals, PR polls | Preserve ordering, identities, bytes and trust bindings unless their owner implements an explicit versioned transition. |
| Treehouse task paths, persistent-home leases and pool metadata | Preserve owned work and historical lease facts; quiesce old wrappers and transfer only validated resources through A10, with no global pool takeover or dependency on a Treehouse executable. |
| Local harness deny lists and launcher credential overlays | Remove only entries demonstrably installed by Multplx; preserve unrelated user settings and report unresolved provenance. |

Design migration as an explicit inspect/apply operation under the relevant home lock, with a dry-run report, backups and repeat-safe progress records.
Do not start new agents, publish branches or delete work as a migration side effect.
Bring participating sessions to a safe boundary before switching writers; an unavailable home stays on its old format until it can be migrated.
Use mixed-version reads only for documented compatibility, not simultaneous incompatible writers.
Rollback requires stopping new writers and restoring the matching code/config/state backup; never boot the old binary against unrecognized new records.
Retire old public names after a documented compatibility window; historical evidence and compatibility fixtures may still use old tokens.

## Implementation sequence

| Phase | Outcome | Depends on |
| --- | --- | --- |
| [01](plans/lean_redesign/01-lean-contract-and-skills.html) | Minimal operating contract, short task briefs and operational skill set | Baseline |
| [02](plans/lean_redesign/02-unified-subagent-model.html) | Common model, attempt/brief identity and filesystem transaction primitives | 01 |
| [03](plans/lean_redesign/03-built-in-worktree-lifecycle.html) | Internal Git worktree lifecycle, ownership and Treehouse removal | 02 |
| [04](plans/lean_redesign/04-free-delegation-and-coordination.html) | Free delegation, durable inbox handling, bounded watcher checks and capacity | 02, 03 |
| [05](plans/lean_redesign/05-scoped-sub-orchestrators.html) | Domain coordinator spawn, reliable parent channels and shared capacity | 02, 03, 04 |
| [06](plans/lean_redesign/06-agent-delivery-human-merges.html) | Repeat-safe publication, revision-bound evidence and human-only PR merges | 02, 04, 05 |
| [07](plans/lean_redesign/07-opt-in-review-tools.html) | Deep-review and vplan fully optional, including startup dependencies | 01, 02, 03, 05 |
| [08](plans/lean_redesign/08-workflows-with-agent-freedom.html) | Ordered workflows with autonomous stage execution | 04, 05, 06, 07 |
| [09](plans/lean_redesign/09-state-migration-and-recovery.html) | Upgrade existing homes, aliases, messages and snapshots without losing work | 02-08 |
| [10](plans/lean_redesign/10-mx-viz-and-scale.html) | MX Viz task/role/workflow view, state freshness and scale validation | 02, 04, 08, 09 |
| [11](plans/lean_redesign/11-workspace-entry-and-project-discovery.html) | Global install, configurable nested discovery, one chat and terminal workspace entry | 02, 04, 09, 10 |
| [12](plans/lean_redesign/12-documentation-validation-cutover.html) | Consistent docs, behavior tests, harness evidence and release cutover | 01-11 |

This dependency graph permits useful parallel implementation where file ownership is coordinated.
It does not require serial coding or force a particular sub-agent count.
Do not activate a partial release in real homes where new instructions promise freedoms that the old runtime still denies.
Each implementation change should identify the old behavior being intentionally removed and the coordination guarantees being retained.
Every phase has an implementation evidence section for its change/revision references, actual checks, results and remaining work.
Keep that evidence concise and keep supporting transcripts in task or PR evidence.
No phase is complete while an assigned accepted requirement or required verification remains unresolved.

## Validation strategy

Use existing tests for retained guarantees and change policy assertions that intentionally conflict with the redesign.
Do not preserve obsolete behavior merely to keep old tests green, and do not remove whole safety suites because some assertions change.
The concrete per-phase test inventory is in the HTML plans.
The Rust test runner owns coverage and resource scheduling in `crates/multplx-cli/src/tooling/runner.rs`.

The final acceptance scenarios are:

1. A simple project change is assigned by the orchestrator to an implementer and ends at an open PR without unnecessary research, a Multplx skill interview, gate, waiver or delivery approval; the orchestrator does not edit project code.
2. The orchestrator and a sub-agent can each launch further work through supported CLI and native paths without a delegation refusal.
3. Restart preserves independently running sessions, queue entries, pending replies, recorded worktrees and unfinished work; session-bound losses are shown honestly.
4. Parallel reports and wakes retain valid task bindings, ordering and one active owner per home.
5. Push or PR-creation retries reconcile partial publication instead of creating duplicate PRs or losing local work.
6. Agent merge, auto-merge, merge-queue and remote target-branch bypass attempts are refused by supported integration backstops; human merge and subsequent polling still work.
7. Missing deep-review/vplan configuration or assets do not impede ordinary work; explicitly requested tool runs retain meaningful errors and real results.
8. Workflow stages execute in order, may delegate internally, and do not advance on a false status report, missing output, failed command or unresolved explicit interaction.
9. Mixed legacy fixtures migrate repeatably without changing pending approvals into completed facts, reviving `yolo` merges or losing child-home settings.
10. Public help, fresh prompts, dashboard labels and retained skills distinguish the orchestrator, sub-agent assignments, persistence and task state without reviving legacy policy classes.
11. An underspecified feature produces research, an agreed scope where user input is needed, and a versioned implementation brief; an explicit planning workflow can include requested vplan discussion.
12. MX Viz renders 20 mixed tasks with nested researchers/implementers/reviewers, decision ownership, current workflow stages, PR readiness and stale or interrupted observations without miscounting sessions as tasks.
13. A comparative 1/5/10/20-task trial records queue age, wake-to-durable-disposition time, orchestrator response delay, resource use, review rework and useful completion throughput.
14. Failure-injection trials cover crash after wake display, duplicate command delivery, old-attempt completion after restart, changed plan during implementation and a slow backend beside healthy ones; A1-A4 define the required recovery behavior.
15. Filesystem transitions recover after each multi-record interruption boundary, with no competing state authority, lost accepted task or duplicate mutable-resource owner.
16. Capacity saturation queues work durably, priority changes take effect, small tasks avoid starvation, dependency cycles are rejected and unrelated work continues past a blocked task.
17. A delayed human answer or review result for an old revision remains historical; research and current evidence reach the correct implementer and the human review queue without a new approval ceremony.
18. MX Viz meets A7 usability checks and A8 measured targets, including multiple tabs, partial failures, stable keyboard focus and visible observation age.
19. Launch from a non-repository dev folder with repo 1 directly beneath it, repo 2 in a grouping folder and repo 3 nested more deeply; discover and reuse all three through remembered configurable roots without cloning or requiring URLs.
20. In one orchestrator chat, request feature A in repo 1, feature B in repo 2 and feature C in repo 3; each gets its own task, checkout, starting revision, evidence and PR/local outcome, while changing TUI context leaves all bindings intact.
21. Overlapping roots, symlink cycles, duplicate names, separate clones of one remote, linked worktrees, nested repos, missing paths and unversioned folders yield truthful identity and bounded discovery; an ambiguous selection blocks only affected work.
22. Concurrent launches and duplicate multi-task submissions converge on one main orchestrator and one accepted task per request item; reconnect or durably route through the supported provider, then recover after restart without losing queued work.
23. Install a verified release with assets without a source checkout or Rust build, optionally configure discovery roots and remember a harness, and enter the same workspace from a fresh terminal; upgrade/uninstall preserve operational state and user repositories.
24. TUI search, keyboard navigation, resize, scan progress, task/decision display and MX Viz shortcut use shared state; closing the TUI preserves independently running work.
25. Borrowed dirty checkouts and remote-free repositories remain usable under their actual limits; registration, refresh, unregister and task completion do not change the user checkout or fabricate a remote, and every implementation records its starting revision.
26. Launch from home, a repo root/subdirectory and an unrelated directory with and without discovery roots; all reach the same workspace and can submit multi-repository tasks without scanning the launch directory or imposing a dev-folder requirement.
27. With Treehouse absent from PATH and downloads disabled, normal startup, task acquisition, persistent-home provisioning, status, safe release and package validation use only the built-in manager and declared remaining tools.
28. Concurrent acquisition, retried requests, endpoint failure, process exit and stale release tokens preserve one allocation owner; persistent leases survive zero live processes and recover after interruption.
29. Dirty, untracked, unknown ignored, unpushed, open-PR, uncertain-occupant and corrupt-record worktrees are retained; safe release/reuse/prune affect only proven managed targets and leave user-owned checkouts intact.
30. Migrate recorded v2.0.1-style leases and task paths with old wrappers quiesced, including move failure and interrupted path updates; preserve work and foreign Treehouse pools, and never permit both managers to mutate one allocation.
31. All supported session backends launch at the exact returned allocation path without Treehouse shell injection or cwd guessing; capacity, startup/setup cost and worktree retention are measured in the existing 1/5/10/20-task trials.

32. From the main chat, create three scoped sub-orchestrators for repositories or ideas; each delegates research/implementation while the root still handles direct tasks and human discussion, and neither coordinator writes project code.
33. Duplicate domain spawn and task handoff requests, including crashes between home creation, binding and endpoint launch, converge on one coordinator/assignment without duplicate children or lost charter state.
34. With an intermediate model idle and its summary omitted, recorded findings, failure, PR readiness and human-decision events reach the root through durable runtime relays; retries and restarts do not duplicate human presentation or lose distinct events.
35. Same-basename/wrong-home reports, stale parent generations and unrelated acknowledgements cannot settle a request; missing replies trigger bounded recovery, and delayed human answers apply only to their exact current revision.
36. Restart or replace a sub-orchestrator with live independent children, transfer a task between domains, and interrupt retirement with an undelivered outcome; preserve one owner, child worktrees, pending messages and truthful retained state.
37. Saturate a shared capacity budget across several domains and direct workers; preserve worker headroom, fair progress and root responsiveness, and distinguish a live endpoint with stalled queue handling from a healthy coordinator.
38. Research an idea without a repository, bind its eventual project explicitly, and run two non-exclusive domains in one repository; project selection never creates a coordinator, shared tasks are not duplicated and implementation remains isolated.
39. Migrate coordinating and non-coordinating legacy homes, inspect the domain hierarchy in CLI/TUI/MX Viz, and compare flat versus hierarchical 1/5/10/20-task trials under equal total budgets, including parent outage and multiple viewers.

Run focused checks during implementation, then the release checks once the combined tree is ready:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo build --release --workspace --locked
target/release/mx test-run --check-coverage
target/release/mx test-run --all --jobs auto
target/release/mx doc-audience-check
```

Exercise Linux and macOS, the existing tmux/Herdr/cmux support matrix, and Claude/Codex/Cursor/Pi integration surfaces that actually changed.
Do not equate a fake forge or mock harness with a live integration result.
Use isolated test homes, backend namespaces and test repositories for mutation checks.
Do not widen previously unsupported combinations, including cmux persistent-home launch or a shell-callable Codex Desktop backend, without implementing and verifying them.

## Planning deliverables and known baseline

The planning change updates `CLAUDE.md`, creates this guide and twelve HTML phase plans, links the new roadmap from the existing plan index, classifies the new Markdown surface, and removes the excluded `firstmate/` folder from the checkout.
It does not alter production role checks, launch permissions, workflows or delivery code.
`AGENTS_E.md` remains the user's renamed file, with its original contents until implementation starts.
The initial documentation audience check fails because it expects `AGENTS.md` and does not classify `AGENTS_E.md`.
Other existing documentation also still links to the absent root filename.
Repair those active owner links during Phase 01 and validate the final restored release in Phase 12; do not undo the user's rename just to make a planning check pass.
