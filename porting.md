# Multplx lean orchestration port

## Purpose and status

Replace Multplx's prescriptive agent operating model with a small coordination contract and a capable CLI.
The user-facing agent is the **orchestrator** and every delegated worker is a **sub-agent**.
The orchestrator can do work directly, delegate, commit, push branches, and create or update PRs within the user's request.
PR merges remain human-executed.
The dependency-ordered implementation plans are in [plans/lean_redesign](plans/lean_redesign/index.html).

This is a planned product redesign, not a behavior-preserving language port.
No runtime change is implemented by these planning documents.
The source baseline is commit `6360b040460a08c2d5b8bcfcbc5e64e6e024ab53`, inspected on 2026-09-11.
The existing [Rust port guide](plans/rust_port/PORTING.md) records an earlier program whose old policy-preservation requirements do not govern this redesign.

## Working checkout

The user deliberately renamed the root operating contract to `AGENTS_E.md` to prevent automatic injection.
Treat [AGENTS_E.md](AGENTS_E.md) as old product source, not as the porting agent's instructions.
Keep that filename throughout development and edit the future operating contract there when Phase 01 is implemented.
Do not recreate root `AGENTS.md`, teach startup to discover `AGENTS_E.md`, run Multplx session start, or use private operational homes to execute this port.
Project-level `AGENTS.md` files are a separate concern and keep their normal meaning.
[CLAUDE.md](CLAUDE.md) provides lean contributor context for this checkout.
Phase 08 describes testing the release contract in an isolated package before the deliberate final filename restoration.

Exclude `firstmate/` completely from reading, testing, packaging, comparison, and implementation references.
The planning change removed that folder from the checkout by moving it to the local Trash without reading its contents.
The port does not depend on it.
Historical plans remain historical records; the main plan index links to this program without rewriting old decisions.
Private `data/`, `state/`, `config/`, credentials, and project clones are not inputs to planning and are never committed.

## Accepted product direction

| Area | Target behavior |
| --- | --- |
| Identity | One orchestrator interface and sub-agents, with no broker, actor, scout, daemon, reviewer, or gate-agent privilege classes. |
| Direct work | The orchestrator may inspect, plan, code, test, fix, commit, and deliver without dispatching merely to satisfy a role rule. |
| Delegation | Any agent can delegate within its assigned scope using Multplx sessions or available native agent tools. |
| Persistence | Long-lived sub-agents remain available as a lifecycle choice, with existing isolated-home and restart mechanics. |
| Delivery | Commit, branch push, PR creation, PR updates, and ordinary follow-up fixes require no additional Multplx approval ceremony. |
| Merge | No agent merges a PR, enables auto-merge, submits it to a merge queue, or delegates the merge to another agent or automation. |
| Workflow | Execute every selected stage in order and satisfy its declared outputs and explicit user-interaction points. |
| Review tools | Run deep-review or vplan only when the user explicitly requests the tool or knowingly selects a workflow that explicitly includes it. |
| Method | Let the agent choose its coding, diagnosis, review, testing, and delegation approach within the task. |
| Coordination | Preserve validated status events, current-state reconciliation, session identity, locks, durable wakes, message correlation, and recovery. |

A reviewer or an investigator can be a sub-agent's assignment without becoming a runtime role.
A persistent sub-agent can coordinate further sub-agents without acquiring a different permission class.
The orchestrator remains responsible for the requested outcome; parenting and home ownership are routing facts, not approval ranks.

No `yolo` switch is needed to obtain ordinary autonomy, and an old `+yolo` value must never grant merge authority.
Do not replace deleted rules with a large capability-policy framework, generic exception registry, mandatory review wrapper, or compulsory planning ceremony.
Missing tools, authentication failures, actual ambiguity in the request, and repository-specific requirements can still require action or clarification.
They do not justify reintroducing Multplx-wide approval gates for routine engineering choices.

## Read-through findings and implementation map

