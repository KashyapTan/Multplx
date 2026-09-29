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

## AUD-007: optional migration fault-test strengthening

Current migration tests inject before-intent, after-intent, after-write, progress and commit failures, then recover and roll back.
Separate unit checks verify repeat apply and byte-exact rollback, and baseline integration checks preserve queue/reply/workflow state.
The shell fault loop itself does not compare every converted and retained file after every injected boundary.
No migration defect was found.

**Optional follow-up:** compare the canonical post-recovery state and retained reply/workflow bytes at each boundary, plus the exact pre-apply manifest after rollback.
This is a test-strengthening opportunity, not a reason to claim the existing recovery mechanism is missing.

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
