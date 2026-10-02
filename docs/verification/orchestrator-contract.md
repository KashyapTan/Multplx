# Orchestrator contract and instruction distribution

This record covers the post-redesign role and instruction-distribution update requested on 2026-10-01.
The user approved delegating substantive requested deliverables, retaining agent freedom within assignments, making the root contract self-contained, exposing operational skills and unifying the Claude entry point.
The implementation starts from `f749bf9`, after PR #50 merged.

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