The runtime is already Rust: one Cargo workspace, six crates, and the `mx` multicall binary.
Most `bin/` files are transport adapters, so changing their prose alone will not change the product.
The restrictions have several independent enforcement points.

| Surface inspected | Current coupling | Owning phase |
| --- | --- | --- |
| `AGENTS_E.md`, `CLAUDE.md`, `.agents/skills/`, generated briefs | No direct project work, fixed escalation recipes, mandatory skill loads, role-specific definitions of done | 01 |
| `crates/multplx-domain/src/project_registry.rs` | Missing registry, unregistered paths, and self-repo work default to deep-review/off | 02, 05 |
| `crates/multplx-domain/src/lifecycle/brief.rs` | Delivery/scout/daemon templates prohibit pushes and lifecycle calls and prescribe review and project-memory procedures | 01, 02 |
| `crates/multplx-domain/src/lifecycle/spawn.rs`, `crates/multplx-cli/src/lib.rs` | Kind/mode/yolo metadata, daemon-specific launch branches, credential stripping and non-writable origin push URL | 02, 03, 04 |
| `crates/multplx-domain/src/supervision.rs`, `.claude/settings.json`, `.cursor/hooks.json`, `.cursor/rules/multplx.mdc` | Native delegation denial and generated operating instructions | 03 |
| `.codex/hooks.json`, `.pi/extensions/`, `docs/supervision-protocols/` | Harness-specific startup and watcher continuity | 03, 07 |
| `crates/multplx-domain/src/review_delivery.rs`, `crates/multplx-cli/src/review.rs` | Agent-session delivery refusal, approved-SHA handoffs, PR registration, polling, merge and local landing | 04 |
| `crates/multplx-cli/src/deep_review.rs`, `.deep-review.yaml`, services and `share/vplan/` | Optional tools with dependencies that currently reach default behavior | 05 |
| `crates/multplx-domain/src/workflow.rs`, `crates/multplx-cli/src/workflow_runtime.rs`, `workflows/` | Broker/actor executors, headless-stage coupling to deep-review, fixed stage contracts and approval points | 06 |
| `crates/multplx-domain/src/maintainer_override.rs`, `decision_hold.rs`, `crates/multplx-cli/src/authority.rs` | General exception machinery and report-completion attestations mixed with useful durable decisions | 01, 06, 07 |
| `crates/multplx-core/src/`, backend adapters, lifecycle recovery | Reusable coordination infrastructure and identity-bound cleanup | 03, 07 |
| `crates/multplx-domain/src/inheritance.rs`, `handoff.rs`, home seeding and system sync | Persistent-home provisioning, inherited settings and correlated parent replies | 02, 07 |
| Snapshot readers, `share/viz/`, launcher, shell shims, docs, tests and CI | Roles and policy reappear in presentation, health checks, startup and regression expectations | 07, 08 |

Preserve the Rust architecture, existing backend transports, Treehouse integration, and static browser assets where their behavior still fits.
This port does not add a backend, replace the worktree provider, or turn Codex Desktop host tools into a shell-callable backend.
Inspect connected callers and tests fully when implementing each source slice; this map identifies the redesign boundaries, not a substitute for that implementation read.

## Small operating contract

The released contract should explain only the following everyday facts.

- You are the orchestrator; fulfill the user's request directly or through sub-agents using your judgment.
- Agents may commit, push branches, open and update PRs; the human performs PR merges.
- Selected workflow stages and explicit task constraints remain binding.
- Deep-review and vplan require an explicit user request; ordinary testing and discretionary sub-agent review remain available.
- Use the CLI for Multplx-owned state and preserve the status, ownership, locking, and recovery contracts.
- Reconcile existing work at startup and retain unlanded work during recovery and cleanup.
- Locate command mechanics through help and a small optional operational skill index.

