# Supervision integration verification

Audience: maintainer verification.

This record supports current session-start, turn-end, watcher-continuity, and wedge-alarm guarantees.
Operator behavior and active limits remain in the linked current guides.
Task-specific chronology, temporary paths, run identifiers, and delivery transcripts remain in private reports or PR evidence.

## Rust Portion 08 supervision cutover

The Portion 08 Rust-default verification ran on 2026-08-11 on macOS 26.5.2 arm64 with Rust 1.97.1, Cargo 1.97.1, tmux 3.7b, Herdr 0.7.4, Codex CLI 0.146.1, and Cursor CLI 2026.08.04-aaa8809.
Production supervision has one implementation: the Rust multicall boundary.
The public adapters preserve stdin, stdout, stderr, exit status, and signal identity, and no environment variable can redirect an entry to shell.

The Rust workspace passed these gates:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo build --release --workspace --locked
```

Focused Rust-default behavior passed for watcher-arm policy, persistent-cd policy, delegation policy, task-bound reporting, report MCP framing, native-transition precedence, durable wake recovery, watcher singleton ownership, watcher triage, bounded checkpoints, watcher nudges, supervision events, turn-end blocking, Claude auto-arm, Cursor translation, Pi watcher continuity, AFK transfer and return, and daemon lifecycle.
The wake recovery matrix killed a drain after publication began, restored its abandoned private drain file ahead of newly queued rows, and then drained every record exactly once under the queue lock.
The status path proved append-before-best-effort journal and identity-checked nudge ordering, while the native transition suite retained queue-before-detector-publication ordering.

The deterministic AFK suite and its real tmux injection path passed.
The live Herdr AFK path could not prepare its isolated lab because the maintainer-owned `default` Herdr session was stopped; this run did not mutate or restart that external session.
The required CI Herdr lane provisions its own pinned 0.7.4 server and remains the authoritative real-Herdr cutover gate.

The full local inventory run selected all 125 behavior scripts and reported the ten live-Herdr-family failures caused by that stopped external session plus one launch-shape assertion that still assumed the pre-MCP option order.
After making that assertion order-independent, its complete daemon harness-model matrix passed and the portable full-suite rerun passed all 115 selected scripts with zero failures and nine intentional opt-in skips in 328,047 ms.
The coverage manifest assigned all 125 scripts: 104 accelerated, 11 serial, and 10 real-Herdr gated.

The executable watcher, arm, guard, hook, AFK, and wake paths are Rust-owned and have no runtime fallback.
`bin/mx-watch.sh` retains only the sourced event-wait helper ABI used by `tests/mx-supervision-events.test.sh`, and `bin/mx-afk-start.sh` retains only sourced daemon-lock helpers used by focused tests.
Every other completed supervision filename is a minimal exec adapter.

### Release performance

The task-bound reporter ran 30 warm successful appends per implementation through the public `bin/mx-report` adapter with watcher nudging disabled and output discarded.
Legacy measured a 25.230 ms median and 27.051 ms nearest-rank p95.
Rust measured a 6.908 ms median and 7.431 ms nearest-rank p95, improving the representative transaction median by 72.6 percent and p95 by 72.6 percent without changing append or binding semantics.

A two-second idle foreground checkpoint sampled the owned process after 700 ms.
Legacy used 2,272 KiB RSS at 0.1 percent CPU and Rust used 2,272 KiB RSS at 0.2 percent CPU.
Both returned the required exit 124 and left zero processes carrying the isolated home path after retirement.

The workspace coverage gate passed at 93.01 percent line coverage (22,126 measured lines, 1,546 missed), above the required 93 percent floor.
`cargo audit --deny warnings` scanned 68 dependencies against 1,211 advisories without a finding.

## Native session-start delivery

The cross-harness transport pass ran on 2026-07-17 with Codex 0.144.4, Pi 0.80.10, and the tracked Claude hook wiring.
Rows for harnesses removed during the Multplx port are trimmed from this record.

Codex command shape:

```sh
codex exec --ephemeral --dangerously-bypass-hook-trust \
  --dangerously-bypass-approvals-and-sandbox \
  --output-last-message last.txt \
  'Follow any SessionStart hook context before this prompt.'
