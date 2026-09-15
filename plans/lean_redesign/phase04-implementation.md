# Phase 04 implementation evidence

## Status and source boundary

Status: complete under the user-approved acceptance scope; implementation, repository validation and live Codex CLI verification pass, with other provider trials deferred.
Work started on 2026-09-15 on `lean-redesign-phase-04` from `bee67f6`, the merged Phase 03 prerequisite.
The checkout was clean when the branch was created.
Implementation commit: `fb307a3`.
Review: [Phase 04 PR #40](https://github.com/KashyapTan/Multplx/pull/40).
The [phase plan](04-free-delegation-and-coordination.html) allocates this work and [porting.md](../../porting.md#accepted-architecture-contract) owns the shared contracts.

On 2026-09-15 the user explicitly accepted Codex and Codex CLI verification for this phase and deferred live testing of the other supported providers until after all phases are implemented.
Claude, Cursor and Pi support changes remain implemented and contract-tested; their unexecuted live trials are deferred evidence, not passing results.
The existing cmux availability limit remains recorded below.
This acceptance decision permits the Phase 04 PR and removes the deferred provider trials from the Phase 05 prerequisite gate.

The prerequisite inspection confirmed canonical task, attempt, brief, project, allocation and lineage records in the existing task metadata owner.
It also confirmed Phase 03's exact-path worktree acquisition and retained interrupted-launch intent.
Before this phase, the wake CLI still treated successful stdout publication as drain completion, the native delegation guard remained active, and interrupted endpoint launch refused uncertain retries instead of reconciling them.

Root `AGENTS.md` remains absent and `AGENTS_E.md` remains dormant.
Development does not start an operational Multplx session, inspect the excluded `firstmate/` directory or use private operational homes as fixtures.
All mutation checks must use isolated test homes and repositories.

## Acceptance inventory

| Requirement | Implementation and verification status |
| --- | --- |
| Free CLI and native delegation; generated hook/settings audit | Implemented; adapter and generated configuration checks pass; other provider live trials are deferred by the user as described below. |
| Canonical native observations and truthful unavailable recovery | Implemented; current/stale identity, provider collision, restart and result non-completion tests pass; live Codex evidence recorded. |
| Nested task, parent and project binding; independent-child continuity | Implemented; a persistent child services the root queue and launches a grandchild through the CLI with exact lineage. |
| Durable claim, disposition, acknowledgement and waiting continuations | Implemented; core wake and verified-owner CLI failure/recovery tests pass. |
| Correlated, attempt/revision-bound messages and validated destinations | Implemented; shared envelopes, foreign/stale refusals, pending replies and retry tests pass. |
| Stable action IDs and reconciliation with worktree allocation receipts | Implemented; exact-attempt retry, interrupted launch, retained allocation and qualified cleanup tests pass. |
| Bounded custom/forge checks and responsive wake ingestion | Implemented; stalled command, held pipe, excessive output, nudge, partial-result ordering and later-batch progress checks pass. |
| Durable root-scoped capacity, priorities, dependencies and fair progress | Implemented; shared receipts, resource fit, aging, priority, cycle and restart tests pass. |
| One-owner connection and repeat-safe partial multi-repository intake | Implemented; concurrent duplicate clients, three registered projects, partial acceptance and frozen revision replay pass. |
| Phase 05 inbox/outbox and shared-admission primitives | Implemented through existing state owners; coordinator lifecycle composition remains Phase 05. |
| CLI help, maintained documentation and focused interface tests | Implemented; documentation, interface and full shell checks pass. |
| Required repository validation and live provider evidence | Repository checks and live Codex CLI pass, including the unchanged 93 percent coverage gate; other provider live trials are user-deferred and unavailable cmux remains a declared limit. |

## Validation evidence

### Implemented surfaces

The existing [`WakeQueue`](../../crates/multplx-core/src/wake.rs) now owns stable inbox receipts, exact process claims, disposition history, acknowledgements, waiting continuations and abandoned-owner recovery.
The CLI exposes those transitions through `mx wake`; the legacy drain adapter claims work without completing it.
[`operational_input.rs`](../../crates/multplx-domain/src/operational_input.rs) supplies durable terminal requests and shared message envelopes, and pending replies retain their correlation through the existing lifecycle owner.
See [durable coordination](../../docs/durable-coordination.md) for operator commands and the separate delivery, response and completion facts.

[`headroom.rs`](../../crates/multplx-backend/src/headroom.rs) owns root-scoped queue and admission receipts with priority, aging, dependencies and named resources.
[`spawn.rs`](../../crates/multplx-domain/src/lifecycle/spawn.rs) retains launch actions against the Phase 03 allocation and reconciles exact task, attempt, owner and endpoint identity before retry.
Nested dispatch resolves its root through canonical ancestry instead of trusting a root override.
Active receipts count descendant and persistent sessions against that root budget, with exact root endpoint deduplication.
Regression checks exercise a one-session root budget exhausted by a child-home receipt and a reservation becoming active between capacity evaluation and locked admission.
See [dispatch capacity](../../docs/configuration.md#dispatch-capacity-configapi-capacity--configadmission-capacityjson--statedispatch-queue) for configuration and replacement commands.

The native observer uses provider lifecycle events or structured tool results and appends canonical observations without advancing task completion from prose.
Generated Claude, Codex and Cursor launch settings install observation hooks; the Pi extension consumes its supported extension event surface.
The former pre-tool guard remains an allowing compatibility adapter.
[`supervision.rs`](../../crates/multplx-cli/src/supervision.rs) runs slow checks concurrently with individual deadlines, bounded output and partial-result retention.
The integrated checks below verify these changes within their stated evidence boundaries.

`cargo test -p multplx-core wake --locked` passed 15 focused tests during component implementation.
The cases include one-owner claims, recovery after display, disposition before acknowledgement, waiting history and continuation, duplicate ingestion and a read-only unfinished count.
This component result does not establish CLI integration or phase completion.
Final commands, results, corrected failures and evidence limits are recorded below.

### Final combined-tree checks

The acceptance reruns use the `/private/tmp/mx-phase04-accept2-*` shell and interface logs and `/private/tmp/mx-phase04-accept3-*` Rust and coverage logs.
The only change after the successful full shell run is a test-only wake regression for conflicting republication and foreign acknowledgement; production code is unchanged.
The macOS host is arm64 macOS 26.6.2 with Rust 1.97.1.
The Linux checks use an isolated Docker source copy with Rust 1.98.1, `LANG=C.UTF-8 LC_ALL=C.UTF-8`, two Rust test threads and `tini -s` to reap test children.
The container has the declared shell fixture dependencies, including Node, and container-only Git safe-directory settings for `/work` and `/work/.git`.
The added daemon fixture disables system Git configuration, so its initial clone refused the copied Git metadata's host ownership; matching the isolated `/work` and `/work/.git` ownership to the container user corrected setup and the unchanged fixture passed.
No timeout assertions, coverage exclusions or acceptance thresholds were relaxed.
The private Linux container was stopped after its checks, with its logs retained.

| Check | Combined-tree result |
| --- | --- |
| macOS formatting, strict Clippy, full Rust suite and release build | Passed; 602 Rust tests, zero failures and zero ignored. |
| Linux formatting, strict Clippy, full Rust suite and release build | Passed; 604 Rust tests, zero failures and zero ignored. |
| Linux selected shell suite | All 16 passed across the main run and the daemon fixture setup correction; no gate skips. |
| Test inventory | Passed: 128 fixtures, 107 accelerated, 11 serial and ten Herdr. |
| Maintained documentation | Passed: 80 surfaces and 406 local links after the user-approved acceptance and contributor-context update. |
| Pi strict extension types | Passed against Pi 0.85.1 with the explicit isolated package override below. |
| macOS full shell suite | Passed: 128 fixtures, zero failures, eight declared gate skips, 326.743 seconds; all eleven phase-named fixtures passed. |
| Unchanged 93 percent Rust coverage gate | Passed: 93.02 percent, 53,003 lines, 3,699 missed; all 602 instrumented tests passed with zero failures and zero ignored. |

Acceptance rerun two passed every test but failed coverage at 92.99 percent: 52,921 lines and 3,712 missed.
One additional test-only regression verifies exact republication after inbox ingestion, conflicting payload and sequence rejection, and foreign acknowledgement fencing.
The fresh full run above passed with the existing threshold and exclusion expression unchanged.

The eight full-suite skips are the real cmux smoke, Claude stop/autoarm, Codex continuity, Cursor live, launcher live, Pi primary live, Pi strict types and Herdr daemon-marker fixtures.
Their skips are preserved as skips rather than counted as live evidence.
The separate strict Pi check and production tmux/Codex trial supply only the explicitly described evidence for those surfaces.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo build --release --workspace --locked
target/release/mx test-run --check-coverage
target/release/mx test-run --all --jobs auto
target/release/mx doc-audience-check
cargo llvm-cov --locked --workspace --all-targets --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' --fail-under-lines 93
```

The Linux run repeats formatting, strict Clippy, the full Rust suite and the release build.
Its additional shell command is:

```sh
target/release/mx test-run \
  tests/mx-subagent-pretool-check.test.sh tests/mx-cursor-adapter.test.sh \
  tests/mx-report.test.sh tests/mx-report-mcp.test.sh \
  tests/mx-operational-input.test.sh tests/mx-pending-reply.test.sh \
  tests/mx-send-strict.test.sh tests/mx-wake-queue.test.sh \
  tests/mx-watcher-lock.test.sh tests/mx-watch-triage.test.sh \
  tests/mx-signal-precedence.test.sh tests/mx-dispatch-queue.test.sh \
  tests/mx-spawn-worktree-settle.test.sh tests/mx-daemon-lifecycle-e2e.test.sh \
  tests/mx-spawn-dispatch-profile.test.sh tests/mx-backend.test.sh --jobs auto
```

The explicit Pi check uses the isolated package override:

```sh
PATH=/private/tmp/mx-phase04-tools/bin:$PATH \
MX_PI_PACKAGE_DIR=/private/tmp/mx-phase04-tools/lib/node_modules/@earendil-works/pi-coding-agent \
bash tests/mx-pi-primary-types.test.sh
```

### Integration checks during implementation

These are diagnostic checkpoints on a changing tree, not final acceptance results.

| Check | Result and interpretation |
| --- | --- |
| `cargo check --locked --workspace` | Passed after correcting initial compile errors in bounded supervision and endpoint teardown integration. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed at the second integration checkpoint; the first run identified a redundant path conversion, which was removed. |
| `cargo build --release --workspace --locked` | Passed at two integration checkpoints before the corresponding shell runs. |
| `cargo test --locked --workspace --no-fail-fast` | Passed at integration checkpoint eight before the subsequent concurrent-client and partial-check regression additions. |
| Eleven phase-named shell checks through `mx test-run --jobs auto` | Initial run: eight passed and three failed on obsolete ownerless drain fixtures. |
| `target/release/mx test-run --all --jobs auto` | Initial run: 128 fixtures, 24 failures and eight declared skips; root-parent queue validation accounted for many failures, with additional wake-fixture and concurrent-check issues requiring correction. |
| Third full shell diagnostic against release checkpoint five | 128 fixtures, 13 failures and eight declared skips; remaining failures identified path normalization, qualified cleanup, legacy message handling and wake-fixture/cadence integration. |
| `target/release/mx test-run --check-coverage` | Passed: 128 fixtures, 107 accelerated, 11 serial and ten Herdr; this checks test inventory, not Rust line coverage. |
| `target/release/mx doc-audience-check` | Passed at the initial documentation checkpoint: 79 surfaces and 397 local links. |
| `cargo audit --deny warnings` | Passed: 70 dependencies checked against 1,246 loaded advisories. |
| Pi strict no-emit typecheck with isolated Pi `0.85.1` and TypeScript | Passed through the existing shell fixture, including the native observation extension; the normal runner strips the private package override and reports a skip instead. |
| Linux Rust tests in an isolated changed-tree container | Diagnostic run reached queue integration failures; the container requires a subreaper for orphaned-process checks, which is supplied by `tini -s`. |
| Concurrent duplicate submission and partial three-project CLI test | Exposed lock contention in an instrumented run; bounded transition retry and per-request serialization corrected the focused test. |
| Full instrumented workspace run with the unchanged CI exclusions | All tests passed; line coverage was 92.30 percent (51,358 lines, 3,956 missed), below the required 93 percent, so this check failed and further failure/recovery coverage is required. |
| Combined macOS candidate checks | Formatting, strict Clippy, 582 Rust tests and the release build passed; the subsequent full shell run had two fixture failures, both corrected in focused reruns. |
| Linux candidate checks with UTF-8 locale and two test threads | 584 Rust tests and release build passed with unchanged timeout assertions; all 13 selected shell checks passed across the initial run and a watcher rerun after installing its missing Node fixture dependency. |

The unchanged CI line-coverage threshold is 93 percent with the existing exclusion expression.
Early instrumented runs stopped on an in-flight test signature change and an obsolete queue-schema assertion; neither establishes a coverage result.
Local diagnostic logs use the `/private/tmp/mx-phase04-*` prefix.

### Fixture isolation correction

Two older backend fixtures supplied temporary state paths but omitted `MX_HOME`.
With root-scoped admission enabled, those diagnostic runs could use the checkout's admission directory and capacity configuration.
Their results do not establish isolated acceptance.
The fixtures now set explicit temporary homes, and their affected checks must be rerun against the combined release.
Cleanup is limited to exact test receipt paths whose task identity, temporary owner state and timestamps establish that this session created them; uncertain or unrelated state is preserved.
That audit identified six test-created admission receipts and moved exactly those files to `/private/tmp/mx-phase04-checkout-artifacts/`.
No other state records or locks were removed.

The Linux legacy composer comparison requires a UTF-8 locale for its multibyte shell pattern.
The isolated comparison passes with `LANG=C.UTF-8 LC_ALL=C.UTF-8`; the initial C-locale mismatch remains diagnostic evidence and did not require a production source change.

## Live integration boundary

| Surface | Evidence boundary |
| --- | --- |
| Canonical state, recovery and transport contracts | Rust and isolated shell failure-injection checks; fake harnesses and forge responses do not establish provider behavior. |
| tmux with Codex | Real production spawn, authenticated Sol High native child, accepted lifecycle hooks and explicit durable report, described below. |
| Herdr | Installed backend integration fixtures exercise real terminal lifecycle and presentation; their synthetic harnesses do not establish model behavior. |
| cmux | Mock transport contracts are exercised; the real smoke fixture is gated because cmux is unavailable. |
| Claude | Exact generated settings load in the installed CLI; native execution is blocked by missing authentication. |
| Cursor | Generated plugin launch reaches the authenticated CLI; requested model execution is refused by the account's Auto-only entitlement. |
| Pi | Strict extension types and installed-runtime loading pass; model execution is blocked by missing credentials. |

Initial executable inspection found Codex CLI `0.151.0`, Cursor Agent `2026.09.10-fd3934a`, Herdr and tmux.
The installed Codex and Cursor CLIs report authenticated sessions.
Claude Code, Pi, cmux and `tsc` were not available on the initial PATH.
Availability and authentication checks alone are not native delegation, observation or recovery evidence.
The official [Codex hooks documentation](https://learn.chatgpt.com/docs/hooks) describes the structured sub-agent lifecycle events exercised in the live checkpoints below.

Isolated temporary installations subsequently supplied Claude Code `2.1.272`, Pi `0.85.1` and TypeScript without changing the user's configured tool installation.
Claude's authentication status reported `loggedIn: false`.
Pi's credential readiness checks for `openai-codex`, `anthropic` and `openai` each reported `credentials_not_configured`.
Those missing credentials block live model trials for these providers until authenticated runtimes are available.
Pi's [documented default tool surface](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/README.md) does not include a native sub-agent tool; its extension tool-call and tool-result events are the supported observation surface.

Actual installed-runtime load checks in `/private/tmp/mx-provider-load.Di14hM` reached those authentication boundaries without a settings or extension loading error.
Claude `2.1.272` loaded the exact generated observer-only settings before reporting that it was not logged in.
Pi `0.85.1` loaded the tracked native-observation extension before reporting that no API key was available.
These checks establish configuration loading only; they do not substitute for native-child execution.

### Live Codex checkpoints

Codex CLI `0.151.0` with `gpt-5.6-sol` and high reasoning completed an unbound native-child trial and a canonical task-bound generated-launch replay.
The unbound trial used `/private/tmp/mx-phase04-codex-live-8vydp5p2`, requested one arithmetic child and returned `42`.
Actual provider hooks recorded matching start and result events with one child identity, the parent session identity and a null task identity.

The canonical replay used `/private/tmp/mx-phase04-canonical/fixtures/codex-obs-codex-75248` and the launch command captured from the production spawn path in an isolated fixture.
The replay replaced only the fixture executable search path and selected noninteractive ephemeral Codex execution with its generated observer and report configuration.
Native evidence was accepted for `obs-codex-75248`, its existing attempt, generation 1 and brief revision 1.
The task's schedule and attempt remained unchanged and no status file was created.
The provider initially reported a thread lookup error before creating a child, then completed one successful native-child retry.
Ephemeral execution did not supply a usable transcript artifact; child identity remains correlation evidence and does not imply resumability.

These are live provider/hook checks using a generated command and fixture endpoint metadata.
These replay checks alone do not establish a live tmux endpoint launch or Claude, Cursor or Pi model behavior.

A later production spawn trial used real tmux in fresh isolated homes at `/private/tmp/mx-phase04-prod-codex.Pdn49p` and `/private/tmp/mx-phase04-prod-codex2.HMcvyl`.
The generated command arrived truncated before the shell entered its interactive input mode, and Codex did not start.
The fix writes the exact command to the attempt-qualified `tasktmp/launch.sh` with mode `0700` and submits only its short quoted path to tmux.
An 8 KiB byte-preservation and file-mode test passes.

The corrected production trial used `/private/tmp/mx-phase04-prod-codex3.Nug5JT`, task `livecodex121451`, real tmux, Codex `0.151.0`, and `gpt-5.6-sol` with high reasoning.
After the isolated repository and hook trust prompts, exactly one native child produced accepted start and result events at `16:15:39Z` and `16:15:44Z` on 2026-09-15.
Both events bound to child `01a0a5da-3b57-7e50-9651-3dfddcae1098`, the same parent session/turn, attempt generation 1 and brief revision 1, with session-bound recovery.
The result artifact contained `42` and `LIVE_NATIVE_CHILD_DONE` on separate lines.
An explicit task report produced status, evidence, a shared message outbox receipt and a wake; the native-child result itself did not advance the canonical attempt or schedule.
The fixture retains before/after metadata, spawn logs, result artifacts and hook/report evidence.
Only its private tmux server was stopped after verification.

### Cursor trial boundary

The corrected generated Cursor command includes `--plugin-dir` and was replayed in `/private/tmp/mx-phase04-canonical/fixtures-v2/cursor-obs-cursor-78517` with `gpt-5.6-sol-high`.
The authenticated CLI initialized but refused model execution because this account permits Auto only.
No native-child lifecycle evidence was produced.
The model listing alone did not establish entitlement, and this failed trial does not count as native delegation verification.

## Remaining work and next phase

The assigned implementation, documentation, CLI help, failure-injection, concurrency, adapter-contract and repository checks are complete.
The user will test the other providers after all phases are implemented; the deferred trials require authenticated Claude and Pi runtimes and a Cursor model permitted by the account.
The requested Cursor Sol High model is unavailable under the current entitlement; permission to substitute Auto has not been given.
The real cmux smoke remains unavailable on this host, while its mocked transport contract passes.
Execute those isolated live trials and update this evidence during the later validation pass; they are not claimed as verified by this PR.
Phase 05 is ready to begin once this Phase 04 PR is merged, under the user-approved acceptance scope.
Coordinator lifecycle composition remains Phase 05 work; publication remains Phase 06 work; real-home migration and release activation remain Phases 09 and 12.
