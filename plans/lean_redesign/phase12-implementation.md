# Phase 12 implementation evidence

## Status and source boundary

Status: in progress; release acceptance and repository cutover are not complete.
Work started on 2026-09-18 on `lean-redesign-phase12` from merged Phase 11 commit `8535d8e1c6048ac3a3525544394e753bed741256`.
The checkout was clean before branch creation.
The [phase plan](12-documentation-validation-cutover.html) assigns the work; [porting.md](../../porting.md#accepted-architecture-contract) owns A1-A11.
The [39-scenario acceptance ledger](phase12-acceptance.md) separates deterministic verification, historical evidence and open combined live trials.

Inspection covered CLAUDE.md, the porting guide, roadmap, Phase 12, prerequisite evidence, accepted architecture/worktree/coordinator research, actual package/launcher contracts, task intake, parent-channel integration, coverage wrappers and CI.
The source checkout remains dormant under `AGENTS_E.md`; root `AGENTS.md` has not been recreated and no operational session has been started in this checkout.
Private operational state and the excluded `firstmate/` tree are not validation inputs.
All mutable verification uses temporary homes, repositories, packages or an isolated Linux source copy.

## Implemented changes

- The release contract describes the implemented product without the development-only startup prohibition or links to absent development documents.
  Its filename remains dormant in this checkout; packages publish the canonical `AGENTS.md` name.
- Packaging accepts the eventual canonical source filename while retaining the deliberate dormant-source packaging path.
  Runtime discovery does not gain support for `AGENTS_E.md`.
- Installed launches pass their verified executable to hook adapters and prepend a scoped `mx` transport for the launched shell/chat.
  Worker launches bind the real harness executable, pass runtime and nested-delegation bindings explicitly, and encode the initial brief directly through the runtime before backend-shell environment expansion.
  This avoids a competing global alias or requiring a Rust source checkout.
- Herdr recovery accepts an unbound version-1 projection only after read-only checks prove exact recorded identity and inactive token-correlated endpoints.
  Live, unreadable, ambiguous and foreign endpoints remain retained, and the existing owner chooses conservative recovery or flat fallback.
- Installed coordinator launches bind private state/data/configuration paths, retain the package runtime and keep child reports separate from the parent route.
- Named tmux endpoints require exact window inventory membership; an absent window cannot resolve to the current primary pane during recovery.
- Interrupted pre-submission launches can retry the same allocation only after authoritative endpoint absence, with a bounded wait for short shared publication locks.
- Package upgrades preserve stopped ordinary tasks and their evidence after exact quiescence checks, including a transaction-time recheck.
  Live, unknown, foreign, persistent and coordinator-owned users remain refused, and uninstall keeps its stricter boundary.
- Capacity candidate lookup now reads canonical `subagent-dispatch.json` and `subagent-harness`, retains legacy fallback and rejects conflicting aliases through the existing owner.
  Previously, a migrated canonical-only configuration could be ignored by headroom even though dispatch used it.
- Active help and watcher labels use canonical roles, while durable wire/state tokens and legacy command filenames remain compatible for this release.
  Removing them requires an announced subsequent breaking migration, not a destructive vocabulary rewrite.
- A practical [user guide](../../docs/user-guide.md) documents workspace entry, projects/discovery, multi-repository requests, sub-agents/domains, workflows, delivery, optional tools, observation, recovery and maintenance.
  README links to it, with release availability and provider limits stated explicitly.
- The release acceptance ledger maps all 39 scenarios and A1-A11 to existing guarantee owners and identifies missing combined evidence.
- Coverage wrappers use the existing resource-aware behavior runner instead of serial shell execution, retaining the same script lists, conflict rules and line-coverage gate.

## Policy and compatibility audit

The whole-system sweep retained the special blocked push URL only for the disposable private upstream-research clone (`crates/multplx-domain/src/lifecycle/upstream_diff.rs` and `docs/upstream.md`); ordinary worker launch tests explicitly reject that restriction and delivery documentation describes inherited ordinary authentication.
Removed policy skills remain only in disposition/history and absence assertions, not installed skill directories.
Deep-review and vplan remain explicit in their command help, task briefs and workflow owner; required review stages apply only to a deliberately selected workflow.
Roles do not restrict delegation in active spawn help.
Legacy `yolo` inputs remain parseable but task authority canonicalizes them to false; historical teardown tests discover an already human-merged PR and do not grant merge authority.
The compatibility boundaries are documented rather than deleting durable tokens or weakening coordination checks.

## Validation environment

The candidate used for the earlier live A11 and retained-home upgrade checks was SHA-256 `18787d10f00daaf82ec201145b799697244019ed8f9862287ead2c94b2109bac`.
Later package generations and their exact validation are recorded below.
Earlier candidate failures below are retained as findings, not counted as final acceptance.

The host is macOS 26.6.2 (25G83), arm64, 15 logical CPUs and 24 GiB RAM, with Rust/Cargo 1.97.1, Node 24.14.1, Git 2.55, tmux 3.7c and Herdr 0.7.4.
Linux verification uses an isolated `rust:1.97-bookworm` container on aarch64 Linux 6.12.76-linuxkit, with the repository's stable toolchain selecting Rust/Cargo 1.98.1, tmux 3.3a and Herdr 0.7.4.
An initial Linux behavior run used root and Debian Node 18, producing invalid permission-test conditions and unsupported TypeScript execution; it is not accepted as a passing platform run.
The corrected environment runs as an ordinary `mxvalidate` user with Node 22.18.0, `gh`, the declared shell/runtime tools, and its own Herdr server.
Only tracked public source is copied into that container; private operational directories are excluded.

## Validation in progress

The following initial checks passed before final closeout:

| Exact command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --locked --workspace` | Passed: 768 tests in 37 result groups, zero failures and zero ignored tests. |
| `cargo build --release --workspace --locked` | Passed in 78 seconds. |
| `target/release/mx test-run --check-coverage` | Passed: 133 scripts, 112 accelerated, 11 serial, ten Herdr, 133 manifest entries. |
| `target/release/mx doc-audience-check` | Initial baseline passed: 90 maintained surfaces and 476 local links; the final changed-document result is recorded below. |
| `MX_CURSOR_LIVE_TESTS=1 tests/mx-cursor-live-e2e.test.sh` | Passed authenticated CLI status and supported option checks for Cursor `2026.09.10-fd3934a`; no model task outcome is claimed. |
| `MX_LAUNCHER_LIVE_E2E=1 tests/mx-launcher-live-e2e.test.sh` | Passed real Codex `0.151.0` and Cursor executable routing and remembered caller context; these are `--version` probes. |

Logs use `/private/tmp/mx-phase12-` paths and will be summarized with final checks below.
The initial Linux source copy also passed formatting, strict Clippy, 770 Rust tests across 37 result groups, and its release build; final changed-source reruns remain required.

The first full macOS behavior run reported 133 scripts, two failures, eight declared gates and 348,370 ms.
The package failure was the new hook-resolution regression running against the pre-fix binary.
The real Herdr presentation fixture failed concurrent recovery with `projection has no exact bound journal`; its unchanged focused retry passed all assertions on Herdr 0.7.4 in 169,938 ms.
Inspection traced that failure to a real recovery gap: an interrupted version-1 projection journal was rejected before the existing conservative flat fallback could run.
The candidate fix proves exact recorded identity and inactive token-correlated endpoints without mutating an unbound endpoint; live, ambiguous and unknown states remain retained.
The unchanged retry is not treated as a fix, and final changed-source validation remains required.

## Coverage investigation

The latest successful merged-main [CI baseline, run 35373191925](https://github.com/KashyapTan/Multplx/actions/runs/35373191925), spent 18m28s in the coverage step.
Its Phase 11 shell wrapper took 353.12s and shared session shell wrapper took 625.66s, with 93.14 percent line coverage.
The preceding [run 35371449197](https://github.com/KashyapTan/Multplx/actions/runs/35371449197) repeated that pattern: 18m12s overall, 352.59s and 616.79s for those wrappers, and 93.14 percent coverage.
The wrappers executed shell contracts serially instead of using the existing conflict-aware runner.

Their unchanged script lists now run through `mx test-run --jobs auto`.
The runner preserves `LLVM_PROFILE_FILE` and the exact `MX_RUST_BIN` together only during instrumented runs; ordinary isolated runs still reject inherited runtime overrides.
The 93 percent threshold, exclusion expression, test inventory and CI timeout are unchanged.
A focused end-to-end instrumented dispatch contract produced 39 distinct profile files, and a lookup-based unit regression verifies both profiling propagation and ordinary isolation without mutating shared process environment.

The candidate gate before the later installed-primary scope fix passed on macOS in **461.63s (7m41.63s)**, with **93.17 percent line coverage**: 71,829 lines, 4,905 missed.
All 771 Rust tests across 31 test result groups passed, including the instrumented shell wrappers.
The Phase 11 wrapper took 117.78s and shared session wrapper 233.52s.
No competing suites ran during this measurement.
This is a macOS-versus-GitHub-Ubuntu comparison, not a same-runner CI speedup claim; the next GitHub run must establish the CI result.

Exact command:

```sh
cargo llvm-cov --locked --workspace --all-targets \
  --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' \
  --fail-under-lines 93
```

The final log is `/private/tmp/mx-phase12-full-coverage-final.log`.
Earlier candidate failures were investigated rather than hidden: stale terminology and brief-transport assertions were corrected, a renamed documentation anchor was repaired, and missing instrumentation propagation initially produced 86.97 percent despite passing tests.
Those incomplete runs are not acceptance evidence; the passing gate includes the coupled instrumentation fix.
The later installed-primary scope and worker-multicall findings require a final changed-source rerun before this becomes release acceptance.

## Candidate checks

The preceding candidate passed `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo test --locked --workspace` (771 tests across 37 result groups, zero failed or ignored), and `cargo build --release --workspace --locked` on macOS.
The final documentation check passed with 93 maintained surfaces and 507 local links.
`target/release/mx test-run --check-coverage` passed with all 133 scripts assigned (112 accelerated, 11 serial, ten Herdr), and `target/release/mx shadow-diagnostic` returned `multplx rust shadow: ready`.
Those initial text-log paths were reused by the later final-candidate run; the historical result summary above is retained, but those paths no longer identify the earlier run.
The older timing JSON is preserved at `/private/tmp/mx-phase12-20260918-retained-final3-shell.json`; current dated logs are identified below.
Full behavior, Linux, coverage and fresh installed model results follow once complete.

## Packaged browser check

The in-app browser loaded the candidate package's actual Viz assets through its Rust loopback service with `tests/fixtures/viz/browser-home.sh` synthetic state in a separate temporary home.
It showed 20 tasks, three coordinators, 23 sessions, 31 attempts, three projects and three domains as distinct facts.
Task expansion exposed attempt generation, brief revision, workflow, allocation and original evidence; the exact brief link opened the expected contained artifact.
Escape closed the artifact dialog and returned keyboard focus to its originating link.
At a 390 by 844 viewport the document measured 386 pixels wide, the long title wrapped, and search for `task-20` showed one record without changing the total task count.
Search text and focus survived polling, and the browser reported no warnings or errors.
The temporary viewport override was reset.
This uses real browser and packaged service execution with synthetic state, not a live 20-agent workload or a new initial-interactivity timing measurement.
The package binary for this check is the initial `b0cb5216857ab35e18b966b4bd89d8fe9e57228bea6ff2a6f8d4511356bb1443` build; final rebuilt package validation is still required.

## Live package finding

The first actual Codex package trial loaded `AGENTS.md`, but session/tool hooks and the foreground watcher failed because packaged adapters sought `runtime/target/release/mx`, absent from the installed layout.
The launcher supplied its installed executable as `MX_LAUNCH_BIN_PATH`, but adapters did not receive `MX_RUST_BIN`.
The version-only launcher test could not detect this failure.
The same trial found `mx --help` unavailable in the installed agent shell because only the public `multplx` entry had been installed.
The launcher and scoped shim fixes now pass the package regression: the synthetic harness invokes installed `mx --help`, SessionStart and PreToolUse with both binary override variables initially unset.
A second actual Codex trial passed installed primary startup, SessionStart, installed `mx`, native researcher start/result, one foreground checkpoint and a second launch attaching to the same owner.
It then exposed a separate installed worker failure: the generated backend launch omitted runtime binary bindings and expanded its brief adapter before the command environment existed.
The worker made the requested local commit only after a supported follow-up supplied the missing brief and the worker manually supplied its runtime path; this is failed/recovered evidence, not a passing installed-worker trial.
The worker launch fix and a fresh no-workaround package trial remain required.
The native researcher did complete a read-only guide task and return the expected token, but manual runtime-binding workarounds used afterward do not establish a passing installed coordination path.

## Installed startup ownership finding

The next fresh installed Codex trial accepted three durable requests and completed a native research probe, but could not claim work because the primary owner remained absent.
This was a real package integration defect, not a successful startup: `primary_scope::matches` recognized a Git primary or persistent-home marker but rejected the non-Git installed runtime, silently suppressing the SessionStart nudge.
The fix binds installed primary scope to the running installed binary's own configuration pointer, the exact configured root and home/state, and a matching regular release marker; it does not introduce an environment-only authority bypass.
Package regression now requires the typed, nonempty startup nudge rather than treating a fail-open empty result as success.
Worker adapter invocation also requires explicit multicall mode when the installed executable is named `multplx`; the strengthened regression executes that adapter under a clean backend environment.
Fresh model validation and changed-source platform/coverage reruns remain pending.

The preceding macOS full suite passed 132 of 133 scripts, including all ten real Herdr suites without skips; the single failure was a stale split-assertion inventory after adding a regression.
The exact inventory was updated to 143 cases without weakening the parity check.
The corrected Linux user initially encountered an empty, root-owned presentation-lock namespace left by the earlier root test run; that empty directory was removed inside the disposable container before rerunning as the ordinary user.

## Further installed-trial findings

The next fresh Codex trial verified the typed SessionStart nudge, exactly one connected primary owner, native research and a second unrelated-directory launch attaching to the same session.
Its first durable request acknowledgement then failed with `caller does not belong to a verified orchestrator session`; no worker launch or model outcome is claimed for this trial.
The evidence fixture is `/private/tmp/mx-phase12-guide-live-final3.j7n4ec`; a fresh no-workaround trial remains required.
An initial helper/depth hypothesis passed focused tests but did not fix the next live trial, so it is not accepted as the root cause.
That fresh trial captured the real owner and direct child shell, then reproduced the ancestry failure.
Direct inspection proved that macOS truncates the `comm` column when followed by `args`: the actual Codex executable appeared as `/opt/homebrew/Ca`, even with `ps -ww`.
A standalone `comm` query returned the full executable path.
The process probe now reads `comm` separately and obtains parent PID and arguments together.
A real spawned executable at a long path ending in `codex` verifies that its complete name survives the probe; the fixture kills and reaps it before asserting.
That regression, the existing core session-lock test and CLI foreign-owner regression pass.
The speculative ancestry changes were reverted; authorization still requires the nearest actual harness within the existing bounded walk to match the recorded owner.

The strengthened package negative scope assertion initially inherited the test harness's intentional `MX_GATE_REFUSE_BYPASS=1`.
It now explicitly disables that bypass for its review-gate rejection case and reports the synthetic harness exit code on failure.
`bin/mx-test-run.sh --timeout-secs 300 tests/mx-release-package.test.sh` passed with exit zero, one script and zero failures in 32,784 ms (`/private/tmp/mx-phase12-audit-package-focused.log`).
The latest normal workspace test attempt passed formatting and strict Clippy but failed `process_fixture_allows_cooperative_group_cleanup`; the run is not accepted as passing.
The unchanged focused fixture reproduced that failure at iteration 234 of a bounded 300-run check.
Its parent shell now records TERM, leaves its foreground wait loop, reaps the child and exits normally instead of exiting from a trap interrupting `wait`; readiness and child-leak assertions remain intact.
The corrected exact compiled fixture passed 1,000 consecutive runs, and `cargo test -q -p multplx-test-support --lib` passed all eight tests; complete changed-source validation remains required.

## Resumed validation on 2026-09-22

Work resumed after the model-usage interruption on the same branch and the same `721bf9d1…` release binary.
The preserved installed trial verified a successful primary request acknowledgement and an unassisted worker launch with a nonempty generated brief and exact accepted starting revision.
This confirms that the macOS process-name correction resolves the previously observed request-authorization failure; worker outcome and remaining installed acceptance are still being checked.
The interruption is not ordinary workload latency and will not be used as a concurrency benchmark.

The resumed macOS workspace run found a dispatch fixture that expected a successful launch despite exhausting its candidate capacity with a retained uncertain receipt and one configured in-use slot.
The success-case setup now provides one additional candidate slot while preserving the failed receipt and retention assertions; the focused test passes.
The corrected Linux ordinary-user run exposed a fixture-layout requirement: service tests create siblings of the source checkout, so placing that checkout directly under `/` was invalid.
The disposable checkout moved to `/home/mxvalidate/work`, and workspace build artifacts containing the old absolute executable paths were cleared before rebuilding.
Neither environment-invalid run is counted as passing platform evidence.
The corrected Linux run then passed formatting, strict Clippy, 776 Rust tests in 37 result groups, the release build, all 133 inventory assignments and documentation validation (93 surfaces, 507 links).
Its full behavior run finished in 225,318 ms with 133 scripts, two failures and eight declared gates; all ten real Herdr suites passed without skips.
The failures concerned cmux absent-target classification and a stale Pi-extension diagnostic; the failing full run is not recorded as a platform pass.
The cmux adapter previously allowed failed `list-panes` output into jq, whose empty-input exit behavior differs between Linux jq 1.6 and macOS jq 1.7.1.
It now checks command success before parsing; focused cmux, process-liveness (11 cases) and lock-bootstrap (five cases) pass on both platforms.
The shared fake process probes now answer the split ancestry query, preserving their existing liveness and stale-extension assertions.
Logs are `/private/tmp/mx-phase12-resume3-linux.log` and `/private/tmp/mx-phase12-resume3-linux-shell.json`.

The installed worker completed its first isolated commit and reported successfully through the documented CLI fallback, preserving the borrowed checkout.
Its optional status MCP handshake failed because the MCP server environment omitted the installed runtime path; the adapter incorrectly sought a source-build binary.
The generated Claude and Codex status-server environments now explicitly bind the packaged runtime, with a clean-environment installed handshake regression.
`target/release/mx test-run tests/mx-spawn-dispatch-profile.test.sh tests/mx-release-package.test.sh --jobs 1 --json /private/tmp/mx-phase12-audit-mcp-focused.json` passed both scripts, including the installed initialize exchange under `env -i`; this is a real local protocol check, not a model-tool invocation.
The second live worker also completed its isolated commit and fallback report, with its nested borrowed checkout preserved.
A concurrent third launch left an uncertain endpoint receipt after a lock refusal.
Short transition publication now waits up to five seconds for independent writers, using the existing lock owner.
A repeat of an interrupted pre-submit launch may reconcile only after fresh proof that its exact recorded endpoint is absent, preserving the attempt, allocation, accepted brief and worktree.
Submitted, running, live, unknown and changed actions remain retained; the whole-task lifecycle lock serializes observation and recovery.
Focused core/domain tests and all six Phase 03 CLI tests pass.
A real retained-home retry then exposed a tmux adapter defect: `display-message` returned the primary pane for a nonexistent named window, causing a false live-endpoint observation.
Exact named-window membership verification and a real tmux regression now pass; native pane targets retain their existing lookup.
The corrected release binary is `fc67ce56594110636af42b19d59b64c0352ce10cf4fe34e0009cc41c51c1baa3`.
Archive `/private/tmp/mx-phase12-release.hhlY4E/package.tar.gz` has SHA-256 `d9e09617304f7c36e3f2949a9bdbedf815b90392c290b2489f83c3771f379af9`.
The full Rust rerun exposed stale actor-state and lifecycle fakes that implement only the old display-message lookup.
Their fixtures now model exact named-window membership and missing sessions without weakening runtime behavior.
`cargo test --locked --workspace` then passed 777 tests in 37 result groups, with zero failures and zero ignored tests; log: `/private/tmp/mx-phase12-tmux-fixture-audit/rust-tests.log`.

## Requested model availability

The user approved the full workload matrix without a time cutoff and requested GPT-6 Luna for all new sub-agents.
A direct `codex exec -m gpt-6-luna -c model_reasoning_effort=medium` probe failed with exit one and a provider error that the model is unsupported with ChatGPT authentication; no workload code or model task ran.
The initial CLI was 0.151.0.
Logs are `/private/tmp/mx-phase12-workload-author/events.jsonl` and `/private/tmp/mx-phase12-workload-author/stderr.log`.
The user requested a CLI update and authorized GPT-5.6 Luna only if GPT-6 Luna remained unavailable.
`HOMEBREW_NO_INSTALL_CLEANUP=1 brew upgrade --cask codex` successfully installed Codex 0.156.0; log: `/private/tmp/mx-phase12-codex-upgrade.log`.
A fresh GPT-6 Luna medium `codex exec` probe returned `LUNA_READY`; log: `/private/tmp/mx-phase12-luna-updated-probe.jsonl`.
New worker and coordinator launches use GPT-6 Luna medium, with no fallback required.
Existing live-trial evidence remains attributed to its actual older CLI and model; the upgrade is not retroactive validation.

## Open release acceptance

Claude and Pi executables and cmux are not installed in the current environment.
Earlier provider deferral is recorded in Phase 04; it does not turn unexecuted Phase 12 provider trials into passes.
A focused user question asks whether to install missing providers or retain explicit unavailable-provider limits, and which disposable forge repository may receive live test branches/PRs.
Local isolated verification continues independently.

The combined live independent/coupled and flat/hierarchical 1/5/10/20-task workloads, model/native telemetry where available, response/disposition latency, quality/rework and human-attention evidence remain open.
Public release publication/download and operational-home activation have not occurred.
Repository filename cutover must wait until release acceptance is satisfied, or the user explicitly elects to keep this checkout dormant.
No next numbered phase exists; release activation is not yet ready.

## Latest full-suite observations

The pre-tmux-fix macOS release suite passed 132 of 133 scripts with eight declared gates in 356,722 ms.
`mx-viz` failed its stale-caller-two assertion during the concurrent 500 ms HTTP-deadline checks; the precise cause is unproven.
An unchanged focused retry passed in 5,303 ms on the newer binary, which does not establish whether the original failure was load-related or intermittent.
No assertion, deadline or service code was relaxed.
Logs are `/private/tmp/mx-phase12-release-shell.log`, `/private/tmp/mx-phase12-release-shell.json` and `/private/tmp/mx-phase12-viz-audit/diagnosis.md`.
A full final-candidate rerun remains required.

The next macOS full run passed 127 of 133 scripts, with eight declared gates, in 354,981 ms.
All ten real Herdr suites and the previously failing Viz suite passed in this run.
Six behavior fixtures failed after the exact named-window lookup change: `mx-actor-state`, `mx-backend`, `mx-send-daemon-marker`, `mx-send-popup-settle`, `mx-send-settle` and `mx-status-snapshot-projection-reconciliation`.
Their fake inventories now match the real named-window contract; all six focused suites pass, but this earlier full run remains a failure.
Inventory-command failure still asserts unknown presence and retains its diagnostic; a separate foreground-command failure preserves verified presence with unknown agent liveness.
The exited-endpoint fixture still produces a genuine missing-session observation rather than an accidentally live inventory.
Logs are `/private/tmp/mx-phase12-final2-shell.log` and `/private/tmp/mx-phase12-final2-shell.json`.
The retained gamma live worker subsequently recovered through one explicit corrected-runtime retry, preserving its original task, attempt, allocation, worktree and brief.
It committed `120409a6acaa067b9763fd81c6deaacf053bdb9c` on `mx/p12-gamma`, with the borrowed checkout preserved.
The ordinary upgrade then refused retained completed worker metadata after provider exit; the installer currently treats every `.meta` as an active runtime user.
No retained record or worktree was deleted to bypass that guard; a narrow authoritative-endpoint quiescence correction is under implementation.

## Useful-work fixture verification

`tests/fixtures/phase12-live-workloads.py` generates the 16 paired workloads without launching providers or including reference solutions.
It supplies 20 independent features across three interleaved repositories and 20 distinct coupled release-bundle tasks with a dependency graph and integration milestones.
Evaluator paths are excluded from worker edit scope, and file-oriented checks use isolated temporary fixtures.
All 40 seed evaluators failed with the expected unimplemented-function error rather than import, name or missing-input errors.
All 40 passed against throwaway reference implementations, and full test discovery passed in each of the four repositories.
Reference validation preserved all seed hashes; implementation copies remain outside the repository and are not given to workers.
Commands include `python3 tests/fixtures/phase12-live-workloads.py --output /private/tmp/mx-phase12-live-seeds-final` and `python3 /private/tmp/mx-phase12-workload-reference/validate.py`.
Generator SHA-256 is `bb24d554d5ab7902936150aef174ca7cf7348facf1368df5ee369f6aa0c9b0cc`; the reference validation harness SHA-256 is `0d1f237595c0c4d1dabd8cfe0a88f6c11a2ecb011947e55737f6c041fb01c7a4`.
This validates benchmark inputs only; the model workload comparison is still pending.

A fresh Codex 0.156/GPT-6 Luna worker initialized the corrected installed MCP server and recorded working and terminal status without the shell fallback.
It committed `8f1b92076aaabf848affbed4957f6626198d8bd3` on `mx/smoke-mcp-report` with a clean worktree and the exact requested `MCP_REPORT_OK\n` content.
The primary brief misstated that literal as thirteen bytes; the worker produced the correct fourteen-byte literal.
Two terminal MCP calls were correctly rejected with `-32602: message must be a string of at most 300 characters`; a concise third call succeeded.
These are message-length validation errors, not MCP transport failures.
The smoke primary used GPT-6 Astra medium; its workers and coordinator used GPT-6 Luna medium.
Comparative workload roots will explicitly use GPT-6 Luna medium as well, rather than inheriting the primary CLI default.
The subsequent packaged A11 smoke exposed an empty `MX_STATE_OVERRIDE` in the persistent coordinator despite a correct private `MX_HOME`.
Its request lookup used a relative inbox path rather than its private state directory, so the trial stopped before child acknowledgement or launch.
A read-only explicit-state lookup confirmed the private request existed; no environment workaround was used to claim a successful child outcome.
The coordinator also prematurely attributed the parent worker result to its own child, which remains a recorded model-quality failure.
The coordinator launch binding correction and its regression test are pending.

Explicit optional review passed from the retained `smoke-mcp-report` worker at commit `8f1b92076aaabf848affbed4957f6626198d8bd3`, with exit zero and a clean worktree.
The command was `mx-deep-review.sh smoke-mcp-report --intent 'Review the completed local-only change for correctness, scope, and exact-byte acceptance. Do not modify files.' --base main` from that assigned worktree.
The supported `MX_DEEP_REVIEW_AGENT` structured-command override selected `codex exec -m gpt-6-luna -c model_reasoning_effort=medium`, with one round, one agent attempt and a 300-second timeout.
The wrapper reported optional review evidence separately from publication; this small fixture had no configured test command.
Earlier wrong-directory and wrong-actor invocations were correctly refused, as was an operator invocation with an extra trailing dot; none launched a reviewer or modified the worktree.
The corrected actor invocation supplied actual model review evidence rather than relabeling those refusals as success.
The native review command has no direct model flag; this evidence applies to the documented custom reviewer command configuration.

## Corrected package and final reruns

The corrected package is `/private/tmp/mx-phase12-release.mzagyN/package.tar.gz`, SHA-256 `4cd1f88e0a189af99163e3699acc96e4ca4e8d7c4ca73d8b285c7e736b62f4ae`, with runtime SHA-256 `18787d10f00daaf82ec201145b799697244019ed8f9862287ead2c94b2109bac`.
Private coordinator launches now bind state, data, projects and configuration to their own home while preserving the installed runtime, parent report route and root capacity bindings.
The coordinator regression executes the generated shell script with inherited parent settings and verifies the private inbox and nested MCP identities; the focused Rust checks and coordinator shell suite pass.
Package upgrade now distinguishes a retained ordinary task from a live runtime consumer using its bounded validated canonical metadata and an exact backend absence observation.
It preserves stopped unfinished work without inventing task completion, rechecks under the existing install/launch barrier, and leaves uninstall strict.
Persistent and coordinator-owned users remain conservatively upgrade-blocked; descendant-safe upgrade is not claimed.
The package regression creates a canonical worker through typed spawn/report, proves byte-identical preservation of metadata, reports, receipts, lease records, worktree and commit, and rejects live, unknown, foreign and transaction-time changed endpoints.
The focused package suite, strict focused Clippy, six Phase 03 spawn tests, release build, formatting and shell syntax passed.
Final macOS formatting, strict Clippy, release build and 778 Rust tests in 37 result groups passed with zero failures and zero ignored tests.
Their dated logs are `/private/tmp/mx-phase12-20260923-18787-{fmt,clippy,rust,build}.log`.
The full behavior suite is in progress; final Linux and unchanged coverage-gate reruns remain pending.

The real retained FINAL5 home upgraded normally to the corrected `18787d10...` runtime from `721bf9d1...`.
The installer succeeded and its installed hash/configuration agreed; a surrounding recorder then failed on a reserved zsh variable, which was a post-install harness error rather than an installer failure.
The 184-entry content manifest over the operational home and project trees was identical before and after: SHA-256 `56fe30bf648d663b8e3d72c1e560169b3038740129ed9642ed755e0ba1c2690b`, with `cmp` returning zero.
No task record, evidence, receipt, lease, worktree or commit was deleted to permit upgrade.

The corrected installed-package A11 smoke used fresh fixture `/private/tmp/mx-phase12-a11-final.hhCptN` and pinned the primary, persistent coordinator and child to Codex 0.156/GPT-6 Luna medium.
The primary acquired owner PID 27912 and provisioned coordinator `a11-coord`, domain `domain-e82548e1a46ba1f698ed63e7`, attempt `attempt-18d7d698a2f97908-45479-1`, with its generated launch script binding the private state, data, projects and configuration paths.
The routed `a11-child` request appeared in that private inbox at immutable base `3fa7c78cdc4306ab170a69b41e48123dc3f39306`.
Acknowledgement correctly refused before the marker-backed coordinator home acquired an owner; normal `bin/mx-session-start.sh` established ownership, after which the coordinator acknowledged, scaffolded the exact accepted scope and launched one Luna-medium child without manual environment overrides.
Child attempt `attempt-18d7d6e54bcc0c20-83391-0` committed `8d13178656894c4ed5718bca9a61e4f90989a7cf`, containing only exact `A11_CHILD_OK\n` in `a11-child.txt`, from the accepted base in allocation `e41eddcc0f61d3508b61eef9966b408ad7eb5354f2bf20f0eb32d9738e80ab4a`.
Its isolated worktree was clean and the borrowed checkout stayed clean on `main` at the base.
The exact child actor then recorded a successful `multplx_status.report_status` done event through MCP with its private home/state and preserved task, attempt, generation and brief identities.

The coordinator model was interrupted immediately after child launch and remained interrupted through completion and relay.
Runtime parent-channel delivery produced private delivery receipts, hop-0 root ingestion and hop-1 root receipts for both durable done envelopes; the MCP done hop-1 receipt is `/private/tmp/mx-phase12-a11-final.hhCptN/install/data/home/state/parent-receipts/parent-inbox-report-18d7d6fca80d7778-89077-0-hop-1-repair-0.json`, SHA-256 `8c05651e9c0bdef3bfa1ade1921de43fdca7e1b8d4a0ca31c212a15f1bd2937e`.
The root recorded durable wake dispositions and acknowledgements without requiring a coordinator summary.
The compact trial record is `/private/tmp/mx-phase12-a11-final.hhCptN/evidence.md`, SHA-256 `8d89dcdf94194fbed9e1782a05dbbd946540b4a48963eb468075ab60e804cbfb`; its reusable Luna fixture recipe is `/private/tmp/mx-phase12-a11-final.hhCptN/fixture-recipe.md`, SHA-256 `ea6c566274d3ecf77c5524e2598b2fc2f0767ada595103a4d7b797a787fb6bc6`.
The child first emitted working and done through the allowed shell fallback before the required MCP done call passed, and an initial coordinator inference recorded a superseded missing-scope question before routed intake arrived; those facts remain preserved.
This bounded marker smoke establishes corrected private-home delegation and model-silent mechanical relay only; it does not complete the useful-work comparison, outage matrix, or broader provider acceptance.

The `18787d10...` full macOS behavior run finished in 352,770 ms with 133 scripts, two failures and eight declared gates.
All ten real Herdr suites and Viz passed.
`mx-daemon-lifecycle-e2e` still expected the old empty coordinator override strings; `mx-status-snapshot-landed-bounds` exposed a shared fake inventory that incorrectly listed endpoints the same fixture marked dead.
The original isolation and section-cap/omission assertions are retained while those fixtures are corrected.
Dated results are `/private/tmp/mx-phase12-20260923-18787-shell.log` and `/private/tmp/mx-phase12-20260923-18787-shell.json`; this run remains a failure.

The two remaining macOS fixture mismatches are corrected: private-home launch assertions now require exact private paths, and the shared inventory fake omits its declared dead endpoints.
Focused daemon lifecycle, catchup forge, landed bounds and projection reconciliation all pass without changing cap or omission expectations.
Linux formatting and strict Clippy passed, but its full Rust run stopped at `ancestry_preserves_a_long_harness_executable_name`: spawning a freshly copied fixture executable returned Linux `ExecutableFileBusy` (`Text file busy`).
That failure is retained at `/private/tmp/mx-phase12-20260923-18787-linux-rust.log`; no production probe failure is inferred from it.
A test-only portable executable fixture correction is pending, preserving real ancestry checks and process cleanup.

## September 23 repository rerun and public-command follow-up

The corrected `18787d10f00daaf82ec201145b799697244019ed8f9862287ead2c94b2109bac` runtime passed the macOS release sequence in `/private/tmp/mx-phase12-validation.66SBog`.
Commands were `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo test --locked --workspace`, `cargo build --release --workspace --locked`, `target/release/mx test-run --check-coverage`, `target/release/mx doc-audience-check`, and `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-phase12-validation.66SBog/macos-shell.json`.
Rust reported 778 passed, zero failed and zero ignored across 37 result groups.
The inventory reported all 133 suites: 112 accelerated, 11 serial and 10 Herdr.
Documentation reported 93 surfaces and 507 local links.
The complete behavior run reported 133 suites, zero failures, eight explicit gates and 350658 ms elapsed; the JSON records the gate boundaries.
These checks precede the public-command follow-up below and are not evidence for a subsequently rebuilt runtime.

The corrected A11 model trial also reported an in-session `multplx task` rejection while `mx task` worked.
The initial explanation that explicit multicall mode alone caused this was disproved: the exact installed candidate accepts `task --help` both with and without `MX_MULTICALL_EXPLICIT=1`, and its internal CLI has a task command.
A provisional flag-removal change was therefore not accepted as the fix; existing installed wrappers also depend on explicit internal dispatch.
Public command lookup through an older inherited `multplx` executable is being investigated with an isolated stale-executable sentinel, preserving real private installations.
The original live failure, the provisional focused failure and the subsequent corrective checks remain distinct evidence.

The corrective change adds `share/shell/shims/multplx`, routing public commands through the captured installed executable's explicit launcher entry.
The existing internal `mx` shim and hook dispatch environment remain intact; the provisional provider/shell flag removals were reverted.
Package validation requires the new public shim.
The regression launches a provider through the installed launcher with a stale `multplx` sentinel earlier in inherited PATH, verifies public task/project commands and internal wrappers, and proves the stale executable was never invoked.
It also exercises the activated shell boundary.
The release build, `tests/mx-release-package.test.sh`, `tests/mx-launcher-shell.test.sh`, `tests/mx-coordinator-spawn.test.sh`, shell syntax and whitespace checks passed after this correction.
These are executable-boundary fixtures with a fake provider, not new live model evidence.
The new candidate is `/private/tmp/mx-phase12-release-public-command/package.tar.gz`, SHA-256 `96dd12b7880e92b19b68ac50db1f5aa608488c695853e005b8b917514535ab0c`, runtime SHA-256 `ca7d6fdd1ad634a0770e459c28eeabcb4bde831d48e0152f45e3af0efcb0f030`.
The bounded change report is `/private/tmp/mx-phase12-public-dispatch/result.md`; raw provider-tool records are retained in `corrected-events.jsonl` beside it.

On the ordinary-user Linux container source copy, the corrected tree passed `cargo fmt --all -- --check`, strict all-target/all-feature Clippy, `cargo test --locked --workspace` and the locked workspace release build.
Linux reported 780 passed, zero failed and zero ignored across 37 result groups; the platform-specific total differs from macOS.
The exact logs are `/private/tmp/mx-phase12-validation.66SBog/linux-{fmt,clippy,rust,build}.log`.
The updated full Linux behavior suite, final coverage gate and useful-work model matrix remain in progress or pending.

The final Linux behavior run on this candidate completed 132 of 133 suites successfully, with eight explicit gates and 203939 ms elapsed.
The failed suite was the fake-Herdr `tests/mx-backend-herdr.test.sh`, where the bordered empty-composer assertion returned `pending`.
It is not a live Herdr model failure.
The result and complete output are `/private/tmp/mx-phase12-validation.66SBog/linux-shell.json` and `linux-shell.log`.
A focused read-only reproduction confirmed that POSIX-locale GNU grep matches the box-drawing footer against the default Unicode bracket class `^[❯›]`; matching whole prompt glyph alternatives avoids that false structural match.
The assertion remains unchanged while a portable correction and focused regression are completed.

The Herdr default now uses `^(❯|›)` rather than a Unicode bracket class, leaving explicit user expressions and the existing content/ownership safeguards unchanged.
The existing suite adds five C-locale assertions: empty bordered and bare agent composers, a rejected box-only footer, preserved pending typed text, and an unknown ordinary shell prompt.
`bash tests/mx-backend-herdr.test.sh` and `bash tests/mx-composer-lib.test.sh` both passed on macOS and in the Linux container as `mxvalidate` with `HOME=/home/mxvalidate`.
These focused commands and their exit-zero outputs are retained in `/private/tmp/mx-phase12-herdr-linux/fixed-events.jsonl`.
The previous Linux failure remains recorded; it is not replaced by a retry without a source correction.

## Combined corrected-tree validation

The final local package is `/private/tmp/mx-phase12-candidate.ANeuVC/package.tar.gz`, SHA-256 `0af9543105be0fa07585991a3c916d8fba482d8cd9acef314193e942e6b02e92`.
Its runtime remains `ca7d6fdd1ad634a0770e459c28eeabcb4bde831d48e0152f45e3af0efcb0f030`; the asset generation includes both the scoped public command and the portable Herdr prompt match.
All sixteen isolated workload installations use this same verified package, and their `candidate.json` and `initial-repositories.json` records preserve package identity, configured budget, seed commits, dirty-checkout sentinels and models.
No workload model session had launched at installation time.

The complete macOS release sequence passed again on the corrected tree, with logs in `/private/tmp/mx-phase12-verified.sX0O8V`.
Commands are the same seven-command release sequence recorded above, with `macos-shell.json` in that new directory as the behavior artifact.
Rust reported 778 passed, zero failed and zero ignored in 37 result groups.
The behavior suite reported 133 suites, zero failures, eight explicit gates and 372470 ms elapsed.
Formatting, strict Clippy, locked release build, complete inventory and documentation checks passed; documentation still reports 93 surfaces and 507 local links.
Candidate fixture installation overlapped part of this ordinary test run, so its elapsed time is not an uncontended performance benchmark.

The corrected Linux full behavior rerun passed all 133 suites, with eight explicit gates and 204551 ms elapsed.
Its exact command was `target/release/mx test-run --all --jobs auto --json /home/mxvalidate/phase12-verified-shell.json`, executed with `docker exec --user mxvalidate --env HOME=/home/mxvalidate --workdir /home/mxvalidate/work mx-phase12-validation`.
The host copies are `/private/tmp/mx-phase12-verified.sX0O8V/linux-shell.json` and `linux-shell.log`; inventory and documentation results are beside them.
The Linux Rust formatting, strict Clippy, 780-test workspace run and locked release build remain in `/private/tmp/mx-phase12-validation.66SBog/linux-{fmt,clippy,rust,build}.log`; the subsequent correction changed only shell source and its regression.
Both platform full behavior results now include the public-command and portable-composer corrections.

## Newly identified dependency prerequisite

Read-only inspection before the coupled workload found a missing end-to-end connection between accepted task dependencies, canonical scheduling and completion.
Admission in `crates/multplx-backend/src/headroom.rs` releases a dependency only when its canonical `schedule.state` is `completed`.
Normal reports preserve accepted done evidence but do not publish that schedule transition; `RequestStore::record_completion` updates the routed intake completion fact without updating the task schedule.
The retained live A11 child `a11-child` has successful done evidence and commit `8d13178656894c4ed5718bca9a61e4f90989a7cf`, while its canonical schedule still says `running`.
No production writer of `WorkState::Completed` was found; existing dependency and review-queue tests construct that state directly.

There is also an intake handoff gap: new spawn bindings construct `ScheduleFacts::default()`, and the inspected ordinary spawn path does not copy the routed request's dependency IDs into the task schedule consumed by admission.
Thus the intake graph, workflow-run graph and canonical task/admission graph cannot be treated as an already verified unified execution path.
The current direct-task path may omit its declared gate rather than merely wait forever; the focused end-to-end reproduction remains to be recorded.
Workflow-run dependency handling is a separate implemented owner and does not establish ordinary task admission correctness.

The user was asked whether to implement this missing prerequisite in the Phase 12 branch or leave Phase 12 blocked on it, as requested for missing prerequisites.
Until that scope answer and correction, the expensive model matrix is held; manually sequencing model work or editing canonical records would not satisfy the required dependency guarantee.
Repository test passes and local measurements do not close this acceptance gap.

## Corrected-tree coverage and local measurements

The exact unchanged coverage gate passed with 93.15% line coverage (72212 lines, 4949 missed), 778 passing tests, zero failures and zero ignored tests.
The command was `/usr/bin/time -p cargo llvm-cov --locked --workspace --all-targets --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' --fail-under-lines 93`.
The log is `/private/tmp/mx-phase12-verified.sX0O8V/coverage.log`; elapsed time was 447.46 seconds, with 492.62 user seconds and 290.97 system seconds.
The Phase 11 and shared-session behavior wrappers took 113.34 and 226.29 seconds respectively.
No competing validation or model workload was intentionally running during this measurement.
The earlier hosted coverage step took 18m28s, but different hosts and cache conditions prevent treating the local 7m27.46s result as a measured hosted-CI speedup.
A hosted branch run remains required to confirm that improvement; the threshold, exclusions and required test inventory were not weakened.

The following commands then completed sequentially with exit zero, using runtime SHA-256 `ca7d6fdd1ad634a0770e459c28eeabcb4bde831d48e0152f45e3af0efcb0f030`:

```sh
python3 tests/fixtures/measure-workspace-entry.py --binary target/release/mx --output plans/lean_redesign/phase12-workspace-entry-performance.json --overlap 'No intentional competing validation workloads; ordinary workstation activity not controlled.'
python3 tests/fixtures/measure-worktrees.py --binary target/release/mx --output plans/lean_redesign/phase12-worktree-performance.json --overlap 'No intentional competing validation workloads; ordinary workstation activity not controlled.'
python3 tests/fixtures/measure-viz-service.py --output plans/lean_redesign/phase12-viz-performance.json
```

The [workspace-entry observations](phase12-workspace-entry-performance.json) cover 1/5/10/20 tasks with one serial trial per count.
At 20 tasks, discovery took 964.494 ms, median intake 352.685 ms, portfolio projection 65.071 ms and median allocation startup 371.318 ms.
The [worktree observations](phase12-worktree-performance.json) record a 362.100 ms median acquisition and 257.678 ms median same-request retry at 20 tasks.
Borrowed checkouts remained unchanged, ignored caches were retained, fresh fallback paths were distinct, and provider/download sentinels remained untouched.
These are local Git/filesystem costs, not model launch or concurrent execution measurements.

The [Viz observations](phase12-viz-performance.json) met the 250 ms cached-API p95 and 2000 ms healthy 20-task refresh p95 targets.
At 20 tasks the deterministic service input measured 15.896 ms cached p95 and 44 ms healthy refresh p95; the real filesystem projection with no live providers measured 15.883 ms and 184 ms respectively.
With one synthetic stalled actor and twelve callers, cached p95 was 11.725 ms and a healthy event appeared in a fresh partial view after 2232.677 ms.
The stalled projection is a fault measurement, not a failed healthy-refresh target or evidence of a live provider outage.
There were zero sampled API errors and zero transient condition-poll errors.
The raw artifacts retain source/script hashes, host details, individual samples and limitations; none closes the pending A8/A11 live model comparison or ordinary dependency prerequisite.


## Authorized prerequisite correction and reduced live scope

On 2026-09-23 the user authorized fixing the ordinary dependency integration gap in this Phase 12 branch and requested a smaller live example to reduce time and model usage.
The focused correction is delegated to GPT-6 Luna medium and remains subject to public-API regression checks and changed-candidate validation.
The accepted live comparison is now four five-task trials: independent/coupled work crossed with flat/hierarchical delegation, totaling 20 workers, six coordinators and four roots.
The original sixteen installed fixtures remain retained; only the four five-task cases will execute after their package is refreshed.
The porting contract and phase plan explicitly record this scope adjustment; full live 10/20-task performance is unmeasured, not passed.
The local scale measurements and all correctness, dependency, capacity, recovery and visualization targets are retained.


## Ordinary task handoff correction

The authorized prerequisite correction connects ordinary accepted intake to the existing spawn and report owners.
Spawn now carries the accepted request's project, checkout, full starting commit and dependency edges into its canonical binding, including the coordinator-owned worker path.
A changed borrowed checkout HEAD cannot silently replace the accepted `--start` commit.
Missing canonical predecessors produce an actionable refusal while preserving the routed request for retry; unfinished canonical predecessors hold admission.
Owner lookup is bounded to registered same-root coordinator homes and refuses ambiguous task identities rather than choosing an arbitrary match.
Qualified dependency identities preserve their actual owner state through lineage validation and admission.

A task-bound current `done` report marks implementation complete only when typed delivery evidence matches the current attempt, accepted brief and actual worktree HEAD, or a report/coordination assignment supplies an existing regular result artifact.
Plain `done` remains accepted status evidence with a diagnostic, without silently releasing dependency gates.
Brief changes, replacement attempts, renewed working reports and a changed current delivery commit reopen completion.
Checks, review, PR readiness and human merge remain separate facts.
CLI help, generated briefs, the release contract and delivery/user guides describe this path; benchmark-only instructions are not its sole documentation.

The public CLI regression in `phase03_spawn::routed_dependencies_wait_for_current_completion_and_preserve_the_accepted_start` exercises real temporary Git and actual intake, spawn, task-model evidence, report and queue-drain commands with a fake cmux transport.
It checks the unfinished gate, plain done, current typed completion, renewed working, stale brief rejection, fresh completion, the same queued identity and its frozen starting revision.
It is deterministic integration evidence, not a live provider pass.
Domain fixtures exercise missing-predecessor retention/retry and registered cross-home resolution without hand-seeding completed schedules.

Initial cross-home fixture failures were fixed by creating the required seeded-home surfaces and canonicalizing the fixture home path on macOS; production identity checks were not relaxed.
An initial Linux documentation check also exposed that the newly linked measurement JSON files were absent from the tracked-only source staging inventory; they are now intent-to-add and included in that inventory.
Earlier partial logs remain in `/private/tmp/mx-phase12-dependencies-verified`; final changed-tree checks are being collected separately in `/private/tmp/mx-phase12-dependencies-final`.
The expanded live comparison remains pending the corrected package and final checks.

## Corrected dependency candidate: complete repository checks, 2026-09-23

The final dependency source passes `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` and `cargo test --locked --workspace` on macOS and Linux.
The macOS Rust run records 780 passed tests across 37 result groups, with zero failures or ignored tests.
The Linux run records 785 passed tests across 38 result groups, with zero failures or ignored tests.
Platform-specific tests and wrapper counts differ; these are the actual log totals rather than an assumed shared count.
Both platforms pass `cargo build --release --workspace --locked`, `target/release/mx test-run --check-coverage`, `target/release/mx doc-audience-check` and the full `target/release/mx test-run --all --jobs auto --json <artifact>` suite.
The macOS full behavior run records 133 scripts, zero failures and eight gated skips in 413064 ms; Linux records the same counts in 205587 ms.
These ordinary test elapsed times are not uncontended speed comparisons.
The eight gated skips are not live provider passes.

Logs and timing JSON are retained under `/private/tmp/mx-phase12-dependencies-final`: `current-{fmt,clippy,rust,build}.log`, `macos-shell.{log,json}`, `linux-release.log` and `linux-release-shell.json`.
The final warning-free macOS release rebuild took 58.73 seconds and produces runtime SHA-256 `b0f73a62a2b224d073771bef09d55f1cc41bb17b7c5ff52fed385bdd25e5f57c`.
The earlier macOS behavior binary was built during removal of an unused variable; the final source is independently covered by the subsequent strict Clippy and full Rust runs, and the final package uses the warning-free rebuild.
The Linux complete run uses the final staged source.

A Linux shell failure exposed malformed request data retained by an earlier assertion in `mx-coordinator-spawn.test.sh`.
The fixture now removes that deliberately malformed request after its strict rejection assertions and before testing an unrelated implementation-domain guard.
Production fail-closed parsing and both assertions remain intact.
A nested-owner unit fixture also retained an intentionally ambiguous duplicate predecessor into its later resume assertion; only that duplicate fixture record is removed after verifying ambiguity rejection.
The final complete runs above supersede these failed intermediate attempts without hiding them.
Final coverage and the four reduced live trials remain pending.

The final unchanged `cargo llvm-cov` command recorded above passes on the dependency candidate: 93.06% line coverage, 72671 included lines and 5044 missed lines, exit 0.
`/usr/bin/time -p` records 453.62 seconds (7m33.62s); `current-coverage.log` retains the complete result.
No model trial or other heavy repository validation overlapped this run; package creation and four idle fixture upgrades overlapped part of it.
This local elapsed time does not establish the hosted CI speedup.
The candidate archive is `/private/tmp/mx-phase12-dependency-candidate/package.tar.gz`, SHA-256 `b34b797c682cf241de59885e87e5fd301c2ff3a5f78d260c7347b80d06743bbc`.
All four selected idle trial homes upgraded to runtime `b0f73a62a2b224d073771bef09d55f1cc41bb17b7c5ff52fed385bdd25e5f57c` before live execution.
The maintained documentation check now passes 93 surfaces and 511 local links.