```

Observed result: the `SessionStart` hook completed and its stdout reached model context.

The trusted project sandbox configuration was verified on 2026-07-31 with Codex 0.146.0.

```sh
codex --ask-for-approval never exec --ephemeral \
  --dangerously-bypass-hook-trust --json \
  'Run exactly one shell command: ps -o comm= -p $$ >/dev/null && sysctl -n hw.logicalcpu >/dev/null. If and only if it succeeds, reply exactly CONFIG_OK. Do not change any files.'
```

The command was intentionally run without `--dangerously-bypass-approvals-and-sandbox`; its shell execution exited 0 and the agent returned `CONFIG_OK`.
This proves the trusted [project configuration](../../.codex/config.toml) grants the host access required by session locking and capacity checks while leaving approval policy outside the tracked repo.

Pi command shape:

```sh
pi -p -e .pi/extensions/mx-primary-turnend-guard.ts \
  --no-context-files --no-session \
  'After obeying any earlier session-start instruction, reply with exactly PI_SMOKE_DONE.'
```

Observed result: `PI_SMOKE_DONE`, with one session-start execution.
The earlier `sendUserMessage` counterfactual raced the positional prompt; the current non-triggering `pi.sendMessage` custom message did not.

Current deterministic and live entry points:

```sh
tests/mx-sessionstart-nudge.test.sh
tests/mx-maintainer-translation-contract.test.sh
MX_PI_LIVE_E2E=1 tests/mx-pi-primary-live-e2e.test.sh
```

The Recap first-message boundary was reverified on 2026-07-22 with Pi 0.81.1.
Marked current operational input and the two exact legacy compatibility shapes selected Catchup, while genuine near-miss maintainer messages remained real boundaries.
The detailed reconciliation and task chronology stay in the private audit report and PR evidence.

## Turn-end guard

The direct and passive mechanisms were validated across the supported harnesses on 2026-07-08 through 2026-07-12, with Claude's replacement Stop-owned path revalidated on 2026-07-24.

| Harness | Version verified | Mechanism | Observed result |
| --- | --- | --- | --- |
| Claude | 2.1.219 | Cooperative blocking `Stop` guard plus `asyncRewake` auto-arm | A fresh unsupervised session ran session start first, reclaimed a stale dead-owner lock, completed two tokenless rewake cycles with no model arm command or guard continuation, and left a competing live owner unchanged. |
| Codex | 0.142.1 | Blocking `Stop` hook | Hook process root stayed anchored to the trusted checkout and one continuation ran. |
| Pi | 0.80.5 | Passive `agent_settled` callback | Exactly one guard follow-up ran for an unhealthy cycle, with no recursion across tool turns. |

The daemon-home scope and manual-repair wake path were measured with Claude Code 2.1.207 on 2026-07-12, when a native background completion re-invoked the idle model with no human input.
The current Stop-owned main/daemon inclusion and child-worktree exclusion are covered deterministically by `tests/mx-claude-stop-autoarm.test.sh`.

The Claude product live path ran with Claude Code 2.1.219 on 2026-07-24:

```sh
claude --version
MX_CLAUDE_LIVE_E2E=1 tests/mx-claude-stop-autoarm-live-e2e.test.sh
```

Observed output:

```text
2.1.219 (Claude Code)
ok - Claude 2.1.219 (Claude Code) live E2E reclaimed a stale session lock through session start, completed two tokenless Stop-owned rewake cycles, and preserved the competing-live-owner boundary
```

Current entry points:

```sh
tests/mx-turnend-guard.test.sh
tests/mx-supervision-instructions.test.sh
MX_PI_LIVE_E2E=1 tests/mx-pi-primary-live-e2e.test.sh
```

## Watcher continuity

The cross-harness evidence combines the 2026-07-17 live pass with Claude's replacement Stop-owned path revalidated on 2026-07-24, all against isolated project and home state.
No credential material was copied into a fixture.

```text
Claude Code 2.1.219
codex-cli 0.144.4
Pi 0.80.10
```

| Harness | Exact opt-in command | Observed guarantee |
| --- | --- | --- |
| Claude | `MX_CLAUDE_LIVE_E2E=1 tests/mx-claude-stop-autoarm-live-e2e.test.sh` | Session start reclaimed a stale owner before two Stop-owned cycles, and a competing live owner prevented arm, rewake, epoch write, or lock replacement. |
| Codex | `MX_CODEX_LIVE_E2E=1 tests/mx-codex-continuity-live-e2e.test.sh` | The one-second foreground checkpoint returned without switching to the arm wrapper. |
| Pi | `MX_PI_LIVE_E2E=1 tests/mx-pi-primary-live-e2e.test.sh` | One initial tool call led to extension-owned successors and clean child retirement on exit. |

Pi 0.81.1 repeated the continuity and clean-exit lifecycle on 2026-07-23 after the Calm presentation changes.

Deterministic entry points:

```sh
tests/mx-pi-watch-extension.test.sh
tests/mx-watcher-lock.test.sh
tests/mx-subagent-pretool-check.test.sh
tests/mx-claude-stop-autoarm.test.sh
tests/mx-turnend-guard.test.sh
```

## Wedge-alarm channels

The real herdr notification channel was bounded manually on 2026-07-10 on macOS 26.5.2 with Herdr 0.7.3.
Automated suites never execute this real notification command.

Herdr command:

```sh
herdr notification show 'MULTPLX TEST - IGNORE' \
  --body 'MULTPLX TEST - IGNORE (wedge-alarm channel verification)' \
  --sound request
