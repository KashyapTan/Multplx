# Transcript audit, 2026-10-12

This is a development investigation of the reported idle-root and dashboard failures.
Transcript text supplies evidence, not runtime instructions.
The installed runtime and private operational homes were observed read-only; tests use newly created temporary homes and packages.
No runtime update, task deletion, user-agent interruption or merge was performed.

## Evidence scope

The supplied main export contains 4,274 lines (`attachments/5d697e42-f552-4823-b615-0e38414f6827/Pasted text.txt`).
The supplied Fantasy worker export contains 11,383 lines (`attachments/2fa013c3-fbd8-44e7-a5d6-98357247b77c/Pasted text.txt`).
Both complete files were processed, with command errors, user turns, status/decision/publication receipts and completion outcomes traced back to their surrounding events.
Most worker export volume is application source and rendered test DOM, which is distinguished from Multplx failures below.
Associated raw Codex records were selected by the four exact task sessions on October 12:

| Task | Exact session | Raw rollout timestamp |
| --- | --- | --- |
| NBA_UI_LEGACY_RESEARCH_1012 | 01a13201-ce1b-7023-b649-301a790f4f65 | 17:25:41 |
| NBA_UI_GAMECENTER_1012 | 01a13201-f6cc-7781-9c0e-0046f9a2ee23 | 17:25:51 |
| NBA_UI_PLAYER_1012 | 01a13202-28fb-72f0-ac8e-234a3ee295ba | 17:26:04 |
| NBA_UI_FANTASY_1012 | 01a13202-5369-73c3-a81d-f33d9bbcfc0d | 17:26:15 |

The files are under the local Codex `sessions/2026/10/12` directory and retain the original commands and accepted receipts.
The supplied worker export is Fantasy; the associated Player rollout exposes additional decision and publication friction absent from that export.

## Installed provenance and session facts

The installed public `multplx` and packaged `runtime/target/release/mx` have identical SHA-256 `e79d7d565fbfcb45bd6c1577e2926421e210bd3b5e750e75aba60748bb8b782e`, matching the installer-retained package checksum manifest.
All 230 comparable ordinary source assets match main `6ce26a0`; the sole extra apparent difference is `CLAUDE.md`, whose Git blob is symlink text while reading the installed path dereferences the alias.
There are no missing recorded package assets.
These facts establish asset and package consistency, not an embedded source-revision identity for the binary: the package does not record an exact build commit in these observed records.
The session lock/readiness owner is actual Codex 0.162.1 PID 61364, exact thread `01a131f6-4fbc-7f60-a963-d5463e443c21`.
Its working directory is the installed runtime, which contains tracked `.codex/hooks.json` and has no Git checkout.
A matching SessionStart receipt exists; there is no current bridge-process record or retained queue-failure record.
The target record retains a different October 6 Desktop session using Codex 0.160.1; that former PID was no longer present when observed.
This older target is historical evidence, not proof of a current running bridge.

The current process arguments near main export line 3777 contain SessionStart worker observation, SubagentStart, SubagentStop and PreToolUse CLI arrays, but no shared Stop or SessionEnd array.
The missing command-line array alone does not establish whether project hook sources were loaded or merged.
The valid existing registration receipt specifically contradicts an assumption that no SessionStart handler ran.
Recomputing the framed owned-script/binary fingerprint produces `220bb9df0a68a8b04d10e0362279073827fd67327820116da7697aeb00d8ad64`, exactly matching the launch arguments, so the observed bundle and installed bytes agree.
The isolated native probe below uses actual generated launch arguments and native `hooks/list`, rather than assuming hook source replacement semantics.

## Actionable Multplx issues