Remove prescribed salutations, escalation templates, fixed retry counts, forced investigative methods, mandatory independent reviewers, and the prohibition on doing project work directly.
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
| `maintainer-override` | Remove as the universal procedure | Only mechanics still required by deliberately retained commands; retire records through Phase 07. |
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
They must no longer determine whether an agent may code, delegate, or publish a branch.
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
Replace role-based executors with execution choices such as current orchestrator context or a sub-agent session.
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
Phase 02 owns the schema mapping and Phase 07 owns the executable migration.
Do not renumber tasks, regenerate message correlation IDs, lose worktree leases or rewrite journals to make names look current.

| Legacy surface | Migration treatment |
| --- | --- |
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
| [02](plans/lean_redesign/02-unified-subagent-model.html) | Common sub-agent model and legacy schema mapping | 01 |
| [03](plans/lean_redesign/03-free-delegation-and-coordination.html) | Native and CLI delegation with preserved communication and supervision | 02 |
| [04](plans/lean_redesign/04-agent-delivery-human-merges.html) | Direct agent publication and human-only PR merges | 02 |
| [05](plans/lean_redesign/05-opt-in-review-tools.html) | Deep-review and vplan fully optional, including startup dependencies | 01, 02 |
| [06](plans/lean_redesign/06-workflows-with-agent-freedom.html) | Ordered workflows with autonomous stage execution | 03, 04, 05 |
| [07](plans/lean_redesign/07-state-migration-and-recovery.html) | Upgrade existing homes, aliases, messages and snapshots without losing work | 02-06 |
| [08](plans/lean_redesign/08-documentation-validation-cutover.html) | Consistent docs, behavior tests, harness evidence and release cutover | 01-07 |

This dependency graph permits useful parallel implementation where file ownership is coordinated.
It does not require serial coding or force a particular sub-agent count.
Do not activate a partial release in real homes where new instructions promise freedoms that the old runtime still denies.
Each implementation change should identify the old behavior being intentionally removed and the coordination guarantees being retained.

## Validation strategy

Use existing tests for retained guarantees and change policy assertions that intentionally conflict with the redesign.
Do not preserve obsolete behavior merely to keep old tests green, and do not remove whole safety suites because some assertions change.
The concrete per-phase test inventory is in the HTML plans.
The Rust test runner owns coverage and resource scheduling in `crates/multplx-cli/src/tooling/runner.rs`.

The final acceptance scenarios are:

1. A simple project change can be performed directly by the orchestrator and ends at an open PR without any Multplx skill interview, gate, waiver or delivery approval.
2. The orchestrator and a sub-agent can each launch further work through supported CLI and native paths without a delegation refusal.
3. Restart preserves independently running sessions, queue entries, pending replies, recorded worktrees and unfinished work; session-bound losses are shown honestly.
4. Parallel reports and wakes retain valid task bindings, ordering and one active owner per home.
5. Push or PR-creation retries reconcile partial publication instead of creating duplicate PRs or losing local work.
6. Agent merge, auto-merge, merge-queue and remote target-branch bypass attempts are refused by supported integration backstops; human merge and subsequent polling still work.
7. Missing deep-review/vplan configuration or assets do not impede ordinary work; explicitly requested tool runs retain meaningful errors and real results.
8. Workflow stages execute in order, may delegate internally, and do not advance on a false status report, missing output, failed command or unresolved explicit interaction.
9. Mixed legacy fixtures migrate repeatably without changing pending approvals into completed facts, reviving `yolo` merges or losing child-home settings.
10. Public help, fresh prompts, dashboard labels and retained skills use orchestrator/sub-agent terminology and describe the implemented behavior.

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

The planning change updates `CLAUDE.md`, creates this guide and eight HTML phase plans, links the new roadmap from the existing plan index, classifies the new Markdown surface, and removes the excluded `firstmate/` folder from the checkout.
It does not alter production role checks, launch permissions, workflows or delivery code.
`AGENTS_E.md` remains the user's renamed file, with its original contents until implementation starts.
The initial documentation audience check fails because it expects `AGENTS.md` and does not classify `AGENTS_E.md`.
Other existing documentation also still links to the absent root filename.
Repair those active owner links during Phase 01 and validate the final restored release in Phase 08; do not undo the user's rename just to make a planning check pass.
