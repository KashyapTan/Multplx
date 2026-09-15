# Phase 02 implementation evidence

Status: complete for Phase 02, 2026-09-14; no release activation is claimed.
Work began on `lean-redesign-phase-02` from `abc2d9f`, the merge of Phase 01 PR #37, on 2026-09-14.
The commits on that branch and its associated pull request identify the delivery revision.
The initial checkout was clean.
The root operating contract remains dormant as `AGENTS_E.md`.
No operational session start or private-home migration is part of this work.

## Prerequisite and source inspection

The implementation inspected `CLAUDE.md`, `porting.md`, the plan index, both Phase 01 and Phase 02 plans, and Phase 01's implementation evidence before editing.
The inspected Phase 01 code supplies concise briefs, literal context handoffs, reviewer scaffolding and persistent charter readers.
It does not supply canonical attempt, accepted brief, project or allocation identity.
The architecture, Treehouse replacement and scoped sub-orchestrator assessments supply rationale; their upstream operating instructions are not imported.

The affected source owners are task spawning and brief generation, persistent home provisioning and configuration inheritance, backlog handoff, project registry and synchronization, report binding, system snapshot production and typed reads, and filesystem publication.
The baseline uses delivery/scout/daemon wire kinds, mode/yolo fields, flat managed-project lookup and task-bound status reports without independent attempt or brief generations.

## Acceptance inventory

| Phase 02 obligation | Evidence required |
| --- | --- |
| Common task/session/home model and explicit assignment changes | Versioned records, compatibility mapping and launch integration tests |
| A1 recoverable filesystem transitions | Intent, partial-write, commit and retry fault matrix with one authoritative completion point |
| A2 attempts, accepted briefs and message/evidence binding | Replacement versus proven resume, stale reports, changed briefs, wrong recipients and explicit unknown identities |
| Persistence and nested parenting | Preserved homes, pinned configuration, routes and leases; cycle and duplicate-owner rejection |
| Typed A4-A6 inputs | Derived task projection with dependencies, priority, runnable/waiting, decisions and evidence pointers |
| A9 project and checkout identity | Local-only repositories, duplicate names, separate clones, linked worktrees, path replacement and immutable routing |
| A10 allocation foundation | Exact project/common-Git/path/base/task/attempt/persistence binding and stale lease-generation refusal |
| A11 domain foundation | Non-exclusive domains, root/parent/owner homes, scope and assignment generations, transfer identity and acyclic lineage |
| Migration boundary | Read-only legacy conversion, ambiguity refusal and explicit writer-version boundary; no home-level cutover |
| Public interfaces | Actual CLI help, configuration mapping, lifecycle documentation and examples |
| Required regression checks | Rust lifecycle/local-state/dispatch and all seven named Phase 02 shell suites |

## Implemented contracts

- `TaskRecord` schema 2 is embedded in the existing task metadata owner, with separate role, artifact, persistence, runtime, attempt, accepted brief, ancestry and scheduling facts.
- Legacy delivery/scout/daemon records normalize without inventing missing attempt or native session identities; duplicate, malformed, conflicting and unsupported records refuse.
- Explicit assignment revision archives accepted brief bytes and history; replacement retains prior execution and allocation references, while proven resume requires actual runtime identity.
- Report envelopes validate task and home, exact configured state location, sender/recipient, attempt, generation and accepted brief; stable message retries append once and stale evidence remains inspectable.
- Versioned filesystem receipts retain intent and progress, verify repeated writes and reject conflicting unfinished operations; the last atomic publication is the authoritative acceptance point.
- Launch reserves durable task/attempt intent, releases task and inheritance locks before backend commands, and revalidates identity at publication; interrupted intent refuses duplicate external launch and remains available for Phase 04 reconciliation.
- Dependency, priority, waiting and revision-bound decision facts feed the existing snapshot wrapper through one shared typed projection, including explicit invalid-record errors.
- Canonical launch, brief and harness/configuration interfaces preserve explicit provider/model/effort, persistent home ownership and pinned inherited settings; report output no longer grants or removes delegation permission.
- Queue records freeze the selected task, attempt, accepted brief, project, checkout, starting revision and resolved launch choices before dispatch.
- Project catalog schema 2 distinguishes separate clones and linked worktrees, preserves local publication without a remote, and refuses ambiguous or replaced locations.
- Before harness submission, launch checks the allocated worktree's common Git identity and accepted base; an exact recorded replacement can retain descendant task commits, while new allocations cannot silently advance their base.
- Remembered borrowed checkouts cannot be upgraded silently, synchronized as managed clones or deleted by cleanup; forgetting removes only catalog metadata.
- Allocation/lease identity binds acquisition generation, exact project/common-Git/path/base, task attempt and persistence; the model rejects stale tokens without claiming an implemented Git manager.
- Home-qualified lineage, domain scope revisions, assignment generations and transfer records supply Phase 05's foundation; non-exclusive project domains remain possible and conflicting owners or cycles refuse.
- Fresh homes claim the canonical writer version; existing legacy homes require the later migration transaction rather than an automatic rewrite.