```

Observed output:

```json
{"id":"cli:notification:show","result":{"reason":"shown","shown":true,"type":"notification_show"}}
```

The safe command-channel contract is covered without a notification by `tests/mx-daemon.test.sh`: the summary reaches both `$1` and stdin, every channel is process-group bounded, and a failed channel falls through.

## Rust Portion 02 shadow primitives

This section owns the active Portion 02 shadow-port evidence for the core contracts listed in [`plans/rust_port/02-core-primitives-durability.html`](../../plans/rust_port/02-core-primitives-durability.html).
It does not authorize a production cutover or deletion of a legacy script.

The proof ran on 2026-08-10 on macOS 26.5.2 arm64 at legacy base `dfbb3698ace1b044e9f794b5d81fa666bafae9a8` with `rustc 1.97.1` and `cargo 1.97.1`.
`cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo test --locked --workspace`, and `cargo build --release --workspace --locked` passed.
The workspace result included 60 `multplx-core` unit tests, eight black-box core-primitives compatibility tests, the pre-existing multicall and shell-harness tests, and all doc tests.
`cargo audit --deny warnings` scanned 62 locked dependencies against 1,207 RustSec advisories and reported no vulnerability or warning.
`target/release/mx shadow-diagnostic` printed `multplx rust shadow: ready`, while `target/release/mx --help` exposed no operator or production subcommand.

The compatibility tests compare exit status, stdout, stderr, file bytes, and file modes against the current Bash owners for classification, transitions, routing markers, gate refusal, composer parsing, home tags, status folds, journals, wake queues, supervisor discovery, probes, custom-check trust, primary scope, tangle detection, supervision, session-lock status, and process identity.
Colocated tests additionally cover malformed records, traversal and symlink refusal, exact private metadata, atomic-publication faults before and after rename, concurrent journal writes, concurrent lock acquisition, concurrent wake append, failed-drain restoration, record round-trip matrices, and PID-reuse rejection.
`cargo llvm-cov --locked --workspace --all-features --lcov --output-path /tmp/mx-plan02-final.lcov` exercised every public `multplx-core` function entry and measured 3,594 of 4,025 core source lines, or 89.29 percent, on this macOS host.
No public core function entry remains unexecuted on this host, while the remaining uncovered line-level branches include platform-specific implementations, non-public parsing variants, injected operating-system and filesystem failure arms, and defensive race outcomes.
The Linux `/proc` identity parser has direct Linux-gated tests for valid records, missing records, malformed stat fields, nonnumeric start times, missing command lines, empty command lines, and oversized command lines.
The concurrent lock-acquisition and wake-append tests each passed 20 consecutive focused repetitions after the final race review.

The final directly named legacy behavior selection passed 11 of 11 scripts in 56,088 ms after the coverage additions and malformed-trust parser fix.
The broader backend, daemon, Claude auto-arm, tangle-guard, and turn-end-guard selection passed five of five scripts in 113,487 ms.
The unrestricted local `--all` run was not a valid portable gate because an installed Herdr instance failed the lab helper's pre-existing exact-system-state tripwire, causing the ten tests in the real-Herdr family to refuse setup before exercising product behavior.
After that external family was excluded through the runner's owned gate, `bin/mx-test-run.sh --all --exclude-family real-herdr-gated --jobs auto` passed all 115 selected scripts with nine expected optional-tool or opt-in skips in 323,179 ms.

A 200-process comparison of the same `blocked` transition-policy decision took 0.29 seconds through Bash and 0.32 seconds through the release Rust compatibility command on this host.
The comparison is a process-bound shadow check rather than a claim about future in-process service performance, and it shows no material regression at the current compatibility boundary.
All 18 Portion 02 legacy source files remain present.
A bounded caller inventory found 78 current shell, test, skill, and documentation files referencing the transferred helper families, so no deletion or caller cutover is eligible in this portion.

## Claude and Pi event-driven recheck (2026-10-03)

The available environment has Node 24.14.1 and tmux 3.7c, but neither a Claude executable nor a Pi executable or installed Pi coding-agent package.
No fresh authenticated Claude or Pi model run is claimed, and historical versions above remain dated evidence.
The existing credentialed tests were inspected for isolation without launching, installing a provider, copying authentication material or changing a private operational home.
The full Pi live test also performs unrelated Recap and presentation model calls, so it was not used as a minimal continuity probe.

The baseline focused release checks passed both `tests/mx-claude-stop-autoarm.test.sh` and `tests/mx-pi-watch-extension.test.sh`, with no failures or gates.
Claude fixtures establish hook-owned arm and rewake translation, idle silence, single-flight ownership, AFK and live-other-owner boundaries.
Pi fixtures establish an extension-owned successor before `followUp` wake delivery, bounded failure restoration and child retirement.
The retained two-event Pi regression additionally checks a silent idle interval, two distinct event cycles, exactly one initial arm-tool invocation and extension-owned successor creation before each typed wake.
Its Pi API and provider are synthetic; the real tracked TypeScript extension and real isolated child processes run under Node.
The responsiveness assertion concerns the mocked host event loop and does not establish interactive provider responsiveness.

The common supervision predicate now excludes well-formed current completed persistent and coordinator assignments as well as ordinary assignments.
Unread or unfinished wakes, explicit checks, reopened current work and unknown or malformed metadata continue to require supervision.
The observer does not acquire a wake lock or alter claim/inbox state.
The four focused Rust predicate tests pass, including an occupied live wake lock, byte-preserved claimed inbox work, reopened assignments and mismatched task/revision identity.
After the locked release build, the final Claude auto-arm, turn-end guard and Pi watcher selection passed all three scripts with no failures or gates in 101.325 seconds.
[AUD-008](postmerge-audit-issues.md#aud-008-completed-standing-assignments-retain-perpetual-supervision-need) records the corrected completed-standing-assignment projection and exact checks.
Completed assignment evidence does not claim that a parent has validated full delivery; unread completion wakes retain the reconciliation path.
No standing agent is deleted, retired or automatically reused by this projection.

Fresh installed-version, credentialed two-cycle, user-input responsiveness and absence-of-model-loop evidence remains required for current Claude and Pi runtimes when they are available.

## Codex CLI event-driven queue recheck (2026-10-03)

Installed `codex-cli 0.160.0` was exercised in fresh temporary homes using a local synthetic Responses endpoint, without credentials or changes to existing operational homes.
The retained probe is [`tests/fixtures/codex-idle-cli-probe.mjs`](../../tests/fixtures/codex-idle-cli-probe.mjs), invoked with absolute paths to the built `mx` and repository.
It loads the tracked SessionStart registration command, reviews only the fixture's native hook hashes, and starts without a preseeded session lock.
The registration receipt identified the exact returned thread UUID, canonical `CODEX_HOME`, executable and live CLI server process.
The final fixture `/tmp/mx-codex-hook-research.Tu7Gmf/evidence.json` retained five actual Stop events for that UUID, each with a distinct turn ID and `stop_hook_active=false`.
An idle queued message started after 10,027 ms; an ordinary human prompt started after 36 ms; a message queued while busy started after the busy turn completed (event indices 56 and 60).
Another thread received zero turns.
This verifies the installed CLI transport and native hook boundary against a synthetic model, not authenticated provider output or Desktop support.

Four focused Rust unit tests and two instrumented runtime integration tests passed.
The runtime fixture uses the actual `mx` owner with isolated queue and watcher executables: two report wakes reach the same bridge without a second hook invocation, while repeated Stop cannot duplicate delivery.
It also checks no-lock and stale-lock registration before lock acquisition, competing or malformed identity refusal, unsupported queue diagnostics, uncertain receipt recovery, watcher-failure durable Check delivery, exact-thread manual cleanup, and retirement after owner loss.
The installed-layout wrapper uses `MX_RUST_BIN` without a release build tree.
A managed-launch fixture establishes Codex activation and removes inherited activation from Claude and Pi launches.
Canonical wake disposition and acknowledgement remain model responsibilities; queue acceptance alone does not settle a wake.

[OpenAI hook documentation](https://learn.chatgpt.com/docs/hooks) describes synchronous Stop continuation and asynchronous output limits.
The supported idle wake path instead uses the installed CLI's `queue --thread --message`; the tagged [queue service](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/ext/queue/src/service.rs) distinguishes idle dispatch from busy queuing.
Managed CLI activation still requires enabled and natively trusted owned hooks plus exact-thread readiness after canonical session-lock acquisition.
Older CLI versions, disabled or untrusted hooks, direct CLI launches without explicit opt-in, and unverified Desktop sessions retain a visible bounded foreground fallback.


## Final event-driven integration validation (2026-10-03)

The local macOS integration branch builds on open PR #52.
The final production source includes the HTTP framing repair at `2df70c7`; test correction `ef75715` and documentation correction `aaa37d5` complete the tested revision.
The locked workspace release build passed in 1 minute 8 seconds after the HTTP repair.

`target/release/mx test-run --all --jobs auto --json /private/tmp/mx-event-idle-final-all.json` passed all 134 scripts with zero failures and eight explicit gates in 441,817 ms.
The inventory check accounts for all 134 scripts.
The final clean coverage command was:

```sh
cargo llvm-cov --locked --workspace --all-targets --no-fail-fast \
  --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime|workspace_tui)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' \
  --fail-under-lines 93
