# Phase 05 implementation evidence

Status: complete for the assigned Phase 05 scope; repository validation and the required local-session evidence pass.
Branch: `lean-redesign-phase-05`, based on merged Phase 04 at `19ce383`.
The [phase plan](05-scoped-sub-orchestrators.html) and [A11](../../porting.md#a11-scoped-sub-orchestrators) own the requirements.
The implementation session inspected the Phase 01-04 code, their evidence, the architecture assessment and the scoped sub-orchestrator assessment before editing.
The source checkout remains inactive: root `AGENTS.md` is absent, no operational session was started here, and private operational homes and the excluded `firstmate/` directory are not implementation inputs.

## Requirement and verification ledger

| Requirement | Implementation and verification status |
| --- | --- |
| Public named spawn and private home | Implemented in CLI/domain spawn with explicit project/idea scope, independent persistence, request identity, common harness options and truthful JSON dispositions. |
| Domain identity and accepted charter | Implemented in the canonical task record and domain owner with pre-launch parent/root/scope binding, immutable accepted charter revisions and original evidence pointers. |
| Durable parent channel | Implemented with validated destinations, exact generation/correlation/question fencing, separate delivery/response/completion and bounded missing-response escalation. |
| Mechanical outcome relay | Implemented with immutable original envelopes, recoverable per-hop publication/receipts, idle watcher relay, bounded queues, fair repair sweeps and outage recovery. |
| Task transfer and lifecycle controls | Implemented with one retained task authority, generation-fenced transfer, routed successor commands, verified endpoint reconciliation and distinct coordinator/subtree/retirement operations. |
| Shared capacity and observation | Implemented with coordinator overhead, worker reserve, aged-request fairness, admission recovery and bounded lineage/channel projection with explicit unknown values. |
| Documentation and compatibility | Command help and maintained architecture/home references describe the new interfaces and responsibility-based legacy mapping, while Phase 09 retains executable migration ownership. |
| Required repository validation | Passed macOS/Linux suites, the unchanged macOS 93 percent coverage gate, final shell suites and two-round isolation proof. |
| Live integration | Actual Codex/tmux independent-child and parent-outage trial passed as detailed below, separate from mocked transport and admission tests. |

### Behavioral coverage map

The phase's existing shell families remain regression coverage, supplemented by focused Rust tests where typed records and interruption boundaries can be asserted directly.
The added `mx-coordinator-spawn` fixture is classified in the runner manifest and executed through the instrumented binary for Rust coverage.

| Contract | Focused checks |
| --- | --- |
| Project/idea domains, repeat-safe provisioning, home ownership and supported launch combinations | Spawn/domain Rust tests and `mx-coordinator-spawn`, daemon lifecycle/safety/harness configuration and backend shell families. |
| Wrong-home, stale-attempt and correlation rejection | Parent-channel and pending-reply Rust tests, `phase05_parent_channel`, pending-reply/send-marker/report/MCP shell families. |
| Crash recovery, model-silent outcomes and bounded progress | Parent-channel interruption, immutable-event, replaced-parent, queue-capacity and continuous-arrival sweep tests, plus the idle-watcher multi-hop integration test. |
| Exact human answers | Current-question/brief/workflow binding, superseded answer history and unrelated-wait preservation tests through domain and report interfaces. |
| One authority across transfer | Handoff fault/retry/cycle tests and `phase05_transfer` CLI integration covering retained state, custom owner/root state, revised brief and successor operations. |
| Child continuity and safe retirement | Teardown generation/endpoint/retirement tests, daemon/teardown shell families and the separate real independent-child trial. |
| Capacity across domains and direct work | Headroom tests for worker reserve, aged coordinator progress, fresh-arrival fairness, dependencies and crash reconciliation. |
| Bounded, truthful projections | Snapshot tests for nested/partial homes, deduplicated facts, distinct task/session counts and unknown native observations, plus the existing system-snapshot shell family. |

### Scenario handoff to Phase 12

The following map separates available Phase 05 fixtures from the combined evaluation owned by Phase 12.

| Scenario | Available evidence and remaining integration |
| --- | --- |
| 32 | Named coordinator spawn, nested delegation and direct-worker admission fixtures plus the real independent-child trial; the simultaneous three-domain human-response measurement remains with Phase 12. |
| 33 | Exact spawn/transfer retry, retained prelaunch failure, domain charter and transfer interruption tests; these preserve one canonical authority and accepted identity. |
| 34 | Frozen report evidence, per-hop retry, root outage, duplicate presentation and idle-watcher relay tests plus actual child reporting during parent outage; publication/PR and workflow producers remain with Phases 06/08. |
| 35 | Wrong-home, same-basename, generation/correlation and bounded pending-reply tests plus exact current-question answer tests; human-answer settlement is fixture evidence. |
| 36 | Transfer, actual successor CLI relaunch with a mocked backend, interrupted archival/retirement and separate live child continuity evidence; no single combined live trial is claimed. |
| 37 | Shared root admission, coordinator overhead, worker reserve, fairness and bounded channel-health tests; saturated live throughput and root response measurements remain with Phase 12. |
| 38 | No-repository idea research, explicit project binding, implementation refusal before mutation, overlapping domains and immutable child task identities are covered by isolated fixtures. |
| 39 | Shared lineage/health projection and responsibility-based migration guidance are available; executable migration, TUI/MX Viz and equal-budget scale trials remain with Phases 09-12. |

## Checks

Local diagnostic logs use the `/private/tmp/mx-phase05-` prefix.
Before implementation edits, `cargo test --locked --workspace` passed 602 tests with zero failures and zero ignored tests on the merged prerequisite tree.
That baseline result establishes prerequisite regression health, not Phase 05 acceptance.
The commands, outcomes and limitations below record the completed checks.
The unchanged repository validation includes formatting, strict Clippy, the workspace Rust suite, release build, shell fixture inventory, full shell suite, maintained-documentation validation and the existing 93 percent Rust coverage gate.
Coverage follow-up adds contract tests for malformed authorities, existing lineage cycles, interrupted control and answer receipts, history conflicts and bounded route repair.
No coverage exclusions, acceptance thresholds or production timing bounds were relaxed.

### Combined-tree checks

Checks below use isolated test homes and backend namespaces, with private operational state excluded.
The final macOS and Linux checks pass.
The macOS host uses Rust `1.97.1` on arm64.
Linux acceptance runs as ordinary user `mxvalidate` in the isolated `mx-phase05-validation` container at `/home/mxvalidate/work` with Rust `1.98.1`, Node `22.23.2`, GitHub CLI `2.46.0`, `LANG=C.UTF-8`, `LC_ALL=C.UTF-8` and `RUST_TEST_THREADS=2`.
Final closeout logs and JSON artifacts use `/private/tmp/mx-phase05-closeout-*`.
The isolated Linux validation container was removed after its artifacts were copied; the separate live trial's dedicated tmux server was also stopped.
`mx-phase05-closeout-stages.json` records exact argument vectors, UTC times, exit codes and durations.
The final changed-code/test/contract manifest has SHA-256 `e236f6652b52be4541045c5ac99d7f3a0196d4e81033261b3759a8ab327edd7a`, recorded with per-file hashes in `mx-phase05-closeout-final-source.json`.
The unchanged Rust source and test subset has SHA-256 `6e119e3bbbfa476299c25dc58ea8157a62cbf6270ca30bd4bd19566a534a25d1`.
The clean coverage and Rust suites preceded only the final shell notification-validator and assertion-inventory correction; both full platform shell suites and the final isolation proof exercise that correction.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on macOS and Linux. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed on macOS and Linux. |
| `cargo test --locked --workspace` | macOS passed 676 tests across 34 test/doc binaries, with zero failures and zero ignored tests. |
| `cargo build --release --workspace --locked` | Passed on macOS and Linux. |
| `target/release/mx test-run --check-coverage` | Passed: 129 fixtures, 108 accelerated, 11 serial and ten Herdr. |
| `target/release/mx doc-audience-check` | Passed after final status updates: 82 maintained surfaces and 423 local links. |
| `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase05-closeout-isolation-proof.json` | Passed: 107 candidates per round, two rounds, 538,460 ms, zero failed rounds, zero leaks and zero known-failure exceptions; verified JSON promoted to the archive. |
| Full instrumented Rust line coverage with unchanged CI exclusions and `--fail-under-lines 93` | Passed clean: 93.30 percent, 61,251 lines, 4,101 missed; all 676 instrumented tests passed in 722.503 seconds. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-phase05-closeout-shell.json` | macOS passed: 129 fixtures, zero failures, eight gated skips, 301,174 ms; real Herdr presentation passed. |
| Linux full Rust suite | Passed: 678 tests across 34 test/doc binaries, with zero failures and zero ignored tests. |
| `target/release/mx test-run --all --jobs auto --json /tmp/mx-phase05-closeout-linux-shell.json` | Linux passed: 129 fixtures, zero failures, 16 gated skips, 184,833 ms. |
| Release CLI help | All seven command owners passed: `spawn`, `domain`, `task-transfer`, `teardown`, `parent-channel`, `task-model` and `send`. |

The final macOS release binary SHA-256 is `eea9d96450a91481acdbc37e050ab74255ac8a26b45e8d638b857c99743c0fe1`.
The verified resource manifest SHA-256 is `d00044b128e8837a4c70f77349e7d844f461a910cac44d0622f2d0e7baf7ef11`, with 650 declared conflict pairs.
The exact coverage command is:

```sh
cargo llvm-cov --locked --workspace --all-targets \
  --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' \
  --fail-under-lines 93
```

### PR CI fixture correction

The initial PR run [35042199323](https://github.com/KashyapTan/Multplx/actions/runs/35042199323) passed ten jobs, including Linux Rust and the unchanged line-coverage gate, but failed the macOS Rust job in `phase05_transfer`.
The final routed transfer inherited the host `PATH` and omitted `MX_TMUX_STATE`, so endpoint reconciliation used host tmux for an endpoint created by the fixture's fake tmux.
The correction supplies the same fake environment for launch and final transfer and asserts the fake endpoint's transition to stopped plus cleared canonical endpoint/session fields.
Production endpoint verification, timeouts and CI gates are unchanged.
These checks use a mocked tmux transport; they do not add live provider evidence.
The prior combined-tree fingerprints identify the original validation snapshot, before this test-only correction.

| Check | Result |
| --- | --- |
| `cargo test --locked -p multplx-cli --test phase05_transfer` | Passed: one test, zero failures. |
| `for iteration in 1 2 3 4 5; do cargo test --locked -q -p multplx-cli --test phase05_transfer || exit 1; done` | Passed five of five repetitions before adding the explicit live-marker precondition. |
| `cargo test --locked -p multplx-cli --test phase05_transfer -- --nocapture` | Passed after the final marker-precondition assertion: one test, zero failures. |
| `cargo clippy --locked -p multplx-cli --test phase05_transfer -- -D warnings` | Passed. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |
| `target/release/mx doc-audience-check` | Passed after this evidence update: 82 surfaces and 423 local links. |

This follow-up records local validation; hosted validation is tracked by the [PR checks](https://github.com/KashyapTan/Multplx/pull/41/checks).

### Platform and live-test limits

The macOS shell runner reported eight gated skips: the optional cmux smoke, authenticated Claude/Codex/Cursor/launcher/Pi live fixtures, the Pi/Herdr marker trial and the Pi typecheck because `tsc` was unavailable.
Linux reported 16 gated fixtures, adding eight unavailable-Herdr checks to the same set.
Optional installed-Pi branches were unavailable, and Linux also skipped its optional zsh branch.
These skips are reported rather than counted as live provider evidence.
The [Phase 04 user-approved deferral](phase04-implementation.md) of other provider live trials until after all phases remains in force.
The separate real Codex/tmux parent-outage trial below supplies Phase 05 independent-child and report-routing evidence.

### Development checkpoints

These checks ran on a changing tree and do not establish final acceptance.
Their failures are resolved by the corrections and passing checks above; they remain recorded as diagnostic history.

| Check | Result |
| --- | --- |
| `cargo check --locked --workspace` | Passed at the first combined implementation checkpoint. |
| `cargo build --release --workspace --locked` | Passed for the first behavior/live checkpoint. |
| `target/release/mx test-run tests/mx-coordinator-spawn.test.sh` | Initial failure: idea-only charter rejected by project-less home-seed validation. After correction, the release checkpoint passed project/idea provisioning, JSON with space-containing paths, duplicate requests, stop/bind/restart, queue replay and provider refusal cases. Later failure/retry additions still require combined verification. |
| `target/release/mx doc-audience-check` | Passed at the initial documentation checkpoint: 82 surfaces and 416 local links. |
| `cargo test --locked --workspace --no-fail-fast` | Development checkpoint failed four assertions: a legacy teardown fixture did not provide verified endpoint absence, one parent-replacement assertion predated the accepted-outcome repair behavior, and two new control fixtures needed consistent endpoint identity. These are not acceptance passes. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Development checkpoint failed two excessive-argument warnings in the new domain API; correction required. |
| Instrumented workspace suite with unchanged CI coverage exclusions | Development checkpoint failed the legacy teardown fixture and the newly added coordinator launch-failure retry case. The interrupted shell wrapper left later fixture coverage unexecuted; its diagnostic 85.42 percent is not an acceptance coverage result. |
| Isolated Linux workspace suite | First changing-tree overlay failed compilation while admission tests still used the prior function signature; a stable-tree rerun is required. |
| `target/release/mx test-run --check-coverage` | Development checkpoint passed: 129 fixtures, 108 accelerated, 11 serial, ten Herdr; this is inventory coverage, not Rust line coverage. |
| `target/release/mx test-run --all --jobs auto` | Development checkpoint failed: 129 fixtures, nine failures, eight gated skips. Failures covered coordinator launch retry, uncertain Herdr launch retry, four teardown-observation fixtures, the report MCP schema expectation, the archived isolation-proof manifest and one real Herdr presentation ordering trial. Repairs and a complete rerun are required. |
| Isolated real Herdr presentation rerun | Passed all scenarios on Herdr `0.7.4`, including three repeated create/order/cleanup waves and multi-home restart/focus checks; 172,887 ms. The earlier failed full-run observation remains recorded. |
| Resumed `cargo test --locked --workspace --no-fail-fast` | Passed 636 tests; the new exact transfer retry test failed because retry reconstruction used the updated scope revision. The focused correction passed subsequently; a final full rerun remains required. |
| First resource-isolation proof, four workers and two rounds | Failed: 107 portable candidates; round 1 had a Herdr teardown mock failure and one watcher-lock timing failure, round 2 had the same Herdr teardown failure. The artifact write also refused macOS's symlinked `/tmp` parent; the acceptance run must use canonical `/private/tmp`. No successful proof archive was substituted. |
| First completed instrumented workspace run | All tests passed, but line coverage was 92.29 percent (59,401 lines, 4,581 missed), below the unchanged 93 percent gate; additional Phase 05 fault and contract coverage is required. |
| macOS full shell checkpoint after initial repairs | 129 fixtures, one Pi watcher hung-successor recovery failure and eight declared gated skips; real Herdr presentation passed in this run. |
| Linux full shell environment checkpoint | Six failures exposed root-user permission bypass, absent GitHub CLI and an older Node runtime; the environment was corrected to an ordinary user with GitHub CLI and Node 22 before acceptance reruns. |
| Linux cooperative process-cleanup fixture | An independently reproduced pre-exec TERM race was fixed with child-owned readiness, preserving the five-second bound and child-absence assertion; 100 candidate Linux repetitions, 20 post-fix Linux repetitions and 20 macOS repetitions passed. |
| Explicit idea-domain implementation refusal | The new CLI assertion caught project registration occurring before refusal; the guard now resolves accepted project bindings without mutation, and all seven coordinator fixture cases pass. |
| Resource proof after the idea-boundary guard | Round 1 failed the bootstrap fixture's partial-output readiness assertion while round 2 passed, with zero leaks; the test now publishes readiness only after its required output, and the final proof must rerun. |
| Linux ordinary-user shell checkpoint | Three failures remained: two from the root-owned Herdr test lock directory and one extra same-cycle watcher control line; after fixing the test namespace ownership, all three focused fixtures passed. |
| Pi recovery repetition | The original hung-successor case passed 20 focused repetitions and the empty-close case passed 12, while full-fixture failures moved between cases under concurrent heavy validation; no timing threshold was changed. |
| Successor execution through retained authority | The CLI integration caught path-alias lock contention and stale legacy endpoint fields after transfer; canonical authority paths and clearing stopped projections restore routed relaunch with one authority, unchanged brief evidence and original allocation ownership. |
| Interrupted parent-channel archival | Fault tests caught delivered-but-unarchived records being skipped during relay and omitted from retirement blockers; relay now resumes archival, while every active record, including malformed records, blocks retirement until repaired. |
| Normal debug cache after coverage | The help-side-effect assertion found two LLVM `.profraw` files from the ordinary debug binary; an isolated reproduction confirmed instrumentation contamination, so generated build artifacts were cleared and the unchanged workspace checks restarted. |
| Linux checkout parent permissions | Two service fixtures require a temporary sibling outside the source root; an ordinary user cannot create that sibling when the checkout is `/work`, so the isolated checkout was relocated to `/home/mxvalidate/work` and rebuilt without changing the fixtures. |
| Linux final notification assertion | The full suite reproduced a valid merged notification followed by its control event in the same watcher cycle; the fixture incorrectly anchored merged text to the end of the entire output, requiring a record-aware assertion while retaining exactly-once delivery and retirement checks. |
| Split-test assertion inventory | The first proof after the validator addition caught its missing inventory registration; the maintained map now records all 142 cases exactly once with an explicit addition reason, preserves the historical baseline, and passes on both platforms. |

The initial real tmux/Codex diagnostic uses `/private/tmp/mx-phase05-live.pd2KqF`, a temporary packaged runtime, a temporary remote-free repository and a dedicated tmux socket.
Named project-domain provisioning returned canonical parent/root/home/domain identities and launched Codex `0.151.0` with `gpt-5.6-sol` and high reasoning.
The temporary runtime initially omitted its binary asset, producing an adapter error; the fixture was corrected by supplying the built release binary before continuing.
After the fixture correction, a metadata-routed `mx send` request with correlation `a5c1f40d7e165094` reached the coordinator.
One native `gpt-5.6-sol` High researcher returned `42`; accepted start/result observations at `21:07:58Z` and `21:07:59Z` bound child `01a0a6e5-dae8-7670-890a-9c94da10fd19` to coordinator attempt generation 1 and brief revision 1.
The coordinator wrote a short private artifact and accepted `done: LIVE_PHASE05_RESULT_42` through `mx-report`, retaining the request correlation and a parent-outbox record.
The dedicated tmux server was stopped after capturing the transcript and binary fingerprint; its homes and artifacts remain retained.
This changing-tree trial establishes real named provisioning, native delegation and direct reporting, not final multi-hop relay, pending-reply settlement or independent-child continuity.

### Real independent-child and outcome trial

The later isolated trial at `/private/tmp/mx-phase05-live-final.8DoyPh` used Codex CLI `0.151.0`, `gpt-5.6-sol` High and tmux `3.7c`, a temporary remote-free project and a dedicated tmux socket.
The runtime binary SHA-256 was `83d99dccccfeef576cb35968943b2d6e06905127dc04c4011f840cb0cfa034e6`.
The named project coordinator delegated an independent CLI researcher; checkpointing the coordinator left the child endpoint alive.
While the coordinator was stopped, the actual child recorded `phase05-live-outage-result-91ab` as event `report-18d59c21efde4ba8-90661-0`, attempt `attempt-18d59bf21167cf18-67807-0`, generation 1 and brief revision 1.
The coordinator restarted at generation 3 while the child's canonical metadata remained byte-identical and exactly one child window remained.
Runtime relay first delivered the event to the coordinator's inbox at its recorded owner state, then root-owned relay advanced the remaining hops and published the original event in the root message outbox.
The original and root event JSON values have identical canonicalized SHA-256 `88e5d9ae07eba549fb3ae572ea8df73fbc27c0eee396af62fb58f9692f75ba15`.
The root wake contains the distinctive token exactly once; a repeated relay reported zero progress, zero pending inbox records and zero pending outbox records.
No coordinator summary or model turn was required for that delivery.
Separately, `idle_watcher_checkpoints_relay_a_nested_outcome_without_model_turns` verifies automatic runtime-only multi-hop relay in isolated local fixtures without manual relay calls.
The trial retained per-hop receipts, relay JSON, pane transcripts, before/after child metadata and `root-owned-relay-verification.txt`; only its exact tmux server was stopped.
`commands-and-evidence.txt` in that directory records the exact lifecycle commands and environment.
The trial used `MX_SPAWN_NO_GUARD=1` and `MX_HEADROOM_SKIP_QUEUE=1` and invoked the runtime relay explicitly, so it does not establish live admission saturation or automatic watcher scheduling.
After the child successfully recorded the outage result, Codex reported its account usage limit; no further model work is claimed after that point.
This is live local session and filesystem evidence, separate from mock-backend regression tests; final repository checks are recorded above.
Pending-reply settlement, human-answer binding and admission saturation have deterministic fixture evidence, not a live provider trial.
Transfer, replacement, child continuity and interrupted retirement are covered compositionally; no single live run is claimed to combine all of Scenario 36.
The final runtime includes subsequent routing and admission safeguards verified by regression tests; the recorded live binary fingerprint identifies the earlier live checkpoint precisely.

## Later-phase integration

Phase 06 owns publication and delivery outcome producers; Phase 08 owns workflow producers and its broader human-decision flow.
Phase 09 owns executable legacy-home migration, including responsibility-based role mapping and ambiguous parent/scope reconciliation.
Phases 10 and 11 own MX Viz and TUI views using the shared projection.
Phase 12 owns the combined flat/hierarchical 1/5/10/20-task trials and release activation.
No scalability improvement or untested provider continuity is claimed by this evidence.
No assigned Phase 05 implementation or verification work remains unresolved.
Phase 06 can begin from this implementation after review and merge.
Release activation and the listed later-phase integrations remain outside this completion boundary.
