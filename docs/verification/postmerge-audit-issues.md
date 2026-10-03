# Post-merge audit issue log

Audited on 2026-09-28 against `5326a8eb2510590444278225ac3af6fc11026cae` on `main`.
Scope: all twelve lean-redesign plans, their A1-A11 contracts and 39 acceptance scenarios, prerequisite implementation/evidence, the subsequent installer and terminal repairs, and the predecessor roadmap accounting.
The [acceptance ledger](../../plans/lean_redesign/phase12-acceptance.md#post-merge-audit-2026-09-28) contains the phase-by-phase source/test mapping and exact validation results.
This file records every finding from this audit; open verification limits are not presented as confirmed runtime bugs.
AUD-001/AUD-002 are fixed and locally validated on `fix/dependency-completion-invalidation`; the repair and exact checks are recorded below.
Documentation corrections below are included with the repair branch.

## Summary

| ID | Severity/type | Status | Finding |
| --- | --- | --- | --- |
| AUD-001 | High / correctness | Fixed and locally verified | A failed prerequisite remains completed and permits dependent work to start. |
| AUD-002 | Medium / regression coverage | Fixed and locally verified | Existing dependency checks miss completion invalidation after a current failure report. |
| AUD-003 | Medium / evidence retention | Open | All 23 referenced raw live-trial artifacts are missing from their temporary paths. |
| AUD-004 | Medium / plan accounting | Corrected locally | Active roadmaps retain obsolete phase, merge and source-cutover status. |
| AUD-005 | Low / onboarding | Corrected locally | Published source installers are still described as unavailable in the getting-started guide. |
| AUD-006 | Verification limits | Explicit follow-up | Broader authenticated providers and live 10/20-task scaling are not established. |
| AUD-007 | Low / optional test hardening | Not a demonstrated defect | Migration interruption fixtures could compare more retained bytes at each fault boundary. |
| AUD-008 | Medium / supervision correctness | Fixed and locally verified | Completed standing assignments retain perpetual supervision need. |
| AUD-009 | Medium / Cursor supervision | Fixed and locally verified | Cursor supervision requires model-authored foreground checkpoints between event turns. |
| AUD-010 | Medium / Codex supervision | Repair in progress | Codex supervision requires foreground checkpoints despite exact-thread queue support in the installed CLI. |
| AUD-011 | Medium / integration durability | Repair in progress | A queue receipt published before uncertainty evidence can suppress an unsent wake after a failed second write. |
| AUD-012 | Medium / integration failure visibility | Review in progress | An idle bridge watcher failure can leave only private failure evidence while the main model stays idle. |
| AUD-013 | Medium / local HTTP correctness | Fixed and locally verified | Valid delayed headers on inherited nonblocking sockets are rejected with HTTP 400. |

## AUD-001: failed prerequisite still releases dependent work

**Impact:** work can begin using a prerequisite that has explicitly reported a current failure.
This reopened A5 and Phase 08/12 correctness acceptance at audit time; the repair below closes this specific finding with new regression coverage.

The pre-fix release CLI reproduced the following sequence:

1. Spawn an implementation prerequisite into its allocated Git worktree.
2. Record typed evidence for that worktree's actual HEAD and submit a bound `done` report.
3. Submit a current bound `failed` report; it succeeds, but task-model inspection still returns `schedule.state = completed`.
4. Submit a new request depending on that prerequisite and spawn it; the output confirms `spawned dependent`.

The writer in [supervision.rs](../../crates/multplx-domain/src/supervision.rs) marks typed completion but only reopens completed state for `working` (lines 579-624 at the audited revision).
The consumer in [headroom.rs](../../crates/multplx-backend/src/headroom.rs) checks only the saved completed schedule state (lines 1561-1585).
Canonical brief revisions and recorded current-commit changes already invalidate completion, but a subsequent failure report does not.

The [preserved reproduction](../../plans/lean_redesign/phase12-dependency-audit-repro.sh) records the original bug after building the release binary:

```sh
cargo build --release --workspace --locked
bash plans/lean_redesign/phase12-dependency-audit-repro.sh
```

It asserts the stale completed state and requires an actual `spawned dependent` output before emitting `DEPENDENT_SPAWNED_AFTER_FAILED_REPORT`.
On the fixed runtime it intentionally exits 1 at the stale-completion assertion; use `target/release/mx test-run tests/mx-report.test.sh` for the passing regression.
Git, worktree, intake, typed evidence, report and admission code are real.
Tmux transport and capacity observations are synthetic; no paid model is launched.
Independent successful reproductions include `/private/tmp/mx-audit-dependency-repro.iQYOKQ` and `/private/tmp/mx-audit-dependency-repro.8K47Em`.
These temporary logs are supplementary; the reproduction source is retained in the repository.

**Resolved:** current failure, blocking, pause and keyed decision reports now withdraw completed state through the canonical report transaction; dependent reservation/drain defer and unrelated work remains runnable.
Regression coverage verifies both reserve and queued-drain paths, stale-attempt/brief rejection and fresh completion recovery.
Potential evidence-file mutation or unrecorded HEAD changes warrant further review, but this audit does not classify them as independently reproduced bugs.

## AUD-002: regression checks miss this transition

At audit time, the full Rust and behavior suites passed while AUD-001 reproduced.
The existing `headroom.rs` dependency predicate unit test accepts a minimal completed-schedule JSON record and does not exercise the real typed completion followed by failure.
This was a concrete test gap; the new owner-driven report regression now covers it.

**Resolved:** owner-driven regressions now cover done-to-failed invalidation, admission/drain, stale report rejection, old-message replay, current decision waits, resumed-report state and completion recovery.
A successful CLI exit alone is insufficient: deferred submissions may succeed without launching, so assert actual admission and endpoint behavior.
The preserved reproduction already makes that distinction.

## AUD-003: raw live-trial evidence is no longer available

Every one of the 23 raw paths in [phase12-live-results.json](../../plans/lean_redesign/phase12-live-results.json) is absent at audit time.
They refer to `/private/tmp/mx-phase12-live-matrix`.
The committed summaries, hashes, timestamps, worker outcomes and limitations remain, but the missing raw content cannot be checked against its hash or independently replayed from those paths.
This does not prove the historical results were wrong; it limits independent verification now.

**Required follow-up:** restore the original artifacts from a durable archive if one exists, or separately authorize a fresh bounded live run and retain its evidence durably.
Do not spend model tokens rerunning the original large matrix merely to conceal the gap.
The new 134-script runner result is kept locally at `plans/lean_redesign/phase12-postmerge-audit-behavior.json` and ignored by Git; the check summary is recorded below.

## AUD-004: roadmap and cutover accounting was stale

The root plan index still described only Phases 01-02 as complete and called the predecessor Plans 01-18 roadmap active.
The lean index and Phase 12 record still said PR #48 awaited merge and canonical source restoration was pending.
Phase 03 and Phase 11 HTML evidence also retained pre-merge next-step statements.
The porting guide and contributor instructions referred to dormant `AGENTS_E.md`, although the user restored `AGENTS.md` in `2f57d21`.

**Corrected locally:** current indexes now distinguish historical predecessors, record PR #48/#49 merges and canonical source cutover, and prominently disclose AUD-001 instead of claiming unconditional completion.
Phase 08/12 status and contributor context link the current audit.
Historical phase narratives remain intact as dated evidence; their superseded next-step instructions are explicitly distinguished from current status.

## AUD-005: installer availability wording was outdated

The getting-started guide said the source-download URL would not work until local scripts were pushed.
The preceding verification downloaded the public script and confirmed it matched the repository version.

**Corrected locally:** the guide now describes the published source bootstrap.
It still states that Rust/build prerequisites apply and does not imply a published prebuilt binary release.

## AUD-006: live verification and release boundaries remain

These are disclosed limits rather than newly demonstrated implementation failures:

- The user approved four five-task live trials instead of the original full 1/5/10/20 matrix; live 10/20-task scalability is not established.
- The historical hierarchical trials needed observer assistance and mixed-runtime recovery; they do not prove unattended throughput or a speedup.
- Claude/Pi/cmux and broader authenticated Cursor candidate verification remain unavailable or unrun.
- Current Codex/Cursor `--version` routing probes establish executable launch only, not authentication or useful model work.
- Complete provider billing, human-review latency and post-merge quality were not measured.
- Public prebuilt release publication and private operational-home migration remain operator actions; published source installation is separate.

**Follow-up:** keep these limitations visible and scope any new paid trials explicitly.
Do not describe synthetic graph scale, mocked transports or gated tests as live-provider success.

### Claude and Pi follow-up on 2026-10-03

The event-driven supervision recheck found neither Claude nor Pi installed in the available runtime.
Focused deterministic hook/extension checks passed, and the Pi fixture now retains two event cycles with one initial tool arm and extension-owned successors.
This does not close authenticated provider, current installed-version or live user-input responsiveness verification.
The [current supervision evidence](supervision.md#claude-and-pi-event-driven-recheck-2026-10-03) distinguishes these checks from the dated historical live results.

## AUD-007: optional migration fault-test strengthening

Current migration tests inject before-intent, after-intent, after-write, progress and commit failures, then recover and roll back.
Separate unit checks verify repeat apply and byte-exact rollback, and baseline integration checks preserve queue/reply/workflow state.
The shell fault loop itself does not compare every converted and retained file after every injected boundary.
No migration defect was found.

**Optional follow-up:** compare the canonical post-recovery state and retained reply/workflow bytes at each boundary, plus the exact pre-apply manifest after rollback.
This is a test-strengthening opportunity, not a reason to claim the existing recovery mechanism is missing.

## AUD-008: completed standing assignments retain perpetual supervision need

The 2026-10-03 event-driven recheck found that the shared supervision predicate excluded only completed ordinary assignments.
Completed persistent workers and coordinators remained in flight solely because their retained metadata existed.
Claude Stop auto-arm separately counted every metadata file, including completed ordinary assignments.
These projections could retain watcher and heartbeat model wakes after evidenced assignment completion, contrary to the standing-agent idle contract.

**Resolved locally:** all recognized well-formed current completed assignments share the same idle projection.
Canonical task identity, accepted attempt/brief revision, compatibility fields and explicit completion remain required.
Legacy, malformed, mismatched and reopened records still require supervision.
Unread source wakes, claimed or waiting inbox work and explicit checks independently retain supervision, preserving parent validation and external monitoring.
The Rust observer uses lock-free `observe_unfinished_count`; a read failure conservatively retains need.
The shell compatibility layer uses that existing Rust projection for inbox work and remains conservative when the runtime is unavailable.
Claude auto-arm uses the common predicate instead of raw metadata existence.
No persistent agent or retained result is removed or retired.

Exact focused verification passed after a locked release build:

- `cargo test --locked -p multplx-core supervision::tests`: four tests passed, including an occupied live wake lock, retained claimed inbox bytes, pending wake/check need and reopened/mismatched assignment identity.
- `target/release/mx test-run tests/mx-claude-stop-autoarm.test.sh tests/mx-turnend-guard.test.sh tests/mx-pi-watch-extension.test.sh`: three scripts passed, zero failures or gates, 101.325 seconds.
- Shell syntax, documentation audience/local links and whitespace checks passed.

The Claude regression proves a completed standing assignment stays idle until a wake, check or reopened assignment requires supervision.
The Pi regression proves two typed event cycles with one initial arm call and extension-owned successors before delivery.
Harness/model APIs are synthetic; fresh authenticated Claude and Pi runs remain the separate AUD-006 limitation.

## AUD-009: Cursor model-owned wait loop

At `f7e4412`, `cursor_hook` accepted only `loop_count: 0`, `.cursor/hooks.json` capped native follow-ups at one, and the primary protocol required repeated 180-second foreground checkpoints.
The model therefore remained responsible for continuing waits instead of ending generation and resuming only on an event or human input.
The [public upstream source and Cursor protocol pinned at `e31bc6e`](../upstream.md#event-driven-supervision-source-comparison-2026-10-03) supplied the stop-park comparison; the prohibited local reference directory was not inspected.

Commit `e6fb1c2` replaces that path with the Rust-owned stop park, latest-stop baton, session-process identity checks, tracked child cleanup, bounded failure feedback and explicit automatic follow-up ceiling.
The existing canonical wake claim, durable disposition and acknowledgement contract is retained.
The focused Cursor adapter and supervision-renderer scripts passed after a locked release build.
The isolated real Cursor `2026.10.01-e373342` probe passed two successive watcher follow-up turns, a typed human turn while parked, older-park retirement and away-mode cleanup without model tool calls.
Controlled native watcher-arm output establishes adapter transport and continuity rather than a live worker-report or forge-poller trial.
[Cursor verification](cursor-cli.md#event-driven-stop-park-recheck-2026-10-03) contains the exact replay command and retained local evidence pointers.

## AUD-010: Codex checkpoint compatibility gap

The pre-repair primary protocol required another model-authored foreground checkpoint after each wake or quiet timeout.
The installed Codex CLI now exposes `codex queue --thread --message`; a real isolated exact-thread transport probe showed an idle thread starts a new turn and busy-thread input waits until its current turn completes.
**Resolved locally:** runtime commit `c88b045` provides a Stop-owned bridge binding the exact thread, `CODEX_HOME` and live session-lock identity while keeping canonical wake disposition separate from queue transport receipts.
Unsupported CLI versions require a visible compatibility warning and the explicit bounded foreground fallback; the upstream Codex checkpoint protocol alone does not establish this modern local capability.
[Current Codex verification](supervision.md#codex-cli-event-driven-queue-recheck-2026-10-03) records four focused Rust unit checks, two instrumented runtime integrations and installed CLI native-hook/queue evidence against a synthetic Responses endpoint.
This evidence does not establish authenticated provider output or Desktop delivery.
The accepted activation boundary is managed Codex CLI launchers setting `MX_CODEX_IDLE_CLI=1` automatically, or a direct terminal CLI user explicitly opting in with that flag.
Inactive sessions keep the bounded foreground protocol, and the renderer selects a separate inactive instruction block rather than claiming a bridge is running.
Native hook trust review can leave project hooks unloaded even with that launch flag, so readiness also requires a native SessionStart receipt matching the exact provider thread and live lock owner.
The renderer requires its current `CODEX_THREAD_ID` to match; a flag, receipt UUID or shared process ancestry alone never establishes current-thread readiness.
Codex Desktop event delivery remains unverified; this CLI integration does not activate the development Desktop conversation.

## AUD-011/AUD-012: Codex bridge integration review

These findings concern the new implementation under review, rather than a defect attributed to the previously released checkpoint path.
Readonly review found suppression receipts written before the uncertainty file in `notify_with`.
If the second write fails after receipt publication, a later explicit retry has no uncertainty keys to release and may permanently suppress a notification that was never enqueued.
The worker was asked to publish recoverable uncertainty before suppression or bind both facts in one owned transaction, with a failure-boundary regression.
The write reordering removes the original second-write gap, but review also identified a crash after suppression publication and before the failure marker: pre-existing uncertainty must itself stop silent suppression and require visible reconciliation.
**AUD-011 resolved locally:** uncertainty is published before suppression receipts, and either retained uncertainty or a failure marker refuses silent bridge restart until exact-thread explicit reconciliation.
The final `codex_idle_runtime_two_cycles_identity_retry_and_cleanup` integration regression passed the crash interleaving with receipts plus uncertainty but no failure marker, refused automatic restart, and allowed explicit retry without discarding earlier successful receipts.

Readonly review also found an unexpected owned-watcher failure recorded only in `.codex-idle-failure` before the bridge exits.
With the main generation already ended, that private record alone cannot request a handling turn until another human or Stop event occurs.
The worker was asked to send one bounded operational failure input when the queue transport still works, while retaining durable failure evidence when the queue itself is unavailable.
**AUD-012 resolved locally:** an unexpected watcher exit creates a canonical durable Check and submits one bounded operational failure input when queue transport remains available.
The same integration regression passed watcher exit 7, preserved the private failure record, and observed the Check in a fifth queued operational wake.
Both instrumented runtime integration tests passed in 8.07 seconds; the queue and watcher endpoints are isolated fixtures, not authenticated model providers.
The separate installed Codex CLI native-hook and queue probe uses a synthetic Responses endpoint; [verification evidence](supervision.md#codex-cli-event-driven-queue-recheck-2026-10-03) records that boundary.
A suspected raw-queue restart storm was checked against the real watcher scan and marker path and was not substantiated; it is not recorded as a defect.

## Initial broad validation of event-driven repairs

The first broad release run exposed startup-hook indexing and missing runtime-inventory entries; commit `3698ee1` selects the startup command by identity and inventories the new idle adapter.
Commit `9fcb3b3` corrected provenance naming by retaining exact source URLs in the existing upstream reference owner; the naming check and documentation audience check passed.
The `mx-launcher-connection`, `mx-report` and `mx-viz` scripts passed their focused rerun in 21.723 seconds; the first two have no substantiated new defect or code repair.
Separate Viz investigation reproduced valid delayed HTTP headers rejected with status 400 in three of three runs; AUD-013 records the parser repair and passing regression.
Final broad validation results are pending and are not represented by the historical checks below.

## AUD-013: valid delayed HTTP headers are rejected on nonblocking sockets

The broad behavior run's Viz stale-caller failure included an HTTP 400 response, rather than proving a snapshot-reader stall.
A separate isolated dashboard probe connected to the server, waited 100 ms and sent a valid GET request; all three attempts received HTTP 400.
The nonblocking listener's accepted socket retained its mode on this macOS host.
The shared HTTP parser treated an early `WouldBlock` before headers arrived as malformed input.

**Resolved locally:** the bounded HTTP parser restores blocking mode before reading request framing.
The existing five-second read/write timeouts and all framing limits remain unchanged.
A portable Rust regression explicitly marks the accepted socket nonblocking, then sends delayed fragmented headers.
The Viz behavior fixture also requires HTTP 200 from a real delayed, fragmented client connection.
The same isolated dashboard probe returned HTTP 200 on all three attempts after the fix.

`cargo test --locked -p multplx-services http::tests` passed all four tests after the repair.
The locked release build passed before the final focused behavior rerun.
`target/release/mx test-run tests/mx-launcher-connection.test.sh tests/mx-report.test.sh tests/mx-viz.test.sh` passed all three scripts with zero failures or gates in 22.348 seconds.
This is an HTTP framing repair, not a relaxation of launcher lock, backend-command or stale-cache timing assertions.
The launcher and report failures did not reproduce in their focused rerun and have no substantiated code repair.

## Validation performed

- Locked release build, formatting and strict all-target/all-feature Clippy: passed.
- Rust workspace: 787 passed, zero failed or ignored.
- Complete behavior suite: 134 scripts, zero failures, eight explicit gates; 426.113 seconds.
- Inventory partition: all 134 scripts accounted for.
- Real executable-routing probe: Codex 0.156.1 and Cursor 2026.09.10-fd3934a passed; no authenticated model call.
- Same-runtime browser checks from the preceding audit: desktop/phone layouts at 0/1/5/10/20 tasks, artifact previews and polling/error behavior passed.
- AUD-001 reproduction: confirmed independently despite the passing suites.
- Documentation audience/links, shell syntax for the reproduction and whitespace checks: passed.

No new Linux run, coverage percentage or paid-model evidence is claimed by these local checks.


## AUD-001/AUD-002 repair validation

Branch: `fix/dependency-completion-invalidation`, based on `5326a8e`; the repair and audit corrections are submitted together in [PR #50](https://github.com/KashyapTan/Multplx/pull/50).
Implementation commit: `82f1603`; subsequent evidence-link edits do not change the tested source.
The production change is confined to the report writer and its help in `crates/multplx-domain/src/supervision.rs`; no schema, dependency scheduler or transport redesign is introduced.
After prior completion, current `failed`/`blocked`/`paused` reports set an external wait, keyed `needs-decision` sets a human wait, and other activity or unproven `done` reports reopen running state.
A fresh evidenced `done` restores completion.
Final review also caught stale display state after a failed/blocked/paused task resumed; current `working`/`resolved` now clears only those report-owned external waits without restoring completion.
A current `working` report does not bypass a keyed human decision or a coordinator lifecycle wait.
These changes use the existing atomic report transition and committed-message retry guard.
They govern future dependent admission; no already-running worker is killed or retargeted.
The delivery guide and report help describe the behavior.

`tests/mx-report.test.sh` now drives real project registration, Git/worktree allocation, typed delivery evidence, reports, intake and spawn/admission.
It checks failure, block, pause, keyed/repeated decisions, resuming failure/block/pause waits, preserving human waits, stale attempt/brief rejection, inert old-done replay, actual queued-versus-spawned outcomes, retained queue on drain, independent task progress, fresh completion release and unproven done after HEAD changes.
Tmux transport and resource observations are mocked; no authenticated model call is claimed.
The fixture honors `MX_RUST_BIN` for instrumented test runners and does not require an installed model CLI.

| Exact check | Result |
| --- | --- |
| `cargo build --release --workspace --locked` | Pass before behavior checks. |
| `target/release/mx test-run tests/mx-report.test.sh` | Pass: one script, zero failures/gates, 8.412 seconds. |
| `cargo fmt --all -- --check` | Pass. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Pass. |
| `cargo test --locked --workspace` | 787 passed, zero failed/ignored across 37 result groups. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-dependency-final-all.json` | 134 scripts, zero failures, eight explicit gates, 431.018 seconds. |
| `target/release/mx test-run --check-coverage` | Pass: all 134 scripts classified; inventory and partitions unchanged. |
| `bash plans/lean_redesign/phase12-dependency-audit-repro.sh` | Expected exit 1 after accepted failed report: its stale-completion assertion no longer holds. |
| `bash -n tests/mx-report.test.sh`, `target/release/mx doc-audience-check`, `git diff --check` | Pass. |

The full Rust and behavior suites were rerun after the final resumed-wait correction; the results above cover that exact final code.
The unaltered fixed-runtime runner result is kept locally at `plans/lean_redesign/phase12-dependency-fix-behavior.json` and ignored by Git.
These local files are not durable published evidence; CI owns its separate uploaded timing artifacts.
Source SHA-256: `supervision.rs` = `f47e86818a78f6672f72a75b00c3c391ccd987a19c0a7e3d93fba54f22219506`; `mx-report.test.sh` = `01486f3af08a928d332f860d6b5b26beae640ae5a5c22505c99d940cb9998674`.
The tested release binary SHA-256 is `5f55dfb1028e2b311cbc6d463c636a2d638f1d8caa9bb185715c94feae3dd9cc`.
This is local macOS validation, not a new hosted CI, Linux, full coverage-percentage or paid-provider run.
AUD-003, AUD-006 and AUD-007 retain their evidence-limit or follow-up dispositions; this runtime repair does not fabricate missing historical artifacts or expand live-provider claims.
