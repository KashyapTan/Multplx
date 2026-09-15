# Phase 03 implementation evidence

Status: planned - the hosted Herdr focus race is fixed and focused checks pass; refreshed isolation, full-shell and hosted CI validation remain in progress.
Branch: `lean-redesign-phase-03`, based on merged phases 01/02 at `8e0576ace6dddab031cd187d8f9c6d3997634f3a`.
The [phase plan](03-built-in-worktree-lifecycle.html) and [A10](../../porting.md#a10-built-in-git-worktree-lifecycle) own acceptance.
The implementation inspected and reused the actual Phase 02 task/project identities, filesystem transactions, locks and process-start identities.
No operational source-checkout session or private-home migration was performed, and root `AGENTS.md` remains absent.

## Implemented requirements

| Requirement | Implementation and evidence |
| --- | --- |
| Small resource owner | `lifecycle/worktree.rs` owns acquire, inspect/list, retain, release and explicit scoped prune; [worktree documentation](../../docs/worktrees.md) publishes JSON grammar and record ownership. |
| Exact project and base | Canonical common-Git identity coordinates borrowed checkouts across homes; detached allocations use the recorded full commit without fetch or source-checkout mutation. Real Git tests cover dirty borrowed sources, linked checkouts, remote-free repositories, branch collisions and two-home isolation. |
| Durable acquisition | A process-identity-bound repository reservation precedes Git mutation; repeated requests reconcile durable receipts. Rust and CLI fault injection cover reservation, Git-success-before-publication and incomplete directories. |
| Launch integration | Allocation precedes endpoint creation; tmux, Herdr and cmux receive the returned path, with no shell acquisition or cwd polling. Task metadata and launch intents bind exact allocation/attempt identity. |
| Replacement and task fencing | A proven replacement preserves its previous worktree and unfinished progress while issuing a new lease/generation. Spawn and teardown share a per-task guard; child cleanup rereads metadata under its matching guard. Real lock-holder tests prove contended first launch and replacement cannot publish an intent or touch an endpoint. |
| Persistent homes | Home seed provisions private directories using installed assets, without runtime/project clones, and remembers canonical borrowed project references. Deliberate persistent Git-backed homes bind the same allocation API. Zero-process reservations remain owned. |
| Home interruption and retirement | Home receipts bind lease, generation and directory inode; seed rollback preserves the reserved home and restores parent artifacts. Private retirement journals and archives material; Git-backed homes stay retained in place. Tests cover replaced paths, symlinks, reused generations and interrupted rename. |
| Safe disposition | Exact lease/generation/attempt and directory identity fence stale callers. Dirty, untracked, unknown ignored, unlanded, open-PR and uncertain occupants stay retained; existing squash/replayed landing and stale Git-lock proofs remain tested. No force/reset/clean or broad process killing is added. |
| Scoped prune and recovery | Preview and application check one owned disposed allocation; Git removes only the exact proven target. Both removal interruption boundaries reconcile on retry. Foreign worktrees, primary checkouts, persistent leases, corrupt records and unexplained paths are excluded. |
| Bounded observation | Git and occupant probes bound time/output and reap their owned process groups on timeout. Short repository/registry locks do not span Git work; per-task guards serialize only the affected lifecycle. Doctor and snapshots consume canonical observations. |
| Migration foundation | Explicitly scoped v2.0.1-style readers preserve missing lease facts as unknown and reject malformed/unknown metadata. Relocation receipt validation is inert; Phase 09 owns live ownership proof, wrapper quiescence, transfer and reference updates. |
| Dependency retirement | Removed installer implementation/dispatch, automatic download, pin, probe, bootstrap and CI requirements. Provider/download sentinels prove the new normal path does not invoke them. Help, command inventory, operational references and dependency/setup/lifecycle docs now describe built-in operations. |
| Phase 05 foundation | Private homes and canonical worker allocations can share one borrowed project across homes without copying it; named coordinator lifecycle remains assigned to Phase 05. |

## Final-tree checks

Local diagnostic logs are under `/private/tmp/mx-phase03-*`; permanent acceptance evidence is summarized here and in the linked JSON artifacts.
Results below distinguish the final release/source from earlier development runs.

| Exact check | Result | Evidence and limits |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Passed | Final Rust source after formatting |
| `cargo build --release --workspace --locked` | Passed, 47.28s | `mx-phase03-herdr-lock-build.log`; fingerprint below |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed, 4.59s | `mx-phase03-herdr-lock-clippy.log`; matches CI flags |
| `cargo test --locked --workspace` | Passed, 540 tests, no failures or ignored tests | `mx-phase03-herdr-lock-rust.log`; instrumented-only shell contracts execute in coverage below |
| `cargo audit --deny warnings` | Passed | 70 dependencies; 1,246 RustSec advisories loaded; `mx-phase03-final-audit.log` |
| `target/release/mx test-run tests/mx-spawn-worktree-settle.test.sh --json /private/tmp/mx-phase03-task-lock.json` | Passed, 44.930s, no skips | Real Git and live production lock owner; endpoints/harness mocked |
| `target/release/mx test-run --check-coverage` | Passed, 128 fixtures | 107 accelerated, 11 serial and 10 Herdr; inventory/partition proof, not line coverage |
| `target/release/mx shadow-diagnostic` | Passed | Runtime boundary ready |
| `target/release/mx doc-audience-check` | Passed; final evidence/status edit will be rechecked | Documentation classifications and local links |
| `for script in bin/*.sh bin/backends/*.sh; do bash -n "$script" || exit; done` | Passed | Toolbelt Bash syntax |
| `[ "$(readlink .claude/skills)" = "../.agents/skills" ]` and `git diff --check` | Passed | Checkout invariants and whitespace |
| `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase03-isolation-proof.json` | Passed, 106 candidates x 2 rounds, 494.125s | 0 failed rounds, 0 leaks, 0 known-failure exceptions; exact JSON archived in `docs/mx-test-isolation-proof.json` |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-phase03-clean-all.json` | Passed, 120 passed, 0 failed, 8 declared skips; 289.254s | `mx-phase03-clean-all.log/json`; all 128 fixtures accounted for, all 10 real-Herdr fixtures passed. Includes the corrected owner-home and hermetic doctor fixtures. |
| Exact CI line-coverage command below | Passed, 93.01% (45,485 lines, 3,178 missed), all tests passed | `mx-phase03-final-coverage.log`; fresh run with no `--no-clean`; threshold and exclusions unchanged. The subsequent doctor fixture isolation correction passed separately under instrumentation (`mx-phase03-doctor-hermetic-instrumented.log`) and in the release runner (3.201s, `mx-phase03-doctor-hermetic.log/json`). |
| Hosted Linux/macOS CI | Rust, portable lanes and exact Linux coverage passed; Herdr focus regression fixed and rerun pending | [Run 34970863861](https://github.com/KashyapTan/Multplx/actions/runs/34970863861) passed Linux coverage at 93.00% (45,561 lines, 3,189 missed). The earlier doctor PATH leak was corrected; the subsequent Herdr focus failure prompted the session-lock fix below. |

```sh
cargo llvm-cov --locked --workspace --all-targets \
  --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' \
  --fail-under-lines 93
```

The fresh coverage run passed 540 tests across 24 test binaries with no failures or ignored tests.
The new worktree module measured 96.19% line coverage (1,787 lines, 68 missed), and teardown measured 94.35% (2,940 lines, 166 missed).
The real-Git worktree, private-home lifecycle and daemon safety fixtures run through the instrumented command boundary.
Additional owner tests cover malformed and stale tokens, replaced directories, interrupted journals and conservative cleanup.
A nine-case isolated Git/forge failure matrix verifies conservative refusal and the exact command trace; matching-origin independent clones still cannot authorize unknown-path deletion.
Native CLI tests cover single-checkout refusal, bound launch intents, exact cmux paths and failures before and after metadata publication using mocked endpoint/harness transport.
The Herdr compatibility fixture uses isolated real Git repositories to verify legacy runtime fast-forward and preservation of unfinished home files without changing the borrowed source.
Its separate packaged non-Git runtime case verifies the compatibility warning, unchanged home HEAD/charter and absence of invented ownership; endpoint/harness transport remains mocked.
Linux CI explicitly installs `lsof`; neither the 93% threshold nor the existing exclusion expression was relaxed.
Isolation proof, full shell regression and instrumented coverage run sequentially because separate runner processes do not share their resource scheduler.
The archived isolation proof preceded the final coverage-only assertion additions; its manifest and production binary are unchanged.
The final full shell and instrumented runs exercise those additional assertions.

Release itself is one atomic disposition-record publication, with no separate file-removal effect; the shared filesystem fault test verifies old/new visibility before and after rename.
The external removal-before-completion-record boundary is covered by `Fault::AfterRemoval` and the real CLI `remove-git` crash: the durable record remains `removing`, and retry reconciles Git inventory before recording `removed`.
Endpoint-before-metadata failure preserves the allocation and bound launch intent; a retry refuses uncertain duplicate execution, while Phase 04 owns convergent delegation replay.

## Session integration evidence

Hosted CI exposed a concurrent native Herdr teardown focus race: projected pane close did not hold the shared session presentation lock.
Native teardown now holds that lock across endpoint proof, focus capture, exact pane close, restoration and journal retirement.
The deterministic live lock-owner assertion failed against the old release in 49.466s and passed with the fix in 166.896s (`mx-phase03-herdr-lock-before.log/json`, `mx-phase03-herdr-lock-after.log/json`).
Portable instrumented tests also prove malformed-session and held-lock refusal preserve metadata, journals, allocation paths and focus without displacing the lock owner (`mx-phase03-herdr-session-lock-instrumented.log`).

The live Herdr projection test passed in 162.03s after adding durable holding-pane quiescence (`mx-phase03-herdr-quiesce-live.log/json`).
It verified concurrent homes, repeated restoration, stable worker path, preserved unfinished files, an advanced lease/generation, cleared holding receipt, focus, cleanup and the default-session tripwire.
Nine focused Herdr receipt tests passed; the final full Rust run includes those cases.
Corrupt, foreign or uncertain topology refuses recovery without transferring ownership.
The 300.105s complete-shell run rechecked the task-fenced release: the strengthened presentation fixture passed in 173.337s, including exact retained allocation/intent and duplicate-retry refusal after endpoint failure.
Its separate workspace-per-home fixture failed because teardown did not select the owning home.
After selecting the exact owner and eliminating duplicate fixture cleanup, its focused live rerun passed all 10 assertions in 8.174s with no cleanup warning (`mx-phase03-workspace-owner-final.log/json`); the final complete-suite rerun passed all available fixtures in 289.254s (`mx-phase03-clean-all.log/json`).
That clean run included all ten live Herdr fixtures; the strengthened presentation test passed in 164.185s.
The model/composer helper is scripted: this is live session transport evidence, not live model execution or task-quality evidence.
cmux is unavailable on this host; its existing live gates remain explicit, separate from deterministic adapter/launch checks.

## Final-binary worktree cost trial

The reproducible [measurement script](../../tests/fixtures/measure-worktrees.py) produced [raw samples, disk observations and fingerprints](phase03-worktree-costs.json) on macOS 26.6.2 arm64 with Git 2.55.0.
The release SHA-256 is `5eee2c44245dea9d2a6954d45a14360e535fac335c6877ca603fcaa499a5f34b`.
The script fingerprint and host load are retained in the artifact.
The trial ran from 12:57:35 to 12:58:44 UTC on 2026-09-15, with a local-only temporary repository containing a 1 MiB tracked file, two alternating owner homes and one serial trial at each task count.

```sh
python3 tests/fixtures/measure-worktrees.py --binary target/release/mx --output plans/lean_redesign/phase03-worktree-costs.json --overlap 'Final Phase03 release with serialized Herdr teardown on a shared workstation. Rust validation and live Herdr checks may overlap; workload isolation was not enforced.'
```

Rust validation and live Herdr checks overlapped; isolation proof and the full shell suite were subsequently sequenced separately.
The artifact preserves the invocation's conservative overlap description and all timing samples unchanged.

| Allocations | Median acquisition (ms) | Median release (ms) | Median prune application (ms) |
| --- | ---: | ---: | ---: |
| 1 | 337.071 | 402.173 | 438.829 |
| 5 | 316.464 | 396.694 | 420.925 |
| 10 | 305.177 | 390.893 | 426.351 |
| 20 | 298.521 | 389.914 | 404.440 |

Setup measures Git top-level/base/status readiness at the allocated directory and excludes endpoint/model startup.
At 20 allocations, logical disk use rose from 1,085,023 to 22,109,131 bytes, then fell to 1,121,091 after safe prune, retaining durable records.
Every 262,145-byte ignored-content fixture remained retained, each fallback used a distinct fresh path without importing that content, and every borrowed source remained unchanged.
Provider and download sentinels were untouched.
Fresh paths are the implemented conservative policy; there is no idle warm pool, and same-request active reconciliation is not warm reuse.
These small, serial, loaded-workstation observations establish costs, not concurrent throughput or a claimed velocity benefit.
Combined model/task-quality and 20-session release trials remain Phase 12 work.

## Remaining work and phase boundary

The initial complete shell run exposed obsolete fixtures and real safety/recovery regressions; fixes were verified with focused checks and the final Rust run.
A separate host executable-startup stall was diagnosed before application execution and later recovered; no operating-system protection was changed.
Those earlier results are not used as substitutes for the final pending gates above.
The first final coverage attempt passed the Phase 03 instrumented lifecycle checks, then stopped on an existing workflow artifact-pointer fixture using a lexical macOS temporary path.
The fixture now uses its physical temporary path while keeping the exact assertion; all 11 focused checks and the full instrumented test run then passed.
That completed coverage run measured 92.35%, below the unchanged 93% requirement.
Strengthened lifecycle and CLI tests then passed a fresh exact gate at 93.01%; no threshold or exclusion was relaxed.

The isolation proof is archived with manifest SHA-256 `afee0940e7b2037ad8116df2c48422d674100574ce9fa547c2a7c9c09a100c0f` and 647 conflict pairs.
The complete local shell suite and unchanged line-coverage gate passed.
[PR #39](https://github.com/KashyapTan/Multplx/pull/39) is published as a draft while final hosted CI runs.
Phase 03 remains planned until these checks pass; Phase 04 readiness is not yet declared.
Phase 04 owns broader durable delegation/inbox recovery, Phase 05 owns named coordinator composition, Phase 09 owns executable legacy transfer, and Phase 12 owns release activation.
No requirement assigned to this phase is waived or silently deferred.
