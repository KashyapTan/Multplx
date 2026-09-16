# Phase 07 implementation evidence

## Status and source boundary

Status: complete; assigned implementation and acceptance checks passed on 2026-09-16 (UTC).
Work started on branch `codex/lean-redesign-phase-07` from clean merged prerequisite `c3f66e76b7eaf7323dc7837e02b4a90befc802c8`.
The implementation revision is `14398800d66f295a6a366d01fd2c61bf6179699c`.
The [phase plan](07-opt-in-review-tools.html) allocates this work and [porting.md](../../porting.md#accepted-architecture-contract) owns the shared contracts.

Inspection confirmed the implemented Phase 02 task, attempt and brief identity, Phase 03 allocation ownership, Phase 05 parent outcomes and Phase 06 revision-bound delivery evidence before Phase 07 was changed.
Root `AGENTS.md` remains absent, `AGENTS_E.md` remains dormant and the excluded `firstmate/` directory was not inspected or copied into validation fixtures.
No source-checkout session start or private operational-home mutation was used.

## Requirement and verification ledger

| Assigned requirement | Implementation and evidence |
| --- | --- |
| Separate destination from review | Missing, unknown, unregistered and self-repository project modes now select ordinary direct PR delivery instead of deep-review; explicit legacy mode remains readable without launching a review tool. |
| Remove implicit invocation | General-purpose workflows, briefs, recovery guidance, configuration prose and CLI help no longer imply deep-review or vplan for ordinary code, reports or HTML plans. |
| Lazy optional readiness | Bootstrap no longer checks vplan assets; vplan validates assets only for `new`, `review` or explicit self-check; doctor reports an asset failure only for an active recorded vplan run. |
| Explicit deep-review behavior | `mx deep-review` remains an explicit bounded pipeline with findings and truthful failures, but it creates no publication approval or `.ready-to-push` authority. |
| Exact deep-review evidence | Version 2 records bind explicit intent to task, attempt, generation, accepted brief, project, allocation and reviewed commit; stale bindings and old passed commits cannot review a replacement revision. |
| Delivery and A11 integration | A successful canonical run records Phase 06 `DeliveryReview(Passed)` and `EvidenceUpdated` facts for the exact task revision, which the Phase 05 parent outcome route can relay without creating publication authority. |
| Legacy continuation | Version 1 retained gate runs remain historical and require a fresh explicit invocation before upgrade; unknown legacy intent never becomes a startup or delivery prerequisite. |
| Project-scoped vplan | `new`, `review`, `comments` and `stop` accept an explicit project root; task-bound review validates the current attempt, accepted brief, allocation and exact selected project. |
| Revision-bound vplan | Version 2 service records bind the canonical artifact root and SHA-256 plus optional task, attempt, brief, project and allocation identity; changed artifacts and superseded briefs are refused while cleanup remains available. |
| Optional failure isolation | Missing optional assets do not block unrelated startup, delegation, delivery, comment recovery or stop; one project's review configuration is never loaded merely by discovering or working in another project. |
| Worktree-provider independence | Both tools use canonical task allocation paths and contain no Treehouse executable or review-specific allocation policy; Treehouse-absent startup, review and cleanup fixtures pass. |
| Operator surfaces | Architecture, configuration, delivery, doctor, scripts, upstream, vplan, vplan-authoring and workflow documentation plus command help describe explicit invocation, lazy checks, exact evidence and the absence of publication coupling. |

The implementation extends existing filesystem records and Phase 06 delivery evidence instead of adding another database, scheduler or approval registry.
Legacy records remain readable and historical evidence remains retained, but only a current explicit request can produce current review evidence.
Comments remain discussion artifacts and do not amend accepted scope automatically.

## Focused checks

| Check | Result |
| --- | --- |
| Project-registry unit tests | Passed all 11 focused tests, including legacy mapping without implicit review. |
| Vplan unit tests | Passed all five focused service tests. |
| Deep-review unit tests | Passed all three focused CLI tests. |
| Actor-state unit tests | Passed all four focused attribution and fail-closed tests. |
| `crates/multplx-cli/tests/services_runtime.rs` vplan cases | Passed all three native loopback integration cases, including stale brief and changed-artifact refusal. |
| `target/release/mx test-run tests/mx-deep-review.test.sh` | Passed all 15 scenarios, including canonical task evidence, revision history, timeout and command failure. |
| Deep-review support fixtures | `mx-deep-review-lib` and `mx-deep-review-config-contract` passed. |
| `target/release/mx test-run tests/mx-vplan.test.sh` | Passed all five loopback service scenarios. |
| Startup and recovery fixtures | `mx-bootstrap`, `mx-doctor`, `mx-daemon-safety` and `mx-workflow` passed with optional tools absent or broken only inside an active run. |
| Supporting behavior fixtures | Focused brief and system-snapshot checks passed after their expected projections were updated. |
| Resource isolation proof | Passed for the changed test-resource schedule. |

The deep-review shell fixture uses a deterministic fake review harness, so it proves command, evidence and failure semantics rather than live model-review quality.
The vplan tests run the real loopback HTTP service, token checks, persistence and shutdown paths against temporary project artifacts.
No live external model provider was required or claimed for this optional-tool phase.

## Repository checks

The required release checks passed on macOS arm64 and Linux arm64.
The macOS host used Rust 1.97.1, Node 24.14.1, Git 2.55.0 and tmux 3.7c.
Linux used an isolated ordinary-user container with a UTF-8 locale, a real subreaper for process-lifecycle tests, Rust 1.98.1 and Node 24.14.1.
The temporary Linux container and its intermediate image were removed after validation.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on macOS and Linux. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed on macOS and Linux. |
| `cargo test --locked --workspace` | Passed on macOS and Linux with 695 tests, zero failures and zero ignored on each platform. |
| `cargo build --release --workspace --locked` | Passed on macOS and Linux. |
| `target/release/mx test-run --check-coverage` | Passed on both platforms with 129 fixtures, 108 accelerated, 11 serial and ten Herdr; this is fixture inventory rather than instrumented line coverage. |
| `target/release/mx test-run --all --jobs auto` | Passed on macOS with 129 fixtures, zero failures, eight declared gate skips and 325,367 ms; passed on Linux with 129 fixtures, zero failures, 16 declared gate skips and 185,864 ms. |
| `target/release/mx doc-audience-check` | Passed after final status and evidence updates with 84 maintained surfaces and 435 local links. |
| `git diff --check` | Passed before each commit. |

The first Linux run used the distribution Node 20 executable and failed only the two TypeScript Pi extension fixtures.
Both fixtures passed individually and the complete 129-fixture suite passed after the container was corrected to the repository-supported Node 24.14.1 runtime.
A process-reaping unit also demonstrated that a container without an init subreaper retains a killed zombie; the focused test and complete Rust suite passed under `tini -s`, matching a correctly initialized Linux runtime.
Those environment-mismatch failures are not counted as passing evidence.

The macOS full suite exercised the installed Herdr integration where available.
Its eight declared skips were unavailable or explicitly opt-in cmux/provider/Pi integration gates, while Linux additionally lacked Herdr.
The gated fixtures are reported as skipped and are not represented as live integration evidence.

## Limitations and remaining work

No assigned Phase 07 implementation or acceptance item remains unresolved.
The review tools are optional and verified, but a deterministic fake deep-review harness does not establish the quality of a live provider review.
The prior user-approved deferral of other-provider live trials remains recorded in the [Phase 04 evidence](phase04-implementation.md), and Phase 12 retains combined release activation.
Unsupported cmux persistent-home and shell-callable Codex Desktop launch combinations remain unsupported.
Phase 08 is ready to begin because its Phase 04, 05, 06 and 07 prerequisites are complete.
