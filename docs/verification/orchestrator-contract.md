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
