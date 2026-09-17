# Phase 08 implementation evidence

## Status and source boundary

Status: complete; assigned implementation and acceptance checks passed on 2026-09-16 Eastern time, with final automated results recorded on 2026-09-17 UTC.
Work started on branch `codex/lean-redesign-phase-08` from clean merged prerequisite `a60661d388349488b73319734d4940b32a442b46`.
The implementation revision is `b5cd12ca29bfd9c93203ec2493f2f570efc04096`.
The [phase plan](08-workflows-with-agent-freedom.html) allocates this work and [porting.md](../../porting.md#accepted-architecture-contract) owns the shared contracts.

Inspection confirmed the implemented Phase 03 allocation owner, Phase 04 durable coordination and admission, Phase 05 coordinator and parent channel, Phase 06 delivery evidence and Phase 07 explicit review-tool boundaries before Phase 08 was changed.
Root `AGENTS.md` remains absent, `AGENTS_E.md` remains dormant and the excluded `firstmate/` directory was not inspected or copied into validation fixtures.
No source-checkout session start or private operational-home execution was used.

## Requirement and verification ledger

| Assigned requirement | Implementation and evidence |
| --- | --- |
| Current placement and assignment vocabulary | Workflow schema version 2 uses `orchestrator-context` and `sub-agent-session`, with researcher, implementer, reviewer and sub-orchestrator as descriptive assignments; version 1 alone maps legacy broker and actor tokens. |
| Main-context coding boundary | Implementer and sub-orchestrator assignments require sub-agent placement; main-context stages cannot request fresh sessions or local-commit contracts. |
| Generic agent transport | The file-backed structured command builder is independent of deep-review; explicit deep-review adds its own environment and timeout policy while ordinary workflow stages do not acquire review identity or policy. |
| Ordered internal delegation | A delegated stage remains current until its output or local-commit contract passes; generated briefs allow decomposition only within that stage and explicitly exclude later-stage work. |
| Scoped coordinators | A sub-orchestrator stage launches the Phase 05 coordinator form with one owner, bounded project scope and exact request identity; child summaries alone do not advance the stage. |
| Durable allocation recovery | Launch and every resume re-read the canonical task, attempt, accepted brief, project, checkout, starting revision, allocation path and base revision; stale, foreign, legacy or replaced bindings fail closed. |
| Revision-bound decisions | Holds and answers persist the exact task, accepted brief, workflow digest plus plan revision, question and answer; a delayed answer for another target is rejected before mutation. |
| Explicit exceptional flow | Skip and reorder consume exact grants, append immutable plan history and increment the plan revision; an open human decision must be resolved or the run aborted instead of being silently bypassed. |
| Dependency scheduling | Dependencies are unique existing workflow runs, cycles and self-dependencies are rejected, and only completed evidence releases dependents while independent workflows continue. |
| One-chat multi-repository correlation | `run --request` imports the durable batch/request identity, task, project, checkout, starting revision, brief, scope, dependencies and original context pointer; conflicting CLI overrides fail before snapshot creation. |
| Truthful restart and completion | Per-run locking, immutable snapshots, captured command exits, artifact containment and local-commit ancestry remain required; prose or a false done state cannot satisfy a contract and no stage may merge a PR. |
| Optional tools and examples | Shipped version 2 examples contain no implicit deep-review or vplan stage, direct agent delivery has no extra approval shell, and explicitly selected command stages remain ordered and visible. |
| Operator surfaces | Workflow and decision lifecycle documentation, workflow CLI help, the workflow authoring skill, fixtures and examples describe the current schema, legacy reader, project/request/dependency binding and mock/live limits. |

The implementation extends the existing filesystem run records, request store, task metadata, backlog and decision holds instead of adding another scheduler or database.
Ordinary retries stay within the current stage, and publication and human merge remain separate from workflow completion.

## Focused checks

| Check | Result |
| --- | --- |
| Workflow domain unit tests | Passed all eight focused tests for schema versions, placement constraints, snapshot integrity and ordered transitions. |
| Decision-hold domain unit tests | Passed both focused tests, including exact revision-bound retry identity. |
| `target/release/mx test-run tests/mx-workflow-lib.test.sh` | Passed all seven schema, legacy-reader, deterministic-contract and path-containment scenarios on macOS and Linux. |
| `target/release/mx test-run tests/mx-workflow.test.sh` | Passed all 14 scenarios on macOS and Linux, including three repositories in one request batch, independent progress, dependency release, stale decisions, exact allocation checks, versioned plan changes and coordinator restart. |
| `target/release/mx test-run tests/mx-decision-hold-lifecycle.test.sh` | Passed all ten durable decision lifecycle scenarios on macOS and Linux. |
| `target/release/mx test-run tests/mx-deep-review.test.sh` | Passed all 15 scenarios on macOS and Linux after generic transport extraction. |
| Supporting allocation and coordinator fixtures | `mx-coordinator-spawn`, `mx-worktree` and `mx-upstream-diff` passed on macOS and Linux. |
| Shipped workflow fixture | The new-feature workflow completes by direct sub-agent delivery with no implicit review or delivery approval stage. |

The workflow and coordinator tests use injected deterministic agent, spawn and state adapters.
They prove invocation arguments, durable binding, restart, ordering, failure and completion contracts rather than live model quality.
The final macOS aggregate exercised the installed live Herdr integration, but that platform test is not a live workflow-provider trial.
No new live Claude, Codex, Cursor or Pi workflow trial is claimed; the prior provider deferral remains recorded in the [Phase 04 evidence](phase04-implementation.md).

## Repository checks

The required release checks passed on macOS arm64, and the changed Rust and focused workflow surfaces also passed in an isolated Linux arm64 container.
The macOS host used Rust 1.97.1, Node 24.14.1, Git 2.55.0 and tmux 3.7c.
Linux used an ephemeral ordinary container with Rust 1.98.1, Node 24.14.1, Git, tmux and an init subreaper.
The Linux container was removed automatically after validation.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on macOS and Linux. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed on macOS and Linux. |
| `cargo test --locked --workspace` | Passed on macOS and Linux with 695 tests, zero failures and zero ignored on each platform. |
| `cargo build --release --workspace --locked` | Passed on macOS and Linux. |
| `target/release/mx test-run --check-coverage` | Passed on macOS and Linux with 129 fixtures, 108 accelerated, 11 serial and ten Herdr; this is fixture inventory rather than line coverage. |
| `target/release/mx test-run --all --jobs auto` | Passed on final macOS code with 129 fixtures, zero failures, eight declared gate skips and 324,328 ms. |
| CI-equivalent `cargo llvm-cov` 93 percent line gate | Passed with 93.18 percent line coverage across 63,860 lines and 4,358 missed lines; the later request-correlation change touched only the excluded workflow runtime plus its shell test and documentation. |
| `target/release/mx doc-audience-check` | Passed after evidence publication with 85 maintained surfaces and 439 local links. |
| `git diff --check` | Passed before the implementation commit. |

The first macOS aggregate found one live Herdr presentation-order race after 128 other fixtures passed.
That exact test then passed all 22 assertions in isolation, and the complete final aggregate passed all 129 fixtures.
A post-coverage Rust rerun initially observed two coverage profiles in a help test's intentionally empty directory because stale instrumented build objects remained in the normal target directory.
Cleaning build artifacts and rebuilding normally made the exact test and complete workspace pass; this was validation contamination, not a product source failure.

## Limitations and remaining work

No assigned Phase 08 implementation or acceptance item remains unresolved.
The generic main-context adapter is explicit through `MX_WORKFLOW_AGENT_COMMAND`; configuration and live model quality remain provider concerns rather than workflow completion evidence.
The isolated Linux run covered the complete Rust workspace and all changed or directly supporting black-box fixtures, while the complete 129-fixture aggregate ran on macOS.
Other-provider live trials remain deferred under the Phase 04 acceptance, and combined release activation remains Phase 12 work.
Phase 09 is ready to begin because Phase 08 now preserves legacy snapshots, exact allocations and durable current-revision evidence needed by migration.
