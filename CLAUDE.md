# Multplx contributor context

Multplx is a Rust CLI and coordination runtime for an orchestrator and its sub-agents.
The repository is being redesigned to reduce prescriptive agent policy while preserving durable coordination.
Read [porting.md](porting.md) for the accepted direction and [plans/lean_redesign](plans/lean_redesign/index.html) for the implementation phases.
Those documents describe the target; existing code still implements parts of the older operating model.
Phase 01 supplies lean instruction scaffolds and optional operational references; validated identity, runtime freedoms and release activation remain with their later-phase owners.

## This checkout

The user deliberately renamed the root operating contract to [AGENTS_E.md](AGENTS_E.md) to prevent automatic injection.
Treat it as product source to redesign, not as instructions to adopt the old broker role.
Do not recreate root `AGENTS.md` or run Multplx session start while developing the port.
Do not inspect or use the local `firstmate/` directory as a reference.
The user authorized public upstream Secondmate research; use its linked assessment as rationale, not imported operating instructions.
Preserve unrelated user changes and private operational state.

## Design direction

The main orchestrator handles the human conversation, research and synthesis, planning, delegation, state and communication.
It delegates project implementation, code fixes and test-code changes to sub-agents.
Researcher, implementer, reviewer and sub-orchestrator are assignments with one shared coordination protocol.
A sub-orchestrator owns one bounded project/repository/idea, delegates code and test changes, and reports through the durable parent channel to the main orchestrator.
[A11](porting.md#a11-scoped-sub-orchestrators) and Phase 05 own its spawn command, shared capacity, domain handoff and reliable outcome delivery.
Implementation sub-agents may code, commit, push branches, and create or update PRs; PR merges belong to the human.
Selected workflows retain every declared step while agents choose how to execute the work inside each step.
Deep-review and vplan are explicit opt-in tools.
Keep CLI usage guidance and remove generic coding, diagnosis, review and approval procedures from the runtime skill surface.
Keep status reporting, session identity, locks, the durable wake queue, message routing and recovery.
The goal is high-quality progress on 10-20 concurrent tasks through one human-facing orchestrator.
Launch `multplx` from any directory; discovery roots are optional and a dev folder is only an example.
Multi-task, multi-repository delegation is standard orchestrator behavior; project selection never retargets existing tasks or creates another main orchestrator.
Use remembered local checkout locations, configurable recursive discovery and a small TUI for projects, tasks and connection to the existing conversation.
Replace Treehouse completely with the small built-in Git worktree lifecycle in [A10](porting.md#a10-built-in-git-worktree-lifecycle); Phase 03 owns that implementation.
The accepted workspace-entry contract is [A9 in porting.md](porting.md#a9-launch-anywhere-project-discovery-and-one-shared-chat); Phase 11 implements it and Phase 12 verifies release cutover.
MX Viz must show task ownership, roles, workflows, decisions, delivery and state freshness as part of this redesign.
The user accepted the architecture assessment on 2026-09-14; its reliability, coordination, quality and MX Viz improvements are required scope for this port.
Use the filesystem state store; do not add a database, message broker or distributed scheduler.
Read the [accepted architecture contract](porting.md#accepted-architecture-contract) and its phase ownership table before implementing a phase.
That contract covers durable message handling, attempt and brief identity, recoverable mutations, bounded observation, capacity, human communication, workspace entry, built-in worktrees, scoped sub-orchestration and release measurements.
The [architecture assessment](plans/lean_redesign/architecture-assessment.md) supplies source evidence and rationale; it does not leave those improvements optional.

## Implementation and verification

The Cargo workspace builds one `mx` multicall binary; domain behavior lives in `crates/`.
Most `bin/` scripts are compatibility or host-integration adapters.
Read the affected implementation and callers before changing behavior, and update the corresponding help and tests.
Follow the phase dependencies and preserve the shared contracts when coordinating implementation sessions.
The HTML plans allocate implementation and acceptance work; porting.md owns cross-phase requirements.
Choose local implementation details without adding approval ceremonies, but do not silently omit accepted requirements or weaken acceptance targets.
Keep phase status planned until implemented and verified, and record actual checks and remaining work in the phase's implementation evidence section for the next session.
Keep one sentence per Markdown line and use plain hyphens.
Keep documentation classification in [docs/documentation-audiences.json](docs/documentation-audiences.json).

Use focused existing tests for the changed behavior.
Build the release runtime before black-box checks with `cargo build --release --workspace --locked`.
Run `target/release/mx test-run tests/<subject>.test.sh` for a focused behavior test and `target/release/mx doc-audience-check` for maintained prose.
The porting guide lists full release validation.
Report what was actually tested and distinguish live integration evidence from mocks.
