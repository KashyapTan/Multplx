# Phase 12 release acceptance ledger

This ledger maps every [porting-guide scenario](../../porting.md#validation-strategy) to its verification owner and records the completed combined-release evidence and its limits.
A fixture pass proves only the deterministic contract it exercises; an executable probe does not prove authenticated model work, and historical live evidence does not verify a new package.
The scenarios combine deterministic fault tests, real local Git/process/terminal integration and the expressly required live trials; they do not require 39 separate model sessions.
Unavailable provider and human-merge observations remain limits, not passing live results.
The [implementation record](phase12-implementation.md) owns exact commands, results, source identity and release status.
No test below may be omitted from the complete repository suites merely because its guarantee also appears in another row.

## Scenario evidence map

Rust test targets below live in `crates/multplx-cli/tests`; named shell suites live in `tests/` with the `.test.sh` suffix.
Domain/core unit tests are included by `cargo test --locked --workspace`.

| Scenario | Deterministic and local integration owners | Release evidence and limits |
| --- | --- | --- |
| 1: ordinary delegated delivery | `phase03_spawn`, `review_runtime`, `mx-push-service`, `mx-brief` | Phase 06 records live Codex/GitHub publication; current release tests verify inherited ordinary credentials and publication boundaries. PR #48 is the reviewable implementation PR; no new disposable worker PR is claimed. |
| 2: nested CLI/native delegation | `phase04_coordination`, `mx-subagent-pretool-check`, `mx-supervise-daemon-native`, native observation unit tests | Corrected installed Codex/GPT-6 Luna primary, private coordinator and child delegation passed with private inbox ownership, a distinct isolated child commit and nested MCP reporting; other providers remain unverified, and the adjusted workload comparison is recorded below. |
| 3: restart and retained work | `session_runtime`, `mx-session-start-process-liveness`, `mx-wake-queue`, `mx-daemon-liveness`, `mx-worktree` | Real process/session fixtures verify restart retention; Phase 05 records live Codex child continuity. The retained first-trial home also passed an installed upgrade with task metadata, journals and evidence preserved. |
| 4: parallel wakes and one owner | `phase04_coordination`, wake/lock unit tests, `mx-watcher-lock`, `mx-report` | Five simultaneous flat workers produced revision-bound reports; all 26 recorded wakes were durably disposed and acknowledged with zero repeated wake keys. The hierarchy also delivered child outcomes through private owners. |
| 5: publication retries | `review_runtime`, `mx-push-service`, `mx-pr-check-security-publication-migration` | Phase 06 has a live retry trial; current deterministic forge tests retain exact retry and partial-publication assertions. No new remote worker retry is claimed. |
| 6: human-only merge | `mx-pr-merge`, `mx-subagent-pretool-check`, `mx-workflow`, supervision guard tests | Supported integration backstops retain merge/bypass refusal coverage. No human merge or independently configured remote protection is simulated or claimed. |
| 7: explicit optional tools | `mx-deep-review`, `mx-deep-review-config-contract`, `mx-vplan`, `mx-removed-deps` | Explicit installed Codex/GPT-6 Luna review passed on commit `8f1b92076aaabf848affbed4957f6626198d8bd3` through the supported structured reviewer override; broader provider evidence is unavailable. |
| 8: ordered workflows | `mx-workflow`, `mx-workflow-lib`, workflow unit tests | Current workflow tests exercise declared stage ordering, outputs and explicit interactions; no new authenticated model workflow run is claimed. |
| 9: mixed legacy migration | `mx-state-migration`, migration unit tests | Real temporary Git/process fixtures establish migration semantics; no private home is used as a fixture. |
| 10: public vocabulary | `mx-naming`, `mx-instruction-owners`, `mx-documentation-audiences`, `mx-bin-runtime-inventory`, `mx-release-package` | Packaged contract, help and actual UI inspection match the candidate; legacy wire readers remain explicitly compatible. |
| 11: research and accepted scope | `mx-workflow`, `phase04_coordination`, decision/workflow unit tests | The user-approved workload reduction and coordinator-authored versioned briefs provide actual scope decisions; deterministic workflow/decision tests cover explicit planning interactions. |
| 12: mixed 20-task visualization | `mx-viz`, `mx-system-snapshot-view`, `mx-status-snapshot-projection-reconciliation`, browser fixture | Synthetic task records establish presentation only; installed UI acceptance is recorded separately. |
| 13: comparative task workloads | Existing workspace/worktree/Viz measurement scripts | All four approved five-task trials completed: 20 worker results and all selected integrated checks pass. Raw timing, resource/usage observations and rework are recorded with observer/mixed-runtime limits; no velocity-gain claim. |
| 14: interruption, duplication, stale results, slow backend | `phase04_coordination`, `phase05_parent_channel`, `mx-watch-triage`, `mx-viz`, wake/report tests | Deterministic crash/duplicate/stale/slow-provider injection passes separately from the recorded live coordinator-model outage and same-attempt queue recovery. |
| 15: multi-record recovery | transition/allocation/migration/publication/parent-channel unit tests, `mx-transition-lib`, `mx-state-migration` | Owner-specific fault boundaries retain their assertions; no second state authority is introduced. |
| 16: capacity and dependencies | headroom/workflow unit tests, `mx-headroom`, `mx-dispatch-queue`, `phase04_coordination` | The authorized prerequisite repair now passes actual CLI intake-to-spawn, typed completion, stale-brief and queued-drain regression checks with real Git and mocked transport; see the implementation record. Both hierarchical trials reached five delegated sessions (three coordinators/two workers), with queued progress and dependency-gated completion; final coupled root response was 6.775 seconds. Deterministic tests separately cover priority, aging and cycle refusal. |
| 17: revision-bound answers and evidence | `phase05_parent_channel`, `review_runtime`, `mx-decision-hold-lifecycle`, delivery/workflow unit tests | Human answer/review latency remains unmeasured. |
| 18: Viz usability and performance | `mx-viz`, service measurements and browser acceptance | Cached-API and healthy-refresh targets pass the recorded local measurements; five actual packaged-browser 20-task reload-and-search trials take 103-171 ms, below two seconds. Live two-viewer focus/evidence checks pass separately; physical hidden-tab behavior is not inferred. |
| 19: nested discovery from dev root | `mx-workspace-discovery`, `phase11_workspace`, `mx-release-package` | Local Git/filesystem integration; no source cwd or URL dependency. |
| 20: one chat, three repositories | `phase11_workspace`, `phase04_coordination`, `mx-release-package` | The installed independent flat trial produced five verified worker commits and passing integrated results across three repositories, preserving borrowed checkouts. The hierarchical pair also passed worker/integration checks after explicitly recorded mixed-runtime recovery. |
| 21: discovery identity edge cases | project registry/discovery unit tests, `mx-workspace-discovery` | Real temporary clones, worktrees, symlinks, nested repositories and missing paths. |
| 22: concurrent entry and retry | `phase11_launcher_terminal`, `mx-launcher-connection`, `phase11_workspace` | Real tmux/PTY with synthetic harness; actual provider connection/continuation remains provider-specific. |
| 23: package install/upgrade/uninstall | `mx-release-package`, `mx-launcher`, `mx-launcher-shell` | Local verified package install/upgrade/uninstall preserves state; public release publication/download has not occurred. |
| 24: terminal interactions | `phase11_launcher_terminal`, workspace TUI unit tests, `mx-launcher-connection` | Real terminal fixture with synthetic harness; package interaction is separate from live model execution. |
| 25: borrowed dirty and remote-free projects | `phase11_workspace`, `mx-workspace-discovery`, `mx-worktree`, registry tests | Real local Git; working-change capture remains explicitly unsupported. |
| 26: arbitrary caller directories | `mx-launcher`, `mx-launcher-live-e2e`, `mx-release-package` | Version probes establish executable routing only; the four live trials additionally start installed Codex from owned non-repository caller directories. |
| 27: no Treehouse or source build | `mx-release-package`, `mx-removed-deps`, `mx-worktree` | Forbidden-tool sentinels fail if cargo/rustc/Treehouse is invoked by installed runtime. |
| 28: repeat-safe allocation and retention | worktree unit tests, `phase03_spawn`, `mx-worktree` | Real temporary Git plus injected failures; persistent reservation outlives endpoint. |
| 29: conservative release and reuse | `mx-worktree`, `mx-teardown`, worktree unit tests | Dirty/untracked/ignored/open-PR/uncertain work stays retained; no user checkout reset. |
| 30: legacy worktree transfer | `mx-state-migration`, migration unit tests | Exported legacy metadata and real temporary Git/process fixtures; no live external pool takeover. |
| 31: exact backend launch and setup cost | `phase03_spawn`, backend suites, worktree measurements | Real tmux/Herdr integration is distinguished from fake transport; cmux is unavailable. Worktree setup costs and live workload observations are recorded with their limits. |
| 32: three domains plus direct work | `phase05_domain`, `mx-coordinator-spawn`, headroom tests | The independent hierarchical trial ran three private coordinators alongside direct root work under the shared budget; all five requested implementations and selected integrated checks passed. Runtime repairs and timing limits are recorded. |
| 33: interrupted domain creation/handoff | `phase05_transfer`, `phase05_domain`, spawn/transfer unit tests | Deterministic interruption and repeat-safe ownership. |
| 34: model-silent mechanical relay | `phase05_parent_channel`, parent-channel/report/delivery unit tests | A private foreground watcher automatically relayed a child done event while its coordinator model was SIGSTOP-paused; the root received the original hop-1 envelope before that same model resumed. Root CLI processing, the earlier manual-relay diagnostic and mixed-runtime limits are distinguished. |
| 35: exact correlation and bounded missing replies | `phase05_parent_channel`, `mx-pending-reply`, `mx-send-strict`, decision tests | Wrong home/generation and stale answers cannot settle current work. |
| 36: child continuity, transfer and retirement | `phase05_transfer`, `mx-daemon-lifecycle-e2e`, `mx-teardown` | Real Git/process transfer and retirement fixtures preserve ownership, messages and children; Phase 05 supplies historical live child-continuity evidence. No new live-model transfer is claimed. |
| 37: shared domain capacity and health | headroom/parent-channel tests, `mx-headroom`, `mx-system-snapshot-view` | Live high-water is five delegated sessions plus root in both topologies. Coupled root progress replies were 3.553 seconds flat and 6.775 seconds hierarchical; coordinator idle/stalled handling is recorded separately from endpoint liveness. |
| 38: idea binding and overlapping domains | `phase05_domain`, `mx-coordinator-spawn`, domain tests | Explicit binding and isolated implementation; selecting a project never implicitly creates a coordinator. |
| 39: migration, hierarchy and equal-budget comparison | `mx-state-migration`, `phase11_workspace`, snapshot/TUI/Viz tests | All four approved flat/hierarchical independent/coupled five-task trials have verified results; local scale, packaged views, migration fixtures and stalled-viewer measurements remain separately labeled. |

## Architecture cross-check

| Contract | Canonical implementation owners | Evidence scope |
| --- | --- | --- |
| A1 | Core filesystem/transition primitives and domain mutation receipts | Fault tests, exact ownership, no database or competing authoritative projection. |
| A2 | `lifecycle/subagent_model`, report validation and delivery evidence | Task/attempt/generation/brief identity and historical stale results. |
| A3 | Core wake queue, operational input, pending replies, parent channel and publication receipts | Claim/disposition/acknowledgement, repeat-safe routing and external-outcome reconciliation. |
| A4 | Supervision, system snapshot and Viz service | Bounded probes, partial results, shared cache and observable freshness. |
| A5 | Backend headroom and workflow dependency owner | Root-scoped capacity, workflow dependencies, ordinary intake-to-admission propagation, registered descendant ownership and current typed completion pass deterministic regressions; the completed live reduced comparison additionally verifies shared capacity and cross-home canonical dependency resolution. |
| A6 | Accepted briefs, decisions, parent outcomes and delivery review queue | Original artifacts, exact revisions and separate readiness/merge facts. |
| A7 | Shared portfolio, terminal presentation and `share/viz` | Read-only browser, keyboard/freshness/evidence and task/session distinctions. |
| A8 | Measurement scripts and the combined live trial | Four completed live trials record useful results, quality/rework, raw throughput, response/disposition latency and resource/root-usage observations; unavailable human-review/full-provider billing and live 10/20 scaling are explicit. |
| A9 | Launcher, project registry/discovery, durable task intake and package installer | Installed arbitrary-directory entry and immutable per-task project bindings. |
| A10 | Built-in worktree owner and explicit legacy migration | Git ownership, safe retention/reuse and no Treehouse runtime dependency. |
| A11 | Domain owner, parent channel, handoff and shared admission | Deterministic hierarchy mechanisms are complemented by the two completed live hierarchy trials, silent-model relay and exact retained-request recovery. |

## Cutover boundary

The implementation was accepted under the user-approved reduced workload and merged in PR #48; PR #49 subsequently repaired the terminal workspace.
The user restored canonical `AGENTS.md` in `2f57d21`, and current instruction, launcher and documentation checks validate that tree.
Earlier records of dormant `AGENTS_E.md` describe historical implementation constraints.
Source-download installation is published on `main`; a public prebuilt release and migration of any private operational home remain separate operator actions.
The post-merge audit below distinguishes present verification from historical results whose raw artifacts are no longer available.

## Original workload design and approved reduced execution

The original A8/A11 comparison specified 16 separate trials: flat versus hierarchical delegation, independent versus coupled work, and 1/5/10/20 accepted tasks.
Use fresh package-only homes and identical seed commits and briefs for each paired trial, rather than treating checkpoints within one cumulative run as separate workloads.
Keep the same total six-session budget, including the root conversation, and two-worker headroom in both topologies, recording actual active sessions rather than equating accepted tasks with concurrency.
Because admission accounting charges delegated sessions, configure five delegated slots alongside the single root and verify the observed total.
Use one scoped coordinator for the one-task hierarchy trial and three for larger trials.
This entails 144 worker executions, 20 coordinator launches and 16 initial root prompts; these are planned session/prompt counts, not measured model API calls or token usage.
A runtime estimate of 18-30 hours is planning guidance, not benchmark evidence.
The user approved proceeding without a time cutoff and requested GPT-6 Luna for new sub-agents.
Codex CLI 0.151 rejected that model with ChatGPT authentication.
At the user's request, Homebrew upgraded the CLI to 0.156; a fresh GPT-6 Luna medium model probe then passed.
All new worker and coordinator launches use GPT-6 Luna medium; the authorized GPT-5.6 Luna fallback was unnecessary.

Use a fixed useful feature bank across three small release-toolkit repositories for independent work, with isolated module slots and golden tests.
Use a fixed dependency graph in one release-bundle repository for coupled work, with each tested prefix ending in integration of the preceding committed artifacts.
For coupled work, give the root the complete task graph once, then submit ready stages against immutable base commits containing verified predecessor changes.
Do not retarget an already accepted task or imply that sibling worktrees share uncommitted edits.
Record staggered durable intake separately from the total requested workload and start overall timing at the initial root request.
Local integration of fixture commits is not a human forge merge.
Run the same task-specific checks against worker commits and record integration defects and rework; marker-only commits are insufficient for this comparison.
Capture request, admission, wake, report and parent-channel timestamps, periodic headroom and portfolio snapshots, task commits and test results using existing commands and ordinary JSON/CSV files.
Calculate queue and disposition latency percentiles, throughput, active-session high-water marks, repeated-event rates and snapshot freshness from those records.
Record unavailable provider usage and human-review measurements explicitly.
Include a coordinator outage with independent children and a 20-task run with multiple Viz viewers and a stalled observation provider.
Run trials sequentially so competing verification does not invalidate equal-budget measurements.
The original full live matrix is superseded by the user-approved reduced scope below; the dependency guarantee must still be demonstrated rather than replaced by manual sequencing.

For hierarchical trials with five or more tasks, keep task-01 as direct root-delegated implementation and assign the remaining tasks to three coordinators.
This exercises scenario 32's simultaneous direct and domain work without changing task content, total worker count, coordinator count or the six-session limit.
The one-task hierarchical trial uses one coordinator and one child, while its flat pair uses one direct worker.


## User-approved reduced live scope (2026-09-23)

The user authorized fixing the ordinary dependency prerequisite in this branch and reducing the large example to save time and model usage.
Execute four five-task trials: independent/flat, independent/hierarchical, coupled/flat and coupled/hierarchical.
This uses 20 workers, six coordinators and four roots instead of 144 workers, 20 coordinators and 16 roots.
Keep the same feature specifications, immutable paired seed commits, six-session total budget, two-worker headroom and GPT-6 Luna medium model.
The hierarchical cases still combine one direct task and three scoped coordinators.
Preserve correctness checks, independent progress during blocked dependencies, durable completion/relay, capacity observations and interruption recovery.
Retain the already recorded local 1/5/10/20-task workspace, worktree and Viz measurements and label their synthetic/provider-free boundaries.
The reduced live sample does not establish 10/20-task live scalability or an increase in velocity over a one-task live baseline.
Unavailable human-review, post-merge and opaque model-usage measurements remain unavailable; this approval does not fabricate them.
All four trials have passing worker and integrated results. The independent runtime defects, interrupted execution and post-fix replay remain recorded; the coupled pair used the same corrected package and retains model-command/observer interventions.


## Post-merge audit: 2026-09-28

The [issue log](../../docs/verification/postmerge-audit-issues.md) lists every finding, severity, status and follow-up.

Audit target: `5326a8eb2510590444278225ac3af6fc11026cae` on `main`, including Phase 12/graph merge `bd4a9b9`, installer/source-contract cutover `2f57d21` and terminal repair merge `edb02ac`.
This audit reads plan requirements, current implementation owners, callers and test assertions, rather than treating CI status or phase labels as proof.
No private operational home is used as a validation fixture and no new paid model workload is run.
The original Plans 01-18 (plus 6.5) and Rust-port program are historical predecessors; their superseded policy requirements are not additional unfinished lean phases.
The twelve lean phases and A1-A11 remain the current accounting boundary, with the approved Phase 01 scaffold, Phase 04 provider and Phase 12 workload exceptions retained.

### Requirement accounting

| Phase | Current implementation and meaningful verification owners | Disposition |
| --- | --- | --- |
| 01: contract and skills | `AGENTS.md`, operational skills, `lifecycle/brief.rs`; `mx-brief`, `mx-instruction-owners` | Implemented within the explicitly accepted scaffold boundary; later identity and runtime integration is accounted for below. |
| 02: identity and transactions | `lifecycle/subagent_model.rs`, `project_registry.rs`, core filesystem transitions; `phase02_model`, transition and registry tests | Implemented: task/attempt/brief identity, stale-result fencing and recoverable filesystem foundations. |
| 03: worktree lifecycle | `lifecycle/worktree.rs`; `phase03_spawn`, `mx-worktree`, `mx-spawn-worktree-settle` | Implemented: exact allocation path/base, generation fencing, conservative retention and no normal Treehouse runtime dependency; legacy transfer is Phase 09. |
| 04: delegation and coordination | Core wake queue, `supervision.rs`, backend `headroom.rs`; `phase04_coordination`, wake/report/dispatch tests | Implemented: durable handling, repeat-safe intake and bounded admission; broader authenticated provider coverage remains open as described below. |
| 05: scoped coordinators | CLI/domain lifecycle and `lifecycle/parent_channel.rs`; `phase05_domain`, `phase05_parent_channel`, `phase05_transfer`, `mx-coordinator-spawn` | Implemented: explicit domain identity, private owners, durable relay, shared capacity, transfer and retained children. |
| 06: delivery and human merges | `review.rs`, `review_delivery.rs`, `lifecycle/delivery_evidence.rs`; publication retry, exact revision and merge guard tests | Implemented: ordinary publication, typed evidence and supported human-merge backstops; arbitrary credentialed code is not claimed to be sandboxed by command hooks. |
| 07: optional review | Lazy review/bootstrap/doctor owners and project-scoped review services; deep-review, vplan, bootstrap and workflow tests | Implemented: missing optional assets do not block ordinary work; explicit runs retain checks and revision binding. |
| 08: workflows | `workflow.rs`, `workflow_runtime.rs`, admission/dependency owners; workflow, decision and dispatch tests | Implemented mechanisms; dependency invalidation defect confirmed below and now fixed with owner-driven regression coverage. |
| 09: migration | `lifecycle/migration.rs`, core filesystem transition owner; `mx-state-migration` and migration unit tests | Implemented: inspect/apply/rollback, exact backups, interruption recovery and legacy resource transfer; private-home migration is an operator action. |
| 10: observation and Viz | Domain/CLI snapshot collectors, service `local_services/viz.rs`, `share/viz`; snapshot, service, graph and browser checks | Implemented: bounded read model, exact artifact links, partial state, task/session separation and responsive views; synthetic 20/500-node views do not establish live model concurrency. |
| 11: global entry | `launcher.rs`, `workspace_tui.rs`, `task.rs`, registry/discovery and `harness_launch.rs`; workspace, launcher/PTY and package tests | Implemented: one-owner connection, immutable per-item bindings, bounded discovery and installed assets; the later Ratatui repair is included. |
| 12: combined acceptance | This 39-scenario ledger, four-trial committed results, package tests and complete repository checks | Implemented and merged under approved scope; the A5 audit regression is fixed locally; unavailable raw live evidence remains explicit. Wider live-provider/scaling claims remain unverified. |

The cross-repository isolation assertion is concrete: `phase11_workspace.rs` accepts A/C, rejects ambiguous B without creating a partial receipt, then accepts B by exact checkout and verifies all three distinct allocations and artifacts.
`phase04_coordination.rs` separately checks concurrent retries, invalid starting revision isolation and rejection of receipts copied to another home.
Migration tests inject before-intent, after-intent, after-write, progress and commit failures; unit tests separately assert repeat-apply and byte-exact rollback.
Graph normalization assertions cover nested coordinators, identical worker IDs in different homes, conflicting ownership, missing parents, cycles and deterministic 500-node layout.
These are substantive local integration and fault assertions, not claims of authenticated model behavior.

### Evidence retention and remaining boundaries

All 23 raw artifact paths named by `phase12-live-results.json` are absent on this machine as of this audit.
They point into the temporary `/private/tmp/mx-phase12-live-matrix` tree.
The committed results retain worker outcomes, timestamps, resource observations, hashes and limitations, but this audit cannot recheck the missing raw files against those hashes or independently replay their original live outcomes.
The prior live results remain historical reported evidence; they are not re-certified by current mocked checks or a green CI run.
A durable raw-artifact archive or a separately authorized fresh live trial is needed to close that reproducibility gap.
Do not silently relabel the historical summaries as newly verified live integration.

The user-approved reduced workload was four five-task trials, not the original full live 1/5/10/20 matrix.
Live 10/20-task throughput, unattended completion, complete provider billing and human-review/post-merge quality are not established.
Claude/Pi/cmux and broader Cursor authenticated candidate verification remain explicit follow-up items; successful executable probes are not provider authentication or model-work evidence.
The recorded hierarchical trials include observer assistance and mixed-runtime recovery, so they do not prove autonomous throughput gains.
Public source installation is available; prebuilt public release publication and any private-home migration remain operator actions.

Current documentation corrections reconcile the source filename, PR #48/#49 merge status, Phase 03/11 stale next-step text, predecessor roadmap labels and published installer wording.
Historical phase implementation narratives retain their original chronology; this dated section supersedes their obsolete pending-work instructions.

### Fresh local validation

Reference machine: macOS arm64; source `5326a8e` with only the accounting-documentation corrections above applied during the check window.
The production source and release executable stayed unchanged throughout validation.

| Exact check | Result |
| --- | --- |
| `cargo build --release --workspace --locked` | Pass before behavior execution. |
| `cargo fmt --all -- --check` | Pass. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Pass. |
| `cargo test --locked --workspace` | 787 passed, zero failed/ignored across 37 result groups. |
| `target/release/mx test-run --check-coverage` | Pass: 134 scripts; 113 accelerated, 11 serial, 10 Herdr. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/mx-deep-all.json` | 134 scripts, zero failures, eight explicit gates, 426.113 seconds with four workers. |
| `MX_LAUNCHER_LIVE_E2E=1 target/release/mx test-run tests/mx-launcher-live-e2e.test.sh --json /private/tmp/mx-deep-launcher-probes.json` | Pass, no gates; installed Codex 0.156.1 and Cursor 2026.09.10-fd3934a version routing from an unrelated caller directory. |
| `target/release/mx doc-audience-check` and `git diff --check` | Pass after the documentation corrections. |

The [unaltered runner artifact](phase12-postmerge-audit-behavior.json) preserves per-script exit codes, timings and gate classifications in the repository rather than relying only on temporary logs.
The eight full-suite gates are cmux smoke, Claude stop, Codex continuity, authenticated Cursor, launcher live opt-in, Pi live entry, Pi types and Pi/Herdr marker delivery.
The separate launcher probe closes only its executable-routing gate; it does not close authenticated provider acceptance.
The suite includes real temporary Git/process/tmux/Herdr integration and mocked provider/forge cases; a green aggregate must not be described as all live integrations passing.
The immediately preceding browser audit on the same runtime passed desktop/phone layouts for 0/1/5/10/20 tasks, exact artifact preview, stable interactions, bounded requests, observation age and failed-refresh warnings; 20 post-data interaction samples had p95 20.896 ms.
No new coverage percentage is claimed by these local commands, and no Linux or paid-provider rerun is implied.


### Confirmed correctness finding: completed dependency survives failure

**Priority: high; reproduced during audit, now fixed and locally verified.**
The [repair record](../../docs/verification/postmerge-audit-issues.md#aud-001aud-002-repair-validation) supersedes the open-finding disposition below and closes AUD-001/AUD-002 with new tests.
A current, valid `failed` report after a typed `done` report leaves the canonical schedule at `completed`.
A newly submitted dependent request can then pass admission and spawn even though its prerequisite has explicitly reported failure.
The existing passing suites do not cover this transition, so their success does not establish that all accepted correctness requirements are satisfied.

The report writer in `crates/multplx-domain/src/supervision.rs` sets completion after typed evidence and only reopens a completed task for `working` (lines 579-624 at the audit revision).
It does not clear completed state for a valid `failed` report.
Backend `headroom.rs:1561-1585` consumes only the saved schedule state when checking prerequisites; reserve and drain paths share that predicate.
Brief revision and recorded evidence-commit changes do invalidate completion through their existing owners, but they do not repair this report transition.
The issue conflicts with A5's requirement to schedule only runnable work and with the dependency gate's current-completion contract.

The isolated reproduction uses actual CLI owners in this order:

1. Spawn a canonical implementation prerequisite into its assigned Git worktree.
2. Record typed delivery evidence for its actual HEAD, then submit a bound `done` report.
3. Submit a current bound `failed` report and inspect the task: report succeeds, but schedule remains `completed`.
4. Submit a new request depending on that prerequisite, then spawn it: admission succeeds.

Git, worktree allocation, durable intake, task-model evidence, report transitions and spawn/admission are real.
A fake tmux executable supplies transport observations, and explicit synthetic capacity inputs avoid host-load interference; no live model was started.
The [reproduction utility](phase12-dependency-audit-repro.sh) preserves the exact fixture and commands.
Independent reruns reproduced `DEPENDENT_SPAWNED_AFTER_FAILED_REPORT` on the unchanged release binary; one run retained its owner-generated records under `/private/tmp/mx-audit-dependency-repro.iQYOKQ`.
This is a newly reproduced runtime defect, distinct from the historical raw-evidence retention gap.
The original documentation/accounting audit did not include a runtime fix; the user subsequently authorized the repair on `fix/dependency-completion-invalidation`.
The subsequent repair implements current failure/blocking/decision transition semantics, invalidate completion through its canonical owner, and add regression assertions that dependent admission remains deferred after invalidation while unrelated work remains runnable.
It should also review whether dependency consumption needs stronger current-evidence checks; artifact mutation alone is not claimed as a separately reproduced defect here.
The specific failure transition and regression gap are now locally verified as fixed; remaining historical evidence and broader live acceptance limits still apply.


### Dependency repair closeout

The fix changes only the canonical report transition/help and adds owner-driven checks to the existing report suite.
All 787 Rust tests and 134 behavior scripts pass (eight explicit integration gates), along with release build, formatting, strict Clippy, inventory and documentation checks.
The original reproduction no longer reaches its stale-completed assertion; fresh regression coverage proves dependents remain queued after failure, old reports are inert and fresh valid completion releases the queue.
See the [issue log](../../docs/verification/postmerge-audit-issues.md#aud-001aud-002-repair-validation) for exact commands, source/binary hashes, mocked boundaries and the retained fixed-runtime runner artifact.
Phase 08/12's specific reopened A5 finding is resolved and locally validated on the repair branch; human merge remains pending.
The 23 missing historical raw artifacts and unrun broader live trials remain separately recorded limits.