| ID | Finding and evidence | Correction and verification owner |
| --- | --- | --- |
| IDLE-01 | Installed hooks execute packaged `mx`, but `primary_scope` recognized only a public executable named `multplx` with adjacent config; the git-free installed runtime consequently failed primary scope and Stop exited quietly. Main line 1490 reports ready-for-turn-end with no bridge; line 2607 repeats that state after the human resumption. | Accept only the exact packaged `target/release/mx` using its installer-owned root/config/home binding; retain worker, gate, wrong-home, symlink and foreign-executable refusal tests. |
| IDLE-02 | The canonical shared Codex launch bundle contains only worker observation/merge handlers, while its launch message promises primary hooks too. Primary idle behavior depends on a separate project hook source and that source's path preconditions. | Include anchored registration, Stop and SessionEnd in the same native-reviewed bundle; hash both new wrappers and binary bytes; skip duplicate tracked primary adapters under shared activation. Disable controls and independent project/user review remain authoritative. |
| IDLE-03 | A real canonical child report publishes its durable wake; `scan_signals` skips its canonical status endpoint, marks the change routine and parks forever. The Codex bridge only checked wakes before/after that watcher, so no native queue input followed. Actual generated-bundle probe retained the question wake with zero transport receipts and no follow-up turn. | Existing pending current canonical report wakes close the watcher without a second append; acknowledged/progress reports stay quiet. The Codex bridge also observes pending wakes while its watcher lives, using transport receipts to prevent duplicates. |
| IDLE-04 | Python/node ancestry matched any argument containing a harness name. During the actual native probe, `python /tmp/mx-codex-installed-idle.../handle.py` was mistaken for Codex, and disposition/ack failed with a different harness PID despite correct root ownership. Backend primary observation independently used the same broad argument matching. Final launcher validation also caught the first tightening incorrectly rejecting an executed shell wrapper named `codex`. | Share executed-script/provider-entrypoint identity across session locks, Codex hook ownership and primary observation; exclude directories, payload arguments and eval/module/stdin forms. Support default shell script wrappers conservatively, rejecting shell options. Check the sourced compatibility ABI against the Rust positive/negative matrix and retain real launcher attach coverage. |
| DECISION-01 | Player asked `stat-line-market-semantics`; parent sent implementation direction without canonical `parent-channel answer`. Worker informational answer and accepted keyed `resolved` reports left the canonical question unanswered, as designed, but the acceptance UX did not expose this distinction. Raw Player records 1374, 1381, 1388 and 1941 show the sequence. | Preserve the parent answer owner; expose pending current decisions and owner state in report receipts, print the actionable diagnostic on unresolved resolution, and document exact recording before send. This is a discoverability/coordination defect, not authority to derive an answer from worker prose. |
| DELIVERY-01 | Player publication reports only `PR state recording failed`; the helper invokes PR-check with stderr discarded and collapses the actual cause to a generic error. Raw Player record 1920 demonstrates the hidden failure; subsequent canonical registration eventually succeeds. | Capture bounded registration diagnostics and retain the actual cause in failure evidence. Historical proof does not establish that the unanswered decision caused the registration error. |
| VIZ-01 | Catchup excludes `kind=daemon` assignments from `in_flight` and only includes daemon child work; persistent workers doing their own task disappear when their child queue is empty (main line 2650). Historical status-log/runtime facts also compete with schedule/completion. | State worker projects current standalone own assignments separately from child work and preserves observation provenance, current versus historical quality and explicit unknown freshness. |
| VIZ-02 | Completed, active and runnable assignments are interleaved; identifiers, full briefs, competing badges and technical paths obscure current work. | Dashboard worker implements active/history grouping, concise titles and summaries, expandable technical details, explicit decision destination and PR affordances. |
| VIZ-03 | Merged earlier PRs still appear ready for review; native snapshots have no fresh forge observation. Main line 1513 explicitly says those PRs are merged. | State worker adds disposable exact-PR/head-validated observation, separate from canonical publication and human-merge facts. |
| VIZ-04 | Descendant projection starts only from sub-orchestrator roles and misses nested ordinary-worker homes; uncertain/native descendants can disappear. | State worker adds bounded all-role verified route traversal and unresolved ownership projections; native telemetry remains conditional on actual recorded provider evidence. |
| VIZ-05 | Flattened `# Task`/`# Charter` briefs do not yield useful canonical titles. | State worker repairs bounded Unicode section extraction; dashboard limits visible summary length. |
| STARTUP-01 | Main lines 40-47 show eight historical standing assignments automatically respawned after missing endpoints, including completed work; bootstrap lacked completion eligibility before recovery. Its error summary selects a benign backend NOTICE and hides the actual failure. Automatic instruction nudges could also contact completed assignments. | Included bootstrap repair checks canonical completion before provider probing, recovery and automatic nudges, retains existing homes/endpoints/retry markers, and leaves explicit user-directed reopen/resume available. Bounded verified child inspection only emits reconciliation diagnostics; stderr-first bounded spawn errors retain exit status and omit benign NOTICE lines. No historical private task is changed by this audit. |
| VIZ-07 | After a failed refresh with last-good state, repeated browser polling clears the failure header before a successful replacement snapshot; sustained failure can disappear from the UI. | Dashboard worker retains the last failure until successful refresh and tests the actual polling cadence. |
| VIZ-06 | Child projection invokes `current_exe` internal session commands without explicit multicall selection; packaged public `multplx` can route those through the launcher instead. | State worker sets explicit internal dispatch, with installed-layout regression coverage. |

