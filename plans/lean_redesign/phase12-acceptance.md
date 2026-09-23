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

Required release acceptance is complete under the user-approved reduced workload.
Keep root `AGENTS_E.md` dormant under the continuing development-checkout restriction; the phase plan explicitly permits delivery of the tested package with source cutover reported pending.
The package publishes its content as `AGENTS.md` without teaching runtime discovery the development filename.
A later deliberate repository cutover must rename the source contract, update current links and the documentation inventory, rerun instruction/launcher/documentation checks, and start operational work only in a fresh session.
A tested local package does not imply public release publication, migration of a private operational home or human merge of the implementation PR.

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