```

It exited zero with 807 Rust tests across 32 result groups, zero failures or ignored tests, and 68,951 of 74,061 lines covered (93.10 percent).
The existing exclusion expression is unchanged; the new Codex idle module is covered.
The retained local coverage log is `/private/tmp/mx-event-idle-clean-coverage.log`.
An earlier coverage run exposed a stale exact repair-string assertion; `ef75715` corrected the expected inactive-readiness text before this full clean rerun.
Final formatting, strict all-target/all-feature Clippy, documentation audience/local links (101 surfaces, 583 links), shell syntax, Node syntax, inventory and shadow checks passed.

[The audit log](postmerge-audit-issues.md#initial-broad-validation-of-event-driven-repairs) retains initial failures and their dispositions.
[Cursor evidence](cursor-cli.md#event-driven-stop-park-recheck-2026-10-03), the Claude/Pi recheck above and the Codex queue recheck above retain their provider-specific limits.
These results do not claim a fresh Linux run, hosted CI, authenticated Claude/Pi run or Codex Desktop delivery.
The user's existing runtime, operational homes and private configuration were unchanged.

## Native idle readiness recheck (2026-10-09)

The read-only `bin/mx-codex-idle.sh --status` projection distinguishes matched native readiness, readiness for turn end, a bound live bridge with a fresh watcher, retained delivery uncertainty, and explicit degraded checkpoint recovery.
Hook registration or queue capability alone does not prove healthy idle.
The session renderer retains degraded instructions when failure or uncertain delivery remains.
The generic `bin/mx-supervision-instructions.sh --harness pi --status` projection reports supervision need and retained Pi delivery failures; watcher freshness alone does not establish native transport readiness.
Healthy native protocols end the handling turn without recurring model checkpoints or manual re-arm.

The installed `codex-cli 0.162.0` was exercised through the retained synthetic Responses probe in a fresh temporary `CODEX_HOME`.
Its evidence is `/private/tmp/mx-codex-hook-research.9YNWXf/evidence.json`.
The idle queued wake began after 9,731 ms, the human prompt after 33 ms, the busy queued item began after the current turn completed, and the unrelated thread received zero turns.
This is actual CLI hook and queue evidence against a synthetic model, with no authenticated provider or Desktop claim.

Pi `1.1.0` was downloaded and installed only under `/private/tmp/mx-pi-native-idle-inspect/runtime` using the public npm package and isolated dependencies.
Its actual native RPC runtime loaded the changed watcher extension with a local synthetic OpenAI-compatible model and an isolated synthetic watcher executable.
The native `agent_end` callback started the first watcher without a model arm tool call; two quiet intervals produced no extra model requests.
A watcher event established one successor and delivered one native `followUp`, and a later human prompt ran without duplicating that successor.
The retained probe is `/private/tmp/mx-pi-native-idle-inspect/probe.mjs`; raw events and requests are `/private/tmp/mx-pi-native-idle.Kb2Tat/evidence.json`.
This verifies native Pi extension loading, awaited lifecycle callbacks, follow-up delivery and human input through RPC, rather than a full credentialed interactive provider trial or native project-trust UI.
The credentialed live Pi gate remains separate.

Focused deterministic fixtures verify first-cycle readiness timeout retirement and subsequent explicit repair, retained unfinished wakes, delivery-failure retention across watcher readiness, two successor cycles without model re-arm, stale ownership refusal, bounded retries and cleanup.
Codex instrumented runtime fixtures verify exact-session status, unsupported queue capability, uncertainty retention, identity changes and bridge cleanup.
Claude was unavailable on this host; its unchanged Stop mechanism retains the earlier evidence and current protocol contract checks without a new live qualification.
Cursor `2026.10.01-e373342` was available; its unchanged interactive stop park and print-mode limit retain the earlier evidence without a new authenticated interactive trial.
Private operational homes, installed global providers, hook trust settings and user configuration were unchanged.

## Canonical report handoff before maintenance

An accepted current canonical report owns its durable wake and must reach the native watcher consumer before unrelated dispatch or PR maintenance.
The watcher checks that handoff only after acquiring its own lock and reconciling accepted-report publication and parent relay.
Early handoff and the ordinary signal pass share the same delivery owner, including signal markers, pause bookkeeping and publication deduplication.
Routine, acknowledged, historical and legacy observations remain available to their ordinary scan without early marker advancement.

The canonical notification runtime regression uses the real report owner and one-second foreground checkpoint with an isolated dispatch maintenance command that blocks.
The base ordering returned 124 after entering maintenance despite an already durable completion wake.
The corrected ordering returns the existing signal before that maintenance starts, keeps the wake queue byte-for-byte, and preserves the original question/completion and acknowledged replay assertions.
This is deterministic runtime evidence with injected maintenance; it does not claim a fresh authenticated harness trial.

## Managed Codex custom-home readiness (2026-10-14)

Five authenticated Codex CLI 0.162.1 trials used an installed runtime with explicit isolated operational homes.
All five reported missing native readiness, and one observed worker completion remained pending after its primary ended its turn until a human follow-up.
The retained failed home's state contains no `.codex-idle-hook-ready.json`; the former combined mismatch diagnostic did not distinguish that absence from a wrong thread or lock owner.
The installed primary-scope predicate accepts the installer-configured default home but rejects those explicit custom homes before registration.
This identifies a custom-home registration defect; it does not establish a default-home wake failure rate.
Inherited Desktop thread/session IDs in three primary processes are a separate launch contamination issue; two clean primary processes had the same absent-readiness result.

Managed custom-home scope now requires the installed runtime's owned package/config binding plus a canonical exact-home launcher connection whose recorded process lifetime is in the caller's bounded ancestry.
An environment override alone, another live process, a reused PID, mismatched home/state/provider, a symlinked record, a foreign executable, worker scope, or a gate agent cannot establish that authority.
Native SessionStart registration still supplies the exact thread and full process identity, and queue delivery still requires the live canonical lock owner and matched native receipt.
Primary and managed worker Codex launches remove inherited `CODEX_THREAD_ID` and `CODEX_SESSION_ID`; native Codex supplies the new session's identity.
The read-only status projection reports primary-scope eligibility and the particular missing receipt/thread/home/owner proof without changing trust or durable wake state.

The focused Rust regressions cover the package/custom-home authority matrix, launch identity removal, two wake cycles, queued work before task metadata, uncertain transport, explicit retry, and owner-loss cleanup.
Authenticated Codex CLI 0.162.1 under Herdr established native readiness in an explicit custom home against a separate installer default home.
The integrated candidate `ef6cb154555e010c3127800874bd4836b733cb48` includes this Codex change plus the separately owned backend detection and worker-hook changes.
Its native primary and both workers recorded `gpt-6-luna` with high effort.
After the primary ended its initial turn, a canonical worker progress report started a new native primary turn within 3.6 seconds of the report's second-resolution timestamp, with an exact-thread operational queue input and no human follow-up.
Both subsequent implementation completion reports were accepted and acknowledged by that primary without human intervention.
Those completion reports arrived while the primary was handling earlier wakes, so this run proves idle report delivery and completion handling, without measuring idle completion latency.
Raw native transcripts, launcher/lock/readiness receipts, canonical report records, exact binary hash, and the observation timeline are retained outside the repository.
The first integrated run delivered nine distinct queued operational inputs, each once; several arrived after the primary had already handled their canonical events while processing earlier inputs.
This was excess submission into the native FIFO, without evidence of a repeated wake ID.
The bridge now permits one submitted receipt group to remain outstanding per exact thread; new arrivals remain durable and unmarked until the entire previous group is acknowledged.
It then rereads canonical pending work, avoiding stale inputs for events handled together while preserving a later input for work that remains unhandled.
The read-only `awaiting-wake-reconciliation` mode names the unacknowledged delivery condition rather than claiming healthy idle when a receiver has not settled its wake.
Deterministic regressions cover partial group acknowledgement, restart, acknowledgement during transport, independent homes/threads, and uncertain submissions with later unmarked arrivals.
A separate authenticated run explicitly deferred unfinished parent integration, independent acceptance checks, and the requested demo after both workers completed.
The wake message now clarifies that handling includes already-authorized follow-up and delivery when their inputs are available, with delegation governed by the current assignment.
This clarification adds no workflow stage or implementation authority.


The final native candidate `77a449d407f91f8e40249a31b9f7f340030dc14b` contains the receipt-group correction and continuation clarification, plus the separately owned backend and hook changes.
Its separately installed binary has SHA-256 `e9eda9a6d630f3f4eeabcd8ffe85ddaf151e0a2c24da65a6b49eb3ab4b67a0aa`.
In the Markdown link checker trial, the primary ended its initial turn at `2026-10-15T02:16:22.122Z`.
A current researcher completion with an attached result became canonical at second-resolution `02:17:12Z`; the exact bound native primary began its next turn at `02:17:19.073Z`, an upper bound of 7.1 seconds after that report.
No human follow-up or model checkpoint triggered that turn.
The implementation completion later caused another automatic turn, and the primary independently ran acceptance tests and the requested demo before acknowledging it.
That later sample ran while another checkout's broad validation was active, so it is not a controlled latency measurement.
Four distinct native operational inputs were observed, each once; after final handling, the canonical queue and receipt set were empty with no failure or uncertain submission marker.
This sparse live sample establishes completion and settling on the final candidate; burst suppression, partial acknowledgement, and disposition-before-ack races are verified by the deterministic receipt-group regressions rather than a live throughput claim.

The [public upstream comparison](../upstream.md#codex-custom-home-readiness-comparison) records pinned sources and why no implementation was imported.
