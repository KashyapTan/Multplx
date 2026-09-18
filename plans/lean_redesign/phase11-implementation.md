# Phase 11 implementation evidence

## Status and source boundary

Status: complete on 2026-09-18 under the recorded integration boundaries; changes are on `lean-phase11-workspace-entry` and are not yet merged.
Work started on branch `lean-phase11-workspace-entry` from clean merged Phase 10 commit `f991c3c3920f58c49ac7e740f04c96844cdeb476`.
The [phase plan](11-workspace-entry-and-project-discovery.html) assigns this work and [A9 in porting.md](../../porting.md#a9-launch-anywhere-project-discovery-and-one-shared-chat) owns the cross-phase contract.
Inspection covered the actual project/checkout registry, borrowed-checkout sync and cleanup, durable request owner and wake queue, session ownership, task portfolio, private home provisioning, launcher, shell adapters and installation boundary.
The architecture, Treehouse replacement and scoped-coordinator assessments supply rationale.
Root `AGENTS.md` remains absent and `AGENTS_E.md` remains dormant.
The excluded `firstmate/` directory and private operational homes are not validation inputs.

## Implemented surfaces

- Configurable roots, depth, exclusions and opt-in symlink traversal feed bounded discovery and an incremental cache.
  The canonical project catalog remains registration authority; discovery neither executes discovered content nor registers every candidate.
  Overlapping roots preserve the deeper remaining traversal budget, linked worktrees preserve checkout identity, missing roots remain explicit scan errors, and unborn/unversioned candidates retain their capability limits.
- Local registration and identity-proven location repair preserve borrowed checkout files and Git state.
  Accepted, not-yet-allocated work resolves a repaired location through its immutable checkout ID and retains its frozen starting commit.
  Repair does not relocate an already-created allocation whose common Git directory was moved.
- `mx task` freezes project, checkout, committed start and retry identity, routes through existing request/wake owners, and appears in the shared portfolio before execution.
  Canonical intake serializes task uniqueness and dependency-graph validation with durable acceptance, including concurrent callers of the lower-level request interface.
  Dirty files remain excluded and untouched; explicit working-change capture reports unsupported instead of silently stashing or including them.
- The launcher separates installed runtime, operational home and caller context, remembers the selected harness, and uses process-bound launch reservations and connection records.
  A supported exact tmux route reconnects to the existing owner; unavailable attachment reports the existing route without a competing launch.
  Dead-owner reconciliation preserves durable requests.
- Terminal presentation consumes canonical tasks, decisions and domains, with project search, keyboard navigation, task entry, explicit refresh, chat and MX Viz actions.
  Project selection affects future intake only, and domain submission uses the selected coordinator's route.
  An unrelated launch directory does not silently select a registered project or become a discovery root.
- Versioned platform packages include binaries and runtime assets with exact inventory, hash and mode verification, transactional publication and data-preserving removal.
  Ordinary installation uses the extracted binary without Rust, a source checkout or Treehouse.
  Packaged private persistent homes use the established non-Git runtime provisioning path.
  CI builds and verifies platform archives; no public release or live-home activation has occurred.
- README, getting started, configuration, the workspace/project reference, launcher/domain/task help and shell completions describe the installed entry path and one-chat contract.

## Acceptance mapping

| Assigned behavior | Evidence surface | Boundary |
| --- | --- | --- |
| Scenarios 19, 21, 25: nested discovery, local reuse, identity and preservation | `project_discovery`/`project_registry` Rust tests, `tests/mx-workspace-discovery.test.sh`, `phase11_workspace` integration | Real temporary Git/filesystem fixtures, including missing paths, overlap, symlink cycles, linked worktrees, nested repositories, ambiguity and dirty state. |
| Scenarios 20, 22: independent three-repository intake, retry, restart and outcomes | `phase11_workspace`, corrected `phase04_coordination`, canonical request-owner concurrency tests | Real request records, wakes, frozen revisions, exact-base Git allocations and independent completion artifacts with a synthetic runtime owner; no live model implementation or chat transcript is claimed. |
| Scenario 22: one owner and reconnect | Backend process/reservation tests, `tests/mx-launcher-connection.test.sh`, Cargo terminal wrapper | Real isolated tmux server and PTY with a synthetic harness; custom-socket attach and launcher-crash reservation, not a provider-authentication trial. |
| Scenarios 23, 26, 27: package-only entry and persistent homes | `tests/mx-release-package.test.sh`, launcher/shell tests and platform validation | Isolated local packages, real Git/private-home provisioning and deterministic harness/dependency sentinels; no public download or installed-user-home mutation. |
| Scenario 24: terminal interaction and shared facts | `workspace_tui` unit tests, terminal PTY fixture, shared snapshot tests | Rendering, controls, resize, canonical portfolio and independent-process survival; MX Viz retains Phase 10's separate browser evidence. |
| Assigned A11 integration | `phase11_workspace`, domain command help and TUI domain routing | Existing Phase 05 domain ownership is exercised alongside direct intake; selecting a project does not spawn a coordinator. |
| Relevant A8 cross-repository costs | [Measurement artifact](phase11-workspace-entry-performance.json), `tests/fixtures/measure-workspace-entry.py` | 1/5/10/20 local serial discovery, intake, snapshot and allocation trials with binary/source hashes; not live-agent throughput or the Phase 12 combined workload. |

## Exact checks and results

All assigned implementation and repository validation checks have passed.
The post-freeze `cargo build --release --workspace --locked` passed in 64 seconds.
The macOS release binary SHA-256 is `f29b1665e6629737151bd2e9e74450f19adb0d19928d5f3fdcff2bceb6c2c8fe`.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on the final tree. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed on the final tree. |
| `cargo test --locked --workspace` | Passed: 767 tests, 37 result groups, zero failures and zero ignored tests; `/private/tmp/mx-phase11-cargo-workspace-final3.log`. |
| `target/release/mx test-run --check-coverage` | Passed: 133 scripts, 112 accelerated, 11 serial, 10 Herdr, 133 manifest entries. |
| `target/release/mx doc-audience-check` | Passed after completion updates: 90 maintained surfaces and 476 local links. |
| `git diff --check` and `test ! -e AGENTS.md` | Passed; checkout restrictions preserved. |
| `cargo test -p multplx-cli --test phase11_launcher_terminal -- --nocapture` | Passed: both terminal/backend contracts; earlier event-synchronized fixture passed three consecutive runs. Real isolated tmux/PTY, synthetic harness. |
| `target/release/mx test-run tests/mx-launcher-connection.test.sh tests/mx-naming.test.sh` | Passed after the final terminal test update: both scripts, zero failures and no gates. |
| `MX_LAUNCHER_LIVE_E2E=1 tests/mx-launcher-live-e2e.test.sh` | Passed for installed Codex and Cursor executable probes through `mx launcher chat HARNESS --version` from an unrelated directory, with remembered-harness/caller display. |
| `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase11-isolation-proof.json` | Passed: 111 candidates per round, 634,669 ms, zero failed rounds, zero leaks and zero known-failure observations; verified JSON archived in `docs/mx-test-isolation-proof.json`. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-phase11-shell-passing.json` | Passed: 133 scripts, zero failures, eight declared gates, 376,336 ms. |
| Source-line coverage | Passed: 93.19 percent, 66,757 of 71,639 lines covered (4,882 missed), unchanged 93 percent gate and exclusions; `/private/tmp/mx-phase11-coverage-final3.log`. |
The unchanged source-line coverage threshold is 93 percent, distinct from shell inventory classification.
The clean coverage invocation is:

```sh
CARGO_TARGET_DIR=/tmp/mx-phase11-coverage-target cargo llvm-cov --locked --workspace --all-targets --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' --fail-under-lines 93
```

The aggregate's eight declared gates are optional cmux, Claude stop/autoarm, Codex continuity, Cursor live, launcher live, Pi live, Pi types and daemon-marker Herdr checks.
The launcher live gate was separately enabled for executable-version probes as described below.

The accepted isolation proof ran against binary `e01af9c3227e36348702467e1ff397290498004f535d31b1118a45197280131a` and the unchanged 133-script resource manifest.
Its manifest SHA-256 is `02854fc2822f06e8fd1193f8fdcda2f96a1e2b62012f08a3c985a8e6ab478edc`, with 664 declared conflict pairs.
The later connection-fixture family label does not change its declared `tmux-server` resource or the candidate partition.
The initial replacement attempt is excluded: both rounds passed but a binary replacement overlapped the run, and publication through macOS's `/tmp` symlink refused.
The accepted rerun used the real `/private/tmp` directory and one unchanged binary throughout both rounds.

Superseded interim failures are retained for provenance: the first full release suite reported 132 scripts, two failures and eight declared gates.
One failure used a stale release binary for a newly added package adapter assertion; the other correctly rejected the old isolation-proof manifest.
The first Rust run exposed two launcher assertion/fixture mismatches, and graph uniqueness subsequently exposed a Phase 04 fixture that reused one task ID for independent tasks.
Those fixtures now assert the intended contracts instead of weakening production checks.
A later 133-script aggregate found one naming-tripwire failure in a new test fixture title.
The fixture wording was corrected, the unchanged naming check passed, and the complete 133-script aggregate passed on rerun.
Terminal acceptance also exposed missing idle refresh and UTF-8 input handling; final event-synchronized PTY checks pass after both fixes.
A clean coverage run initially reached 92.69 percent and failed the unchanged 93 percent gate.
Follow-up tests exercise registry validation, corrupt durable-request projection and launch boundaries.
The terminal coverage wrapper now propagates the instrumented runtime consistently, and its navigation case exits normally so coverage counters survive; independent signal/crash ownership cases remain intact.
An intermediate full rerun was stopped early when this missing-profile cause was identified; it is not acceptance evidence.

## Linux/package validation

Linux validation used an actual copied filesystem tree, not a macOS bind mount, with Docker `--init` for child reaping.
The source stage came from `git archive HEAD` plus `git diff --binary HEAD` applied to that stage; private operational directories, `firstmate/`, `.git` and host build output were excluded.
The container was `mx-phase11-copy-final-20260918-v4`, from `rust:1.97-bookworm` image `sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97`.
It ran Debian 12/aarch64 on Linux 6.12.76-linuxkit with Rust/Cargo 1.97.1, Git 2.39.5, tmux 3.3a, Python 3.11.2 and zsh 5.9.

The copied-tree checks passed `cargo fmt --check`, `cargo test --locked --workspace`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo build --release --workspace --locked` and `target/release/mx test-run --check-coverage`.
The focused release command ran `tests/mx-release-package.test.sh`, `tests/mx-launcher.test.sh`, `tests/mx-launcher-shell.test.sh`, `tests/mx-launcher-connection.test.sh`, `tests/mx-workspace-discovery.test.sh`, `tests/mx-bin-runtime-inventory.test.sh`, `tests/mx-backend.test.sh` and `tests/mx-backend-tmux-smoke.test.sh` through `target/release/mx test-run`.
The final Linux workspace run passed 750 tests in 37 result groups, with zero failures and zero ignored tests.
All eight scripts passed, with no gates, in 35,314 ms.
After the final test-only additions, the complete current Rust source and terminal fixture were copied into the same isolated container.
Source timestamps were refreshed to force recompilation, avoiding reuse of binaries older than the copied source.
`cargo fmt --all -- --check`, `cargo test --locked --workspace` and `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` passed again: 769 tests across 37 result groups, zero failures and zero ignored tests.
The follow-up log is `/private/tmp/mx-phase11-linux-current-tests-rebuilt.log`, SHA-256 `aa46f4b90d0cfa37820ed7cf36c77dc292c9024347bf98d17a7afe71036c8d14`.
Production code and the measured release binaries were unchanged by these test additions.
This includes real isolated tmux/PTY routing with a synthetic harness, not model-provider execution.

An interim full Rust rerun after test instrumentation hit the existing MX Viz `snapshot_refresh_is_single_flight_and_does_not_hold_runtime_mutex` timing assertion (`reader did not start`).
It passed in the preceding complete run, on exact retry and in the final v4 complete run; the interim timing-flake observation remains recorded.
Linux also exposed a POSIX-locale mismatch in the retained shell composer classifier: wildcard removal split a multibyte prompt glyph.
The minimal literal-prefix removal now agrees with Rust without changing classification rules.
The final log is `/private/tmp/mx-phase11-linux-copy-final-validation.log`, SHA-256 `3e64ce6a480e2a9fa3c98180bc0f497b31e2a0f3865797a89dd9afa99e045f4d`.
The verified Linux package has 228 manifest entries and 224 runtime files; `/private/tmp/mx-phase11-linux-package-SHA256SUMS` has SHA-256 `eee0a48c6faf6ee5dffd7287f9d98f70eb93f4bf11bc7b673dd7198fc3fb5f6a`.
Its binary SHA-256 is `46ef1b37725669542a6e6d883bef819fc95423f4b41b562d9df8c482179d6fd5` and archive SHA-256 is `6e04d62af50f902d3791b458a75fbc3f47195f4ac3652fcbbf7963ff2eebc32c`.
The package excludes development-evidence JSON and the tracked historical document containing a private user path.
The package fixture uses real Git and filesystem operations; `gh` is only a dependency sentinel, and the Codex executable is synthetic.
The installer protects recorded supported consumers; unrecorded external processes reading runtime assets directly are outside that lifecycle boundary.

## Local cross-repository measurements

The following command generated the [recorded artifact](phase11-workspace-entry-performance.json) against the frozen release binary:

```sh
python3 tests/fixtures/measure-workspace-entry.py --binary target/release/mx --output plans/lean_redesign/phase11-workspace-entry-performance.json --overlap 'Final Phase11 local release on a shared workstation; the repository coverage run and routine desktop workloads may overlap, and workload isolation was not enforced.'
```

The artifact records host details, limits, raw observations and source/script/binary hashes.
Its runtime source-set SHA-256 is `ae52c6c0482a7ec7f84541d6540b321d76c8361ad7304ef178f19bcc5179b02a` over 260 files.
Each row verified exactly N discovered candidates, accepted requests, portfolio tasks, queued wakes and allocations.

| Repositories/tasks | Discovery ms | Intake median ms | Snapshot ms | Allocation median ms |
| --- | --- | --- | --- | --- |
| 1 | 94.049 | 410.654 | 57.262 | 430.716 |
| 5 | 283.927 | 438.021 | 62.994 | 365.404 |
| 10 | 490.913 | 361.772 | 68.799 | 336.952 |
| 20 | 951.188 | 337.231 | 63.616 | 353.082 |

These are local serial Git/process-startup costs under the recorded host load.
They do not measure model response delay, human review, network publication, review rework or useful live-agent completion throughput.
Those combined release measurements retain their Phase 12 ownership.

## Integration limits and remaining work

The real executable probes ran Codex CLI 0.151.0 and Cursor 2026.09.10-fd3934a with `--version` through the isolated adapter.
They establish executable routing only, not authenticated chat, transcript continuation or model-generated outcomes.
Claude and Pi were not live-tested in this phase.
Other-provider live trials retain the previously recorded deferral; no new provider capability is inferred from mocks.
Codex Desktop remains a host-tool integration, not a shell-callable backend.
Unsupported attachment reports the recorded conversation route; provider transcript resume remains provider-dependent.

The initial acceptance results above are local macOS and containerized Linux results.
The subsequent remote CI run and its follow-up fixes are recorded below.
Public release publication/download, deployment into a real operational home and the combined live-model 1/5/10/20 workload remain Phase 12 cutover work.
The checkout's prohibition on Multplx session start remains respected.
No assigned Phase 11 implementation or required repository check remains unresolved.
Phase 12 is ready to begin with the provider deferral, public release, real-home activation and combined live-workload boundaries above.

## PR 47 CI follow-up

The first remote run, [35354230611](https://github.com/KashyapTan/Multplx/actions/runs/35354230611), exposed two timing issues that the initial local runs had not reproduced.
The Linux Rust, both parallel behavior lanes and coverage job encountered an empty `/proc/<pid>/cmdline` while identifying a newly spawned launcher gate process.
The launcher now retries the strict identity read for at most one second while its FIFO remains closed, refuses persistent uncertainty, and reaps failed children.
The macOS Rust lane exposed a visualization fixture whose 20 ms cache lifetime allowed a legitimate third refresh before its assertion.
That fixture now explicitly ages the initial cache and retry timestamp without depending on a short sleep.
CI also explicitly installs the test tools used by the fixtures, including ripgrep, tmux, Python and zsh; missing ripgrep had caused static assertions to be bypassed in the earlier log.
The coverage threshold and exclusion expression are unchanged.

Follow-up validation passed:

- `cargo test --locked --workspace`: 768 macOS tests and 770 Linux tests, zero failures.
- `cargo build --release --workspace --locked`: passed on macOS and Linux.
- `CARGO_TARGET_DIR=/tmp/mx-phase11-ci-coverage cargo llvm-cov --locked -p multplx-cli --test dispatch_runtime --no-report`: all nine instrumented dispatch tests passed.
- Backend launcher identity tests: 11 passed on each platform, including injected transient recovery, persistent failure and early exit.
- `target/release/mx test-run tests/mx-cursor-adapter.test.sh tests/mx-launcher-shell.test.sh tests/mx-launcher.test.sh tests/mx-release-package.test.sh tests/mx-launcher-connection.test.sh tests/mx-viz.test.sh --jobs auto`: six passed, zero gates, 35,794 ms on macOS.
- The four Linux launcher/package suites passed, followed by 50 rapid Cursor and 25 rapid Codex launch repetitions with synthetic harnesses.
- The corrected visualization test passed 40 repetitions; all eight visualization unit tests passed.
- Formatting, all-target/all-feature Clippy, workflow YAML parsing, documentation classification and whitespace checks passed.

The original full coverage measurement and performance artifacts above describe the pre-follow-up build; the follow-up does not claim a new complete coverage measurement or remote green run.
The user will monitor the new CI run after this fix is pushed.

## PR 47 second CI follow-up

[Run 35369297490](https://github.com/KashyapTan/Multplx/actions/runs/35369297490) passed the Linux Rust lane and every behavior lane after the launcher fix.
Two checks remained: the macOS production-boundary suite hit a Viz shell-fixture race, and the coverage job was canceled at its 15-minute job limit.
Coverage setup consumed 80 seconds; the instrumented command then ran for 13 minutes 51 seconds before cancellation during the shared lifecycle contracts, after the Phase 11 instrumented contracts and subsequent suites had passed.
The coverage job now allows 30 minutes, consistent with the prior complete local run duration and still bounded.
The coverage command, tests, exclusions and 93 percent threshold are unchanged; the repository CI invariant checks the new cap.
The Viz shell fixture captures the failed conditional response before validating the twelve concurrent responses and metrics, so assertion subprocesses cannot consume its retry window.
Fixed cache-expiry and retry sleeps are replaced by observable-state predicates.
The original 200 ms refresh interval, 500 ms command deadline, two-second failure wait, exact reader counts and failed-response assertions remain unchanged.
Eight consecutive focused Viz suite runs passed; shell syntax, ShellCheck, workflow YAML parsing, documentation classification and whitespace checks passed.
The updated CI contract suite also passed, with the coverage command and threshold preserved.
The second follow-up will be pushed for the user to monitor; it does not claim a completed new remote coverage result.