The public reference is [sub-agent records and compatibility](../../docs/subagent-model.md).
Configuration, CLI help, adapter descriptions and the project/persistence operational references describe implemented syntax and identify later-phase interfaces explicitly.

## Checks and results

The reference machine is macOS 26.6.2 on arm64, Rust 1.97.1 and Cargo 1.97.1.
The initial release build and seven assigned shell suites passed before canonical launch integration.
The final release runtime includes the allocated-worktree identity guard and independent brief output selection.
The final complete regression passed after all runtime and fixture corrections.

| Check | Observed result |
| --- | --- |
| `cargo test -p multplx-domain --lib project_registry --locked` | Passed, 10 tests using real temporary Git repositories. |
| `cargo test -p multplx-domain --lib lifecycle::system_sync --locked` | Passed, 6 tests including borrowed-checkout preservation. |
| `cargo test -p multplx-cli --lib bootstrap::tests --locked` | Passed, 20 tests including canonical config and conflicting aliases. |
| `cargo test -p multplx-domain --locked subagent_model` | Passed, 11 tests including cross-home parent validation and custom state paths. |
| `cargo test -p multplx-core --locked transition_tests` | Passed, 4 tests including all seven injected intent/write/progress/commit boundaries. |
| `cargo test -p multplx-cli --test phase02_model --locked` | Passed, 7 tests, including concurrent three-repository CLI queue writers and exact single-checkout reservation validation. |
| `cargo test -p multplx-domain --lib lifecycle::teardown --locked` | Passed, 27 tests. |
| `cargo test -p multplx-domain --locked lifecycle::brief` | Passed, 5 tests including independent role/output/persistence and invalid combinations. |
| `cargo test --locked --workspace` | Passed, 500 tests across 29 test targets; 0 failed, 0 ignored. |
| `cargo fmt --all -- --check` | Passed after final source formatting. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed after the final identity guard. |
| `cargo build --release --workspace --locked` | Passed; final confirmed build completed in 39.38 seconds. |
| `target/release/mx test-run tests/mx-system-sync.test.sh` | Passed, 22 behaviors, one script, no gate skip. |
| `target/release/mx test-run tests/mx-spawn-worktree-settle.test.sh` | Passed, all 7 behavior groups, 27,253 ms; wrong repository/base, actual queued base drift, retained-commit replacement and stale proof included. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-phase02-all-final.json` | Passed: `total=127 failed=0 skipped_gate=8 duration_ms=302202`; all seven assigned suites passed without skips. |
| `target/release/mx doc-audience-check` | Passed after the evidence/index updates: 76 surfaces, 381 local links. |
| `target/release/mx test-run tests/mx-instruction-owners.test.sh tests/mx-documentation-audiences.test.sh --jobs auto` | Passed after the final documentation changes: 2 scripts, 0 failures, 0 skips, 970 ms. |
| `target/release/mx test-run --check-coverage` | Passed: 127 scripts, 106 accelerated, 11 serial, 10 Herdr. |
| `target/release/mx shadow-diagnostic` | Passed: Rust shadow ready. |
| `git diff --check` | Passed after the final evidence/index updates. |

Intermediate failures found and corrected included missing immutable-brief directories, fixtures without accepted briefs, positional inherited-setting assertions, obsolete permission-promotion and gate expectations, archived charter paths, ephemeral identity comparisons, missing-endpoint replacement, and an overbroad borrowed-checkout retirement guard.
The first full regression returned `total=127 failed=7 skipped_gate=8 duration_ms=253757`; this was not a passing gate.
Later focused runs exposed a Bash 3 empty-array fixture error, remote-free publication expectations and a queue-capacity fixture setup error.
The final complete regression passed every non-gated script, including all corrected cases.
A seven-suite behavior pass initially ended with runner exit 2 because its JSON destination used macOS's `/tmp` symlink; subsequent timing artifacts use `/private/tmp`.

The final full run's seven assigned suites were:

| Suite | Final result | Duration (ms) |
| --- | --- | --- |
| `tests/mx-brief.test.sh` | Passed, no skip | 593 |
| `tests/mx-spawn-dispatch-profile.test.sh` | Passed, no skip | 28,662 |
| `tests/mx-spawn-batch.test.sh` | Passed, no skip | 230 |
| `tests/mx-daemon.test.sh` | Passed, no skip | 1,591 |
| `tests/mx-daemon-safety.test.sh` | Passed, no skip | 32,339 |
| `tests/mx-daemon-harness-model-resolution.test.sh` | Passed, no skip | 12,102 |
| `tests/mx-shared-maintainer-inheritance.test.sh` | Passed, no skip | 2,393 |

The machine-readable full result is `/private/tmp/mx-phase02-all-final.json`; its text log is `/tmp/mx-phase02-all-final.log`.
The final Rust, release-build and lint logs are `/tmp/mx-phase02-rust-identity.log`, `/tmp/mx-phase02-build-final-confirmed.log` and `/tmp/mx-phase02-clippy-final-confirmed.log`.
The eight existing gates skipped `mx-backend-cmux-smoke`, `mx-claude-stop-autoarm-live-e2e`, `mx-codex-continuity-live-e2e`, `mx-cursor-live-e2e`, `mx-launcher-live-e2e`, `mx-pi-primary-live-e2e`, `mx-pi-primary-types` and `mx-send-daemon-marker-herdr-e2e`.
No gate was added or relaxed for this phase, and skipped optional tests are not reported as live validation.

### Live report environment evidence

`bash /tmp/mx-phase02-live.sh` exercised real Codex 0.151.0 and Cursor 2026.09.02-c22c1a3 against synthetic, owner-published canonical records in separate temporary homes.
Both provider processes exited 0 and their actual shell-tool report calls wrote accepted evidence with the expected task, attempt, generation and brief revision.
The script instructed each model to perform exactly one report command, without project changes, MCP or operational session start.
The final report-owner smoke summary is `/tmp/mx-phase02-live-h8qvqzs3/results.json`; provider logs and accepted evidence remain beside it.
This establishes real provider environment-to-reporter integration, not a full backend launch or restart trial.
Claude and Pi were absent from PATH, so no live result is claimed for them.

The project CLI smoke used real temporary repositories and exercised registration, listing, exact ID resolution, ambiguous aliases, three independent selections, ownership no-upgrade, metadata-only forgetting and missing-location refusal.
Its local evidence is `/tmp/mx-phase02-project-cli-um8s5h8o`.
The focused lifecycle shell suites use mocked harness/backend/provider responses with real filesystem and Git fixtures; these are not live model, forge or worktree-provider evidence.
The full guarded Herdr presentation/recovery test also passed independently with real Herdr 0.7.4 and Treehouse, a stub model harness and the default-session tripwire intact: 221,891 ms, no gate skip.
The final complete regression repeated the full lab matrix with the allocated-worktree identity guard and passed in 224,664 ms without a gate skip.
No live forge publication, human merge, Linux trial or packaged release activation is claimed.

## Boundaries retained for subsequent phases

Phase 03 owns built-in worktree acquisition, release, recovery and replacement of external provider calls.
Phase 04 owns durable inbox handling, full report transport integration, capacity scheduling and the remaining native-delegation enforcement surfaces.
It also owns automatic reconciliation of uncertain retained external-launch intents; local receipts alone do not make external execution exactly once.
Existing inherited-config Git/date observations and provider presentation serialization retain their specialized guards; the new task/receipt locks do not span harness or backend launch commands.
Phase 05 owns the named coordinator spawn operation, reliable parent-channel composition and end-to-end coordinator lifecycle.
Phase 06 owns publication evidence integration and the complete human-only merge backstop.
Phase 09 owns the executable transaction that upgrades existing homes.
Phases 10-12 own UI consumption, workspace discovery/entry and combined release trials.
These boundaries do not waive Phase 02's identity, validation or integration requirements.
No Phase 02 requirement remains unresolved after the final checks.
Phase 03 is ready to begin from this implementation; activation in real operational homes remains deferred to the combined release and migration phases.
