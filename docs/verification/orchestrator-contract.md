# Orchestrator contract and instruction distribution

This record covers the post-redesign role and instruction-distribution update requested on 2026-10-01.
The user approved delegating substantive requested deliverables, retaining agent freedom within assignments, making the root contract self-contained, exposing operational skills and unifying the Claude entry point.
The implementation starts from `f749bf9`, after PR #50 merged.
The verified implementation is commit `d205df45c7b05af82a1dfd7655a71f800944135e`, delivered in [PR #51](https://github.com/KashyapTan/Multplx/pull/51).
The subsequent evidence update only records this revision and PR reference.

## Acceptance and repaired gaps

- The main and scoped orchestrators delegate requested research, investigations, planning deliverables, implementation, testing and reviews, including small tasks.
- Narrow intake inspection, discussion, synthesis, coordination plans, briefs and supervision remain coordinator responsibilities.
- Workers execute their recorded assignments; loading the shared contract does not turn them into another root orchestrator.
- The operating contract explains the complete lifecycle and provides concrete delegation examples without mandatory research/review chains or restored generic engineering procedures.
- Focused dispatch, supervision and delivery skills expose operational actions while detailed schemas remain with their documentation and command owners.
- `CLAUDE.md` points to `AGENTS.md`; contributor context and development-checkout restrictions move to `VISION.md`.
- Packages expose matching Claude contract bytes and private homes receive the instruction and skill entry points needed by their harnesses.
- Existing uncertain or locally edited instructions remain retained for reconciliation rather than being overwritten.
- Generated briefs, current design documentation and Cursor guidance use the same role boundary and no longer describe the released contract as dormant or partial.

## Validation

Deterministic package and home fixtures, native model behavior and authenticated provider integration are separate evidence categories.

| Check | Observed result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo build --release --workspace --locked` | Passed. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --locked --workspace` | Passed: 789 tests, zero failures or ignored tests across 37 result groups. |
| `target/release/mx test-run tests/mx-ask-user-authority.test.sh tests/mx-maintainer-translation-contract.test.sh` | Passed: both updated contract checks, without removing the scope, independence, durable-question or evidence requirements. |
| `target/release/mx test-run tests/mx-status-snapshot-projection-reconciliation.test.sh` | Passed unchanged: 10.936 seconds for the complete script; the bounded collection assertion passed. |
| `target/release/mx test-run tests/mx-release-package.test.sh` | Passed: 47.171 seconds, including legacy installation/upgrade, semantic contract mismatch refusal and private-home discovery. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/multplx-contract-behavior-final.json` | Passed: 134 scripts, zero failures, eight existing environment-gated skips in 401.030 seconds. |
| `target/release/mx test-run --check-coverage` | Passed: 134 scripts, 113 accelerated, 11 serial, 10 Herdr; manifest matches the inventory. This is inventory coverage, not a line-coverage measurement. |
| `target/release/mx shadow-diagnostic` | Passed: Rust runtime ready. |
| `target/release/mx doc-audience-check` | Passed: 100 classified surfaces and 560 local links. |
| `for script in bin/*.sh bin/backends/*.sh; do bash -n "$script" || exit; done` | Passed. |
| `git diff --cached --check` and `git diff --check` | Passed. |

Each of `task-dispatch`, `task-supervision` and `task-delivery` passed the skill-creator validator with this command, substituting its directory name for `SKILL`:

```sh
PYTHONPATH=/tmp/multplx-skill-validation-deps python3 "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/quick_validate.py" .agents/skills/SKILL
```

The initial plain Python invocation lacked PyYAML; the successful runs used a temporary dependency directory without changing the repository or global Python environment.
Generated runner JSON and full build/test logs remain local temporary outputs rather than tracked source files.
The first complete behavior run reported 134 scripts, four failures and eight existing environment gates in 408.714 seconds.
Two tests still matched old contract wording, the package privacy check caught a machine-specific validator path in this evidence document, and a snapshot timing assertion exceeded its unchanged bound while Rust tests were also running.
The runner also rejected JSON publication through macOS's symlinked `/tmp` parent after executing the tests; subsequent JSON output uses the real `/private/tmp` directory.
The follow-up preserves the scope and evidence assertions, makes the validator reference portable and reruns the snapshot timing check without relaxing its bound.
The final complete run passed without a concurrent Rust build/test workload and published its JSON successfully.
Its eight existing environment gates were cmux smoke, Claude stop/auto-arm live E2E, Codex continuity live E2E, Cursor live E2E, launcher live E2E, Pi primary live E2E, Pi primary types and persistent-home marker Herdr live E2E.
No gate, timeout or timing assertion was weakened.

## Bounded native model trial

One fresh GPT-6.1 SOL medium orchestrator read the new canonical contract and the three new operational skills, then handled an isolated fixture through native collaboration tools.
It created a GPT-6.1 SOL medium researcher and implementer with explicit accepted assignments and separate result artifacts.
The trial used an ordinary temporary Git fixture and a prepared task worktree, with operational startup and real private-home access forbidden.

The accepted user requests and observed outcomes were:

| User input | Observed action and result |
| --- | --- |
| "Research how retries behave in this project. Tell me the default retry count, total attempts, and whether delays use backoff or jitter. Cite the project files and state what you verified. Do not change the project." | The orchestrator delegated to the researcher. The worker inspected `config.json` and `docs/retry-policy.md`, returned 3 retries / up to 4 attempts / fixed 250 ms / no backoff or jitter with source citations, and made no project changes. |
| "Also fix the typo defualt in README.md. Keep the retry research going." with the prepared isolated worktree and explicit no-commit/no-push constraints | The orchestrator delegated the small change to an implementer. The worker made only the one-word README correction in that worktree and checked its diff; the primary fixture remained clean. |
| "While those run, tell me what is underway and whether you need any input from me. Keep both tasks going." | Native task inspection confirmed both workers were running. The orchestrator answered the question immediately, reported no missing input and kept both assignments active. |

The second deliverable arrived after contract inspection but before the first worker launched; the later status question exercised responsiveness while both workers were actually running.
Neither worker recursively delegated its assignment or assumed the root role.
The orchestrator reconciled the returned sources and README diff, rather than performing either requested deliverable itself.
No implementation or test runner existed in the retry fixture, so its findings are explicitly source inspection, not executed retry behavior.
The coordinator read the final canonical contract; the fixture worker copies had identical role rules but predated the final three startup-guidance sentences.
This trial therefore supports delegation, worker identity, concurrent outcomes and conversational availability, not operational startup or long-running drift guarantees.

Detailed local artifacts are retained under `/tmp/multplx-contract-behavior-trial`: `intake.md`, `retry-brief.md`, `typo-brief.md`, `retry-result.md`, `typo-result.md` and `evidence.md`.
The table above preserves the accepted inputs and substantive results in the repository if those temporary artifacts expire.
This was native model execution, not an authenticated Claude/Cursor/Pi trial or proof of their provider-specific instruction consumption.

## Boundaries

The change does not replace the workflow engine, add a policy framework or grant agents PR merge authority.
Deep-review and vplan remain explicitly selected tools.
Historical phase measurements and provider limitations remain historical evidence rather than being recertified by this update.
Private operational homes and the user's global installation are not test fixtures and are not upgraded as part of this repository change.

## Follow-up: managed standing delegation default (2026-10-01)

The human changed the contract default after the native behavior trial above.
All delegation, including nested delegation, now uses Multplx-managed agents by default; native delegation requires an explicit human request for that scope and is never a fallback after a managed launch failure.
The earlier native trial predates this default and remains historical evidence of role separation and responsiveness, not validation of the new dispatch policy.
Native capability and its existing observation/capability checks remain available; this change adds no tool-authorization framework.

Dispatch selects standing managed workers and coordinators through the existing brief, home-seed and spawn owners.
Low-level omission of `--persistent` remains task-scoped for caller compatibility.
Workers keep their assignment role; standing lifetime does not make them coordinators.
The parent may guide, correct mistakes, request revisions and re-engage a premature done reporter until it validates the full agreed job, resolves gaps and delivers it.
Only then does automatic contact, polling, nudging and work routing end; the agent remains available for explicit user-directed follow-up without automatic reuse by responsibility or retirement.

Inspection found that Git-backed home provisioning binds the home reservation, not a separate canonical implementation project allocation.
The owner-level repair therefore requires persistent implementation spawn to name a referenced `--project` and exact full accepted `--base`, binds that project and acquires its own persistent project worktree before endpoint launch.
The home and project allocation remain separate, and existing exact-HEAD completion validation is unchanged.
Generated instructions route task-model inspection/evidence explicitly to the parent state rather than confusing the private home with the report owner.
Queued admission and recovery preserve role, output, home, accepted base and attempt identity; report workers can reconcile a private-home identity without an implementation project.

### Isolated-home live Viz check

The parent diagnosed the missing worker as a home mismatch: the earlier server at port 4890 read the development checkout, while this managed task belongs to the deliberately isolated development home.
The parent stopped the old read-only service through its owner and served the assigned home at the same URL.
Its supplied live API and Firefox accessibility-tree checks found one managed worker connected to the root with a working session; screen capture failed, so this is not screenshot evidence.
This worker separately read `http://127.0.0.1:4890/api/state` at `2026-10-02T03:10:14.089506Z`: the snapshot reports `/private/tmp/multplx-pr51-managed`, its matching state directory, one task `pr51-managed-default` and its current working managed session.
Root session health remains unobserved; a root node does not establish liveness, and native event coverage is not claimed complete.
[The Viz troubleshooting guide](../viz.md#missing-task-troubleshooting) now explains per-home URLs, matching launch home/state and deliberate development isolation without task copying or cross-home scans.

### Validation boundary

The fixture uses real local Git, home provisioning, CLI task/attempt/brief owners, project allocations and typed evidence; its tmux endpoint transport is mocked.
It verifies persistent implementer launch at the separate exact-base allocation, wrong/stale HEAD and stale-revision refusal, corrective revision completion and retained home/endpoint availability.
Additional fixtures cover queued implementation/report launch with frozen role/output/base, and recovery of an exact never-started standing-worker admission.
No authenticated new provider launch, global install update, private operational-home migration, root-health verification or Herdr lifecycle operation was performed.
The parent will run broad regression after integration; this follow-up uses focused changed-owner checks.
Cmux persistent-home launch and persistent task delivery mode/yolo overrides remain unsupported and fail explicitly rather than changing transport or lifecycle.

| Follow-up check | Observed result |
| --- | --- |
| `cargo build --release --workspace --locked` | Passed before focused shell checks. |
| `cargo test --locked -p multplx-domain lifecycle::` | Passed: 183 lifecycle tests. |
| `cargo test --locked -p multplx-domain lifecycle::brief::tests` | Passed: five brief tests, including all worker roles, standing implementation, parent-state evidence routing and corrective-follow-up instructions. |
| `cargo test --locked -p multplx-backend headroom::tests` | Passed: 28 tests, including exact never-started standing-worker retry with implementation and report role/output preservation. |
| `cargo test --locked -p multplx-cli --lib` | Passed: 118 unit tests. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `target/release/mx test-run tests/mx-daemon-safety.test.sh tests/mx-brief.test.sh tests/mx-subagent-pretool-check.test.sh tests/mx-cursor-adapter.test.sh tests/mx-dispatch-queue.test.sh tests/mx-spawn-worktree-settle.test.sh --json /private/tmp/pr51-focused-final.json` | Passed: six scripts, zero failures or skips, 83.070 seconds. The two added standing fixtures execute before the safety script's `exit 0`. |
| `cargo fmt --all -- --check` | Passed. |
| `target/release/mx doc-audience-check` | Passed: 100 classified surfaces, 571 local links. |
| `git diff --check` | Passed. |

The first focused shell run caught the report-worker queue reconciliation's project-only identity assumption; it passed five scripts and failed the new queued report fixture.
The repair preserves the required implementation project identity while comparing frozen private-home and assignment facts for report workers; the final six-script run above passed.
Initial brief test iterations exposed an obsolete draft assertion and inherited assignment environment; the obsolete assertion was corrected, and final Rust and shell checks ran with assignment-specific `MX_*` variables cleared before fixture setup.
No timeout, skip, assertion or validation boundary was weakened.
Final logs, timing JSON and the live Viz API response are local artifacts under `/private/tmp/pr51-*`; the task result report retains their exact paths alongside the commit and accepted revision.

### Parent integration validation

The parent integrated the managed standing implementation as `6574fa8` and the documentation correction as `2d483c5` on the PR branch.
On `6574fa8`, `cargo build --release --workspace --locked` passed, `cargo test --locked --workspace` passed 790 tests with zero failures, and `target/release/mx test-run --all --jobs auto --json /private/tmp/pr51-integrated-all.json` passed all 134 scripts with zero failures and eight existing environment-gated skips in 409.915 seconds.
The following documentation-only commit changes README, user-guide examples and Viz freshness troubleshooting; `target/release/mx doc-audience-check` passed with 100 surfaces and 572 local links, and formatting and diff checks passed after integration.
The original logs are `/private/tmp/pr51-integrated-build.log`, `/private/tmp/pr51-integrated-rust.log` and `/private/tmp/pr51-integrated-all.log`; timing JSON remains an untracked local artifact.

The original managed model launch failed with Codex CLI 0.156.1 rejecting `gpt-6.1-sol` under ChatGPT authentication.
After the user's CLI update to 0.160.0, the same retained task was relaunched through `mx spawn` at generation 2 with GPT-6.1 SOL medium and completed this implementation, including a parent-requested documentation correction after its first done report.
This is live authenticated managed-worker evidence; it does not by itself prove live standing-worker provisioning, nested delegation, other providers or primary-orchestrator health reporting.
The task retained its original temporary lifecycle identity rather than rewriting existing state.
The user also confirmed the worker became visible in Viz after the dashboard's home was corrected.

## Follow-up: primary observation for MX Viz (2026-10-01)

The earlier isolated-home check above measured a snapshot that had no primary-health projection.
It remains historical evidence; it did not verify the observation added in this follow-up.

Canonical snapshots now publish `primary` with the `mx-primary-observation.v1` schema, exact home/state, evidence status, observation timestamp, provider and a plain explanation.
The workspace launcher adds home/state bindings to its existing connection record rather than creating another session registry.
A live result requires that binding, the recorded process lifetime, the current session lock and a supported harness process to agree, followed by another lifetime check.
This uses the launch owner's process-start identity, which survives exec and rejects PID reuse; a bare PID, remembered provider, child session or root graph node cannot establish health.
The collector runs this read-only owner in one bounded subprocess with a two-second deadline and 16 KiB output cap.
The child forces internal multicall mode even when the installed TUI executable is named `multplx`; a command-boundary fixture covers that routing requirement.
Connection and lock reads use the existing bounded no-follow regular-file reader.
No process scans, daemon, private-home migration, event tracking or dashboard mutation endpoint are added.
The projected evidence omits command lines, lifetime markers, caller paths and terminal routes.

Deterministic owner fixtures cover a valid registration, canonical caller aliases, missing registration, changed lifetime, different home/state, legacy unbound registration, conflicting lock, malformed registration and absent/failed identity evidence.
The separate tmux fixture launches a synthetic local harness through the real owner and verifies process-live evidence without claiming model activity.
Collector fixtures cover timeout, failed start, nonzero exit, invalid output and fresh collection of stale identity evidence.
Graph fixtures cover primary evidence, unrelated healthy child work, home mismatch, unverified live claims and old snapshots.
These fixtures establish branch behavior; they are not authenticated primary-provider observations or live green screenshots.

The result distinguishes `live` process evidence, `stale` contradictory identity/lock evidence, `unavailable` incomplete or failed probes and `unregistered` absent registration.
It does not claim working/idle or model responsiveness, nor infer failed/exited sessions from the boolean process probe: absent processes and failed identity probes remain explicitly unavailable.
Legacy connection records without exact home/state bindings remain unavailable until a new managed primary launch records those bindings.
Codex Desktop conversations outside registered Multplx session management remain unregistered and not observed; arbitrary Desktop threads have no implemented supported primary adapter here.
Cache freshness and agent health remain separate labels.
The parent must integrate this commit into PR #51 and validate the complete job; no merge or publication occurs in this worker.

### Normal Viz entry and home resolution

The added user scope makes `mx viz`, `multplx viz` and the TUI `v` action use the existing global launcher configuration and service-returned URL.
The installer adds the verified `mx` binary to its existing atomic generation beside `multplx`; foreign executable collisions remain refused on install, upgrade and uninstall.
Normal Viz opens the returned loopback URL; `--no-open` and explicit `serve` remain scriptable, and the service itself never opens a browser.
A fully explicit root/home pair bypasses unrelated missing or stale global registration, preserving deliberate development isolation.
When that pair matches the selected installation registration, its pending-generation safety check still applies.
A partial global registration fails visibly rather than silently selecting the caller directory.
One configured home is authoritative; this does not discover whichever arbitrary agent happens to be running or retarget a user's tasks.
The previous isolated development home remains separate from the normal globally registered operational home.

The command/TUI fixture installs into temporary directories, starts two real local read-only Viz services with synthetic snapshot readers, occupies the first selected port and checks each API's home identity.
It launches the actual terminal workspace through a pseudo-terminal, presses `v` and verifies that the browser opener receives the selected home's exact fallback URL.
The browser opener is a capture fixture; this verifies URL routing, not real browser rendering or authenticated provider execution.
The fixture also covers no-open behavior, a foreign `mx` collision, partial registration refusal and explicit alternate-home precedence.
No global installation or private operational-home migration is performed.

Validation of this follow-up (2026-10-02) used a release build before behavior checks:

| Command | Observed result |
| --- | --- |
| `cargo build --release --workspace --locked` | Passed. |
| `cargo test --locked -p multplx-backend harness_launch::tests` | 12 passed. |
| `cargo test --locked -p multplx-cli --lib` | 120 passed. |
| `cargo test --locked -p multplx-domain snapshot::tests` | 2 passed. |
| `cargo test --locked -p multplx-services local_services::viz::tests` | 8 passed. |
| `target/release/mx test-run tests/mx-launcher-connection.test.sh tests/mx-launcher.test.sh tests/mx-viz.test.sh tests/mx-status-snapshot-projection-reconciliation.test.sh tests/mx-system-snapshot-view.test.sh tests/mx-release-package.test.sh --json /private/tmp/pr51-root-health-checks-final.json` | All 6 passed; no gate skips. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |

The connection fixture observes a real local synthetic harness process, not a live provider conversation.
The command/TUI fixture runs installed binaries from an unrelated directory with a captured browser opener, and verifies the selected service's home over its read-only API.
It does not upgrade the user's global installation.
The separate actual parent-home observation remains `unregistered`; no live green screenshot was fabricated.

After the final public-help correction, the release build, launcher script, CLI unit tests, focused domain/Viz tests and clippy were repeated.
Public `mx viz --help` and `multplx viz --help` describe configured-home selection, browser opening and `--no-open`; the raw service help still states that it never opens a browser.
Formatting, graph JavaScript syntax/fixtures, documentation audience/link checks and test-manifest coverage also passed.

### Parent regression correction (2026-10-02)

Parent integration `029aa06` passed 793 Rust tests, but its full 134-script run failed the public Viz help assertion in `tests/mx-viz.test.sh`.
The original results remain at `/private/tmp/pr51-final-all.log` and `/private/tmp/pr51-final-all.json` (one failure and eight existing gated skips).
The final public-help interception had omitted the bounded port range, inactivity default and private run-record path that the existing service-header assertion requires.
The correction restores those contracts alongside configured-home selection and browser-opening guidance, and extends installed `mx` and `multplx` help assertions without weakening the Viz test.
Missing-task troubleshooting now starts with the normal installed command/TUI flow and keeps environment matching specific to deliberate development isolation.

Correction validation rebuilt with `cargo build --release --workspace --locked` before behavior checks.
`target/release/mx test-run tests/mx-viz.test.sh tests/mx-launcher.test.sh --json /private/tmp/pr51-root-health-help-correction.json` passed both scripts with zero failures and zero gate skips in 21.239 seconds.
`cargo test --locked -p multplx-cli --lib` passed 120 tests; clippy with all workspace targets/features and warnings denied, formatting, public/raw help checks, Bash syntax and documentation audience/link checks also passed.
Logs remain at `/private/tmp/pr51-root-health-help-correction-{build,tests,cli,clippy,static}.log`.
The parent must integrate this correction and repeat its final regression; this worker does not claim the earlier full-suite failure is a pass.

### Final parent integration and live standing-worker evidence (2026-10-02)

The parent integrated worker commit `a301cc168d851166f211af8e568e8ace693df47d` as `029aa06`, then the public-help correction `1a01dd13763ccebe5536c2ac46d8dc796a7cfb00` as `5cb61e4`.
The first complete integration run passed 793 Rust tests but failed one of 134 behavior scripts: `tests/mx-viz.test.sh` rejected missing bounded port, idle and run-record information in the new public help.
The worker restored those contracts and added installed-command assertions without weakening the existing check.
That failed run remains recorded in `/private/tmp/pr51-final-all.log` and `/private/tmp/pr51-final-all.json` (408.020 seconds, one failure, eight existing environment gates).

The second managed worker was provisioned through `mx brief`, `mx home-seed` and `mx spawn --persistent --role implementer --output implementation --project ... --base ... --backend tmux --harness codex --model gpt-6.1-sol --effort medium`.
It used a separate project worktree and retained private home, accepted revision 2, and generation 1 of its recorded attempt.
It implemented the requested Viz changes, then accepted a correction after its initial done report and recorded fresh typed completion evidence for `1a01dd1` before its corrected done report.
The canonical owner reports `persistent: true`, schedule `completed` and that exact delivery commit.
This is live authenticated standing-worker provisioning, implementation and correction evidence; it does not establish arbitrary nested delegation or another provider's behavior.
Its home and endpoint remain available for the user, with no automatic reuse or further outreach after parent acceptance.
The worker's optional publication-prepare refresh refused replacement of an older ready SHA; that record was retained, and current typed local-completion evidence was accepted through its independent owner.
The parent owns publication into the existing PR, so no machine-owned publication record was hand-edited and no worker publication was claimed.

The rebuilt local dashboard at `http://127.0.0.1:4890/` serves the isolated managed-task home and both completed assignments.
Its primary observation reports `unregistered` with an explicit explanation that external Desktop conversations are not observed.
At 04:40:05 UTC, the live read-only `/api/meta` reported 200 refresh successes, zero refresh failures, an idle refresh and no error; the last refresh took 309 ms and the maximum was 880 ms.
Its 199 stale serves reflect last-good snapshots served during refresh, not 199 failed sessions.
The user previously confirmed the managed worker appeared after correcting the selected home.
The final browser reload was blocked by the locked Mac; no new rendering screenshot or authenticated primary-model health observation is claimed.
An already-open tab needs a reload to receive the changed frontend labels.
The user's global installation and private operational home remain unchanged; the normal automatic configured-home behavior is delivered by an installed upgrade.

Final checks on integrated source `5cb61e4`:

| Command | Result |
| --- | --- |
| `cargo build --release --workspace --locked` | Passed; 59.66 seconds. |
| `cargo test --locked --workspace` | Passed; 793 tests, zero failures across 37 result groups. |
| `target/release/mx test-run --all --jobs auto --json /private/tmp/pr51-final2-all.json` | Passed; 134 scripts, zero failures, eight existing environment-gated skips, 407.038 seconds. |
| `cargo fmt --all -- --check` | Passed. |
| `target/release/mx doc-audience-check` | Passed; 100 classified surfaces and 574 local links. |
| `node --check share/viz/app.js` | Passed. |
| `node tests/fixtures/viz/agents-graph.test.cjs` | Passed. |
| `bash -n bin/mx-viz.sh tests/mx-launcher.test.sh tests/mx-launcher-connection.test.sh` | Passed. |
| `target/release/mx shadow-diagnostic` | Passed; Rust shadow ready. |
| `target/release/mx test-run --check-coverage` | Passed; 134 scripts covered (113 accelerated, 11 serial, 10 Herdr). |
| `git diff --check` | Passed. |

Worker Clippy validation above applies to the same integrated source; the parent's subsequent edit only records evidence.
Final build, Rust and behavior logs are `/private/tmp/pr51-final2-build.log`, `/private/tmp/pr51-final2-rust.log` and `/private/tmp/pr51-final2-all.log`.
Generated timing JSON and logs remain untracked local artifacts.
The eight environment gates do not constitute live validation of gated providers.
The earlier PR head `3f0e9d6` passed all 11 GitHub CI jobs, including line coverage in 12 minutes 37 seconds; the newly pushed head requires its own CI run and that earlier result is not substituted for it.
The canonical delivery is [PR #51](https://github.com/KashyapTan/Multplx/pull/51); publication is separate from human merge and installation.

## Worker identity and standing-agent vocabulary audit (2026-10-03)

The worker-reporting correction makes generated briefs self-contained about task identity, parent route, report correlation, completion evidence, questions and parent corrections until the full accepted job is validated. Temporary and standing homes already loaded a worker-first assignment before the shared contract; the screenshots do not establish that a worker actually adopted the main orchestrator role. The clarified contract prevents that ambiguity without inventing an approval rank or mandatory review ceremony.

The separate vocabulary work inventories tracked files with:

```sh
git grep -n -i -E 'broker|daemon|maintainer' -- AGENTS.md .agents bin crates docs tests
git ls-files '*broker*' '*daemon*' '*maintainer*'
```

The raw census includes internal identifiers, historical evidence, symlinked instruction copies and fixtures; a lexical hit does not establish an obsolete agent role. Reviewed current producers and retained classes are:

| Surface | Current output and retained boundary |
| --- | --- |
| Shared contract and briefs | Recorded researcher/implementer/reviewer/coordinator responsibility wins over generic root instructions. The accepted `sub-orchestrator` role token remains schema vocabulary, not a rank. Legacy `--daemon` is an explicit persistence input alias. |
| Home seed, spawn, retirement, inheritance and fast-forward | New `.mx-agent-home`, `data/agents.md`, `standing-agent-harness`, `MX_AGENT_CHARTER/SCOPE`; legacy-only layouts stay in place. Current diagnostics use standing agent/parent. Existing metadata `kind=daemon`/`mode=daemon`, private journals, home allocation identities and sourced function names remain compatibility data/ABIs. |
| Bootstrap/update | `AGENT_SYNC`, `AGENT_LIVENESS`, `NUDGE_AGENTS`, `reread-parent`, `nudge-agents`; the exact historical pending update message is read for retained retries, while new nudges describe the parent runtime. |
| Reporting | `agent-report` and `mx-agent-report.sh`; old command/script forwards to the same owner. Parent-carrier emission is owned by the separate reporting correction; old marked bytes decode only. |
| Backend containers | New Herdr primary/agent labels and cmux primary/agent titles; exact legacy labels for the same ID/root remain adoptable. New detached tmux sessions use primary, with the existing exact broker session retained. Unqualified legacy tmux metadata targets and away-service fallback remain compatibility endpoints. |
| Pi presentation | New multplx presentation/status keys; historical custom entries and installed Symbol patch guards decode/reuse only, so transcripts remain intact and reload does not patch twice. |
| Human authority | Current operator override entry and operator-words flag use the same exact single-use store. Historical command/flag, maintainer-overrides store, human preference filenames, maintainer-held values and literal human decision record headings remain compatible. No agent authority is inferred from these names; only humans merge. |
| Snapshot/public summary | Standing-agent summary and all-agents flags are current, old flags remain aliases. Version 1 daemon-prefixed JSON projections and bounds remain wire compatibility, not a schema rewrite. |
| Service/history/generic terms | The away-mode background service is a real daemon, credential broker is a generic intermediary, human preferences identify the human operator, and past plans/verification logs retain provenance. These are not agent ranks. Existing test/script filenames explicitly exercising compatibility remain unchanged. |

An obsolete source comment claiming standing agents never delegate standing children was removed. It described no current enforced rank rule; the current inherited default supports nested delegation. No remaining generic role-approval requirement was found in the reviewed active contract. Safety locks, exact human grants, boundary-specific safety rules and the human-only merge rule stay intact.

Identity parsing now trims only surrounding whitespace: malformed `wor ker` cannot alias `worker`, and different case/punctuation remain different IDs. Seed rollback admits either exact canonical or historical journal target after validating current layout conflicts, including a retained prepared journal whose original legacy files were not created before a crash. Conflicting identity recovery retains the journal and both original files. No private home is migrated or activated by these changes; [layout compatibility](../configuration.md#standing-agent-layout-compatibility) gives the exact duplicate-layout boundary.

Focused evidence: core resolver 3/3; exact legacy seed recovery 1/1; harness alias conflict 1/1; cmux exact historical adoption 1/1; tmux creation arrays 1/1; brief contracts 5/5; canonical/historical shared header 1/1; fast-forward report 1/1; canonical operator grant/receipt owner 1/1. Runtime inventory, startup nudge and cmux shell contracts passed 3/3 in 10.994 seconds using the worktree debug binary through the normal thin entry. Pi static contract passed; package/rendering/native E2E checks skipped because Pi and its package were unavailable. This is deterministic mock CLI evidence, not a new live backend or authenticated model test. Broad release/coverage validation remains separate.
