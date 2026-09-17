# Phase 09 implementation evidence

## Status and source boundary

Status: complete; assigned implementation and acceptance checks passed on 2026-09-16 Eastern time, with automated timestamps recorded on 2026-09-17 UTC.
Work started on branch `lean-redesign-phase-09` from clean merged prerequisite `b12e1173c453dd75cd8348c048fb42fcbdbc6ece`.
The [phase plan](09-state-migration-and-recovery.html) allocates this work and [porting.md](../../porting.md#accepted-architecture-contract) owns the shared contracts.

Inspection confirmed the implemented Phase 02 identity and recoverable-transition primitives, Phase 03 allocation and persistent-home owners, Phase 04 durable coordination records, Phase 05 domain roles, Phase 06 publication receipts, Phase 07 retained review boundaries and Phase 08 workflow snapshots before Phase 09 was changed.
Root `AGENTS.md` remains absent, `AGENTS_E.md` remains dormant and the excluded `firstmate/` directory was not inspected or copied into validation fixtures.
No source-checkout session start or private operational-home mutation was used.

## Requirement and verification ledger

| Assigned requirement | Implementation and evidence |
| --- | --- |
| Explicit migration lifecycle | Public `mx migrate inspect/apply/rollback/summary/relocate-worktree` commands separate read-only planning, locked conversion, exact rollback, bounded recovery context and explicit resource transfer. |
| Writer exclusion and compatibility | Apply, rollback and transfer refuse a live session owner, unsafe links, corrupt records and newer schemas; legacy aliases and task files remain readable during the compatibility window. |
| Repeatable recoverable publication | The migration stores exact private before/after backups, uses the core recoverable transition owner and safely resumes at every injected intent, write, progress and commit interruption boundary. |
| Durable work preservation | Wake queues, claimed or waiting messages, pending replies, journals, status history, workflow snapshots, decisions, PR records and action receipts are not rewritten; fixtures verify representative records byte-for-byte. |
| Honest task identity | Legacy task files gain a schema-2 compatibility projection without fabricated attempt, brief or completion identity; ambiguous values remain `legacy_unknown` and historical approval, yolo or publication fields confer no authority. |
| Configuration aliases | Actor/daemon harness and dispatch aliases copy to canonical sub-agent/coordinator names only when values agree; a conflict blocks before partial publication and legacy files remain available until Phase 12. |
| Project migration | Available flat managed checkouts gain Phase 02 project identities through the canonical registry without moving repositories; missing locations are retained as repair items. |
| Compact recovery | The restart summary is bounded and exposes task, owner, assignment, attempt, accepted brief, dependency, open-question and evidence identities without transcript replay. |
| Legacy worktree transfer | Exact recorded Git worktrees move into the internal Phase 03 store only after ownership, project, task, attempt and process-quiescence checks; a new lease and historical mapping are published transactionally. |
| Persistent-home transfer | A legacy persistent worktree receives matching internal worktree and home-allocation receipts even when no modern home receipt existed, and every task/home path is updated in the same recoverable publication. |
| External manager isolation | Treehouse metadata is inert input; missing v2.0.1 acquisition IDs remain historical uncertainty, foreign entries remain untouched and retries after an interrupted Git move converge on the same allocation. |
| Domain responsibility | Coordinator mapping requires an explicit task selection plus persistent-home evidence; persistence alone never creates coordinator authority and existing children, routes and correlations are retained. |
| Operator surfaces | Migration, rollback, compatibility and worktree-transfer conditions are documented in CLI help, the operator migration guide, configuration, doctor, startup, sub-agent and worktree documentation. |

The implementation extends the existing filesystem records, project registry, task model, worktree store, home allocation and recoverable transition primitive instead of adding another database or resource manager.
Unsupported or ambiguous records remain retained with a concrete diagnostic.

## Focused checks

| Check | Result |
| --- | --- |
| `target/release/mx test-run tests/mx-state-migration.test.sh` | Passed on macOS and Linux with all ten scenarios for read-only inspection, repeat apply, exact rollback, all home fault boundaries, alias and unsafe-lock rejection, bounded or unavailable summaries, exact coordinator mapping, live occupant refusal, interrupted Git transfer, v2.0.1 lease uncertainty, foreign pool isolation and persistent-home receipts. |
| Existing lock and queue fixtures | Session-start lock/bootstrap, process liveness, wake queue and watcher lock passed. |
| Existing lifecycle fixtures | Daemon sync/liveness, pending reply and backlog handoff passed. |
| Existing security and projection fixtures | All three PR-check security suites plus status snapshot and system snapshot passed. |
| Existing allocation fixture | The complete worktree fixture passed alongside the new real-Git relocation cases. |
| `target/release/mx test-run tests/mx-doctor.test.sh` | Passed with the new current, legacy and incompatible home-migration diagnostics. |

The migration fixtures use isolated temporary homes and exact byte manifests.
The relocation cases use real local Git repositories and worktrees, real process-occupancy observation and deliberate process exit at the post-Git boundary.
They do not mutate private operational state or an external Treehouse service.

## Repository checks

The required release checks passed on macOS arm64 with Rust 1.97.1, Node 24.14.1, Git 2.55.0 and tmux 3.7c.
The changed migration surface also passed in an isolated Linux arm64 container as ordinary user `mxvalidate` with Rust 1.98.1, Git and tmux.
The disposable Linux copy excluded `firstmate/`, the host `.git` directory and the host build tree, and was removed after validation.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on macOS and Linux. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --workspace --locked` | Passed across the complete workspace with zero failures. |
| `cargo build --release --workspace --locked` | Passed on macOS and Linux. |
| `target/release/mx test-run --check-coverage` | Passed with 130 fixtures: 109 accelerated, 11 serial and ten Herdr. |
| `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase09-isolation-proof.json` | Passed 108 portable candidates per round, two rounds, 553,872 ms, zero failed rounds, zero leaks and zero known-failure observations; the 130-resource archive has manifest SHA-256 `cd0877f1fbab85d7627fa221fd65283b04c6d99dcdf235fc20541ac8abd85b08` and 653 conflict pairs. |
| `target/release/mx test-run --all --jobs auto` | Final macOS aggregate passed all 130 fixtures with zero failures and eight declared environment skips in 326,069 ms. |
| `target/release/mx doc-audience-check` | Passed with 87 maintained surfaces and 450 local links after evidence publication. |
| `target/release/mx shadow-diagnostic` | Reported ready. |
| `git diff --check` | Passed. |

The test-run coverage command verifies fixture inventory and classification rather than source-line coverage.
Mocked state adapters establish deterministic recovery semantics, while the focused relocation fixture is real local Git and process-liveness evidence.
The first aggregate correctly rejected the stale 129-resource isolation archive after the Phase 09 fixture was added.
After the two-round proof archive refresh, a second aggregate observed one load-sensitive real-Herdr focus-order failure outside the changed surface; that exact fixture passed all 22 assertions alone in 162,776 ms, and the complete third aggregate passed.

## Limitations and remaining work

No assigned Phase 09 implementation or acceptance item remains unresolved.
Migration was proven on isolated fixture homes as the phase plan requires; no private operational home was available or changed.
No live external Treehouse daemon or model-provider harness was involved, because migration intentionally treats exported legacy metadata as inert input and must not create a competing manager.
Linux covered formatting, all migration domain tests, the release workspace build and the focused real-Git migration fixture; the complete 130-fixture Linux aggregate and combined release cutover remain Phase 12 responsibilities.
Legacy readers and aliases remain intentionally available until the Phase 12 compatibility-window decision.
Phase 10 is ready to begin because canonical homes now provide truthful task, domain, project, allocation and recovery projections for visualization.