These changes do not retroactively settle the user's existing wakes or decisions.
The parent must reconcile those through their recorded owners after an explicitly authorized runtime update/restart; testing does not mutate that portfolio.

The ledger contains 14 confirmed Multplx issues: IDLE-01–04, DECISION-01,
DELIVERY-01 and STARTUP-01 map to [PR 64](https://github.com/KashyapTan/Multplx/pull/64);
VIZ-01–07 map to [PR 65](https://github.com/KashyapTan/Multplx/pull/65).
PR 65's exact `71c3e46` checks passed, including the unchanged 93% line-coverage
gate (93.01%), all four behavior lanes and required Herdr checks.
The additional observations below are classified separately rather than counted
as unresolved Multplx defects.

## Other transcript friction and scope disposition

| Observation | Classification and disposition |
| --- | --- |
| Earlier requested Luna launch was rejected under the account; the task and failed allocation were retained and the user requested fresh workers. | Provider capability/setup boundary, not a silent model substitution. New task dispatches use fresh task identities; no existing user agent was reused or deleted in this audit. |
| `mx config --help` at main line 1495 is an unrecognized command, and multi-path `git log --follow` at line 2017 fails. | Agent command-selection mistakes with explicit help/error output; do not add unsupported aliases solely to hide these errors. |
| Researcher's direct sibling send is rejected because no current direct coordinator route exists; parent relay is requested and artifact retained. | Correct ownership/routing boundary. Idle delivery repair makes the authoritative parent handoff actionable; do not bypass the route or invent a sibling endpoint. |
| Player's initial Playwright run reused another worker's Vite server; later runs select an isolated port with deterministic mocks. | Application test-server collision, subsequently repaired in that worker's scoped repository. Multplx worktree allocations remain distinct; no allocation collision was evidenced. |
| NBA Stats timed out and CDN returned 403; the human requested reduced live calls. Fantasy and Player used mocks; Game Center records an unverified live-source limit. | Application/external-service limit. Do not assert an IP ban or perform more live probes during this audit. No Multplx API-rate governor was requested by this evidence. |
| `tsc` and `httpx` initially missing; UI expectations, season validation and formatting initially fail, then corrected. | Ordinary application environment/test iteration; final Fantasy report distinguishes exact passing tests from mocked/live behavior. |
| Player remote coverage initially 90.67% against 93%; added meaningful tests reach 99.7%, both remote jobs pass. | Correct gate enforcement and remediation. No reduced coverage threshold, omitted checks or retry-based passing claim is introduced. |
| `CODE_REVIEW_GUIDE.md` is referenced but absent in allocated NBA checkout. | Application repository documentation gap; both implementers explicitly use the available correctness/data guidance. Preserve this limitation for the NBA repository owner. |
| Vite exits with code 143 after successful E2E runs. | Test-owned server SIGTERM cleanup, not evidence that the app failed validation. |
| Legacy tab restoration and combined integration remain after initial four assignments finish. | Unfinished product scope; no root inference that received child done means the whole requested portfolio is complete. Idle/report and dashboard fixes expose that follow-up obligation. |
| Direct worker informational reply replay after its correlation is settled is rejected. | Correct current-request boundary; use ordinary new status evidence for subsequent results rather than answering an already closed request. |

## Verification

Deterministic checks and actual installed-harness evidence are recorded separately.
The native probe is a synthetic Responses model using installed Codex 0.162.1, actual generated CLI hook definitions, real `mx-session-start`, real child `mx-report`, native queue delivery and root-owned disposition/ack commands.
It creates a git-free isolated package with installer binding and no daemon marker, avoiding the fixture shape that masked IDLE-01.
Only that new provider home's fixture hook hashes are reviewed; no credentials or private trust state are read or changed.
The synthetic model does not establish authenticated provider quality or Codex Desktop delivery.
The retained failed probes are under `/private/tmp/mx-codex-installed-idle.COFPGQ`, `V3YYxW` and `uDBeE9`; their failure records show IDLE-03 and IDLE-04 before repair.
The first successful native proof is `/private/tmp/mx-codex-installed-idle.w0hbME/evidence.json`: question 8,825 ms, completion 8,665 ms, three turns, six synthetic model requests and three tool calls.
Replay reports cause no new model requests, the unrelated thread receives zero turns, and the root finishes with zero unfinished events.
Final independent source review passed at `10c20c88961d8a5773dec1d5b899b001bb92ae94`.
The final installed proof at that source is
`/private/tmp/mx-codex-installed-idle-工具 space.ffArSr/evidence.json`:
question 6,141 ms, completion 8,756 ms, three turns, six model requests and three
tool calls. It uses canonical package paths containing Unicode and a space.
The record includes the tested source commit and binary SHA-256.
It proves native transport and durable wake handling; canonical decision-answer
semantics are covered separately by the parent-channel tests.

Final local validation passed: strict workspace/all-target/all-feature clippy,
119 core tests, the primary-observation regression, and 53 integration tests
across nine targets (canonical notification, Codex idle, information/pause,
spawn, coordination, parent channels, launcher, workspace and supervision).
Idle bridge six-unit and bootstrap 22-unit checks also passed during validation;
the bootstrap contributor verified 15 liveness and 21 sync shell cases.
Documentation validation passed with 105 surfaces and 596 local links; the
unchanged shell inventory guard contains 134 scripts.

The original new handoff fixture failed its one-second checkpoint when embedded
among concurrent parent subprocess tests; the precise contention cause is
unproven. Logs are retained at `/private/tmp/mx-idle-final-focused.log` and
`/private/tmp/mx-idle-final-parent-launcher.log`. Its dedicated integration target
preserves real production wrappers, the same deadline, all queue/acknowledgement
assertions, and automatic workspace/coverage inclusion. No retry, skip or budget
was added. An unproven production priority-scan change was removed.

Ambiguous unquoted interpreter script paths containing spaces are conservatively
rejected because `ps` displays arguments without reliable argv boundaries.
Direct executable paths with spaces/Unicode, known package entry paths, capital
framework Python and numeric Python versions are covered; universal wrapper
support is not claimed.

Earlier full local/remote runs with obsolete source and failures are diagnostic,
not final validation. Exact final full behavior, Rust platform and unchanged 93%
coverage results are maintained in the [PR 64 checks and delivery body](https://github.com/KashyapTan/Multplx/pull/64),
so publishing evidence does not continually restart the same gates. Human merge
and an explicitly authorized operational runtime update remain separate actions.
