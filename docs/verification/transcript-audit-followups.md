# Transcript audit follow-up repairs

This repair follows the 2026-10-03 transcript audit against `15646bc6cd498b034742fcaed33fefbc45fb1df1` after PRs #52-#55 merged.
The original transcripts and task-specific audit remain private, untracked local artifacts.
This record contains sanitized findings and reproducible validation, without publishing conversation content.

## Findings and implemented behavior

| Finding | Repair | Regression coverage |
| --- | --- | --- |
| TA-001: canonical metadata rewrites invalidate existing PR records | Publication readers share the canonical metadata suffix contract, including schema/model fields; strict identity, duplicate-field and file checks remain. | Canonical evidence rewrite followed by PR registration/retry; malformed and duplicate metadata rejection. |
| TA-002: publication assumes the repository default base | Explicit publication bases are bound to accepted task revisions, frozen within that revision and retained historically for reconciliation and merge protection. | Default and stacked bases, revised bases, stale commits, external retarget validation, retry convergence and target-branch push refusal. |
| TA-003: generated delivery helpers are relative to the wrong checkout | Briefs emit shell-quoted absolute runtime helper paths. | Execute generated help from an external checkout with spaces and an apostrophe in the runtime path. |
| TA-004: doctor's migration suggestion loses the diagnosed home | Suggestions include the exact quoted diagnosed home. | Execute the emitted read-only inspection from an unrelated cwd and verify home bytes remain unchanged. |
| TA-005: evidence examples and errors obscure check-label limits | Documentation distinguishes short names from full command/result summaries; errors identify the field and byte bound. | UTF-8 boundaries, invalid fields, strict identity and no mutation on rejection. |
| TA-006: successful terminal submission is mistaken for worker startup | All terminal backends submit a short owned script; Running requires an exact launch receipt and the corresponding harness process. Uncertain submitted launches retain their ownership for reconciliation. | Inert process versus shell-only startup, failed executable, exact retry, supported wrapper/path forms and guarded real Herdr experiments. |
| TA-007: advertised helper help is rejected | PR-check and harness commands provide side-effect-free help and reject unknown arguments. | Help without state/forge access and invalid-argument refusal. |

Publication check names remain limited to 200 UTF-8 bytes and summaries to 20,000 bytes.
Existing evidence, revision, file ownership and human-only merge checks remain enforced.
Changing a PR's base on the forge is an explicit ordinary forge operation; local base selection does not silently retarget it.
Publication retries with unchanged facts reuse their evidence identity; changed check, review or limitation facts receive a distinct evidence identity.

## Managed Codex session discovery

`multplx task-session inspect TASK` reads the exact current managed Codex SessionStart receipt using the configured home.
`multplx task-session history TASK` exposes retained identities without claiming that historical sessions are live or resumable.
Registration requires the recorded task, owner, attempt, accepted brief, checkout and exact launched native provider process.
A supplied transcript path must be an owned regular file whose header matches the session UUID and checkout.
Repeated identical hooks preserve an existing verified transcript pointer.
Conflicting identities, inherited environments from another process or checkout, malformed pointers and stale attempts are refused.
An unavailable discovery receipt produces a diagnostic without suppressing the existing native lifecycle observer or changing its result.
No newest-file or cwd-based transcript search is used.
Old launches and other providers can legitimately have no verified receipt; transcript availability requires a validated pointer from the native hook.

## Validation status

The integrated production release was built with `cargo build --release --workspace --locked` (passed, 1m 27s).
`cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, toolbelt `bash -n`, `git diff --check`, `bin/mx-doc-audience-check.sh`, `target/release/mx test-run --check-coverage` and `target/release/mx shadow-diagnostic` passed.
The inventory guard reported all 134 scripts accounted for.

`target/release/mx test-run --all --jobs auto --json /private/tmp/mx-transcript-behavior-final.json` passed: **134 scripts, zero failures, eight declared optional gates, 454,423 ms**.
All ten real-Herdr scripts ran without a gate skip.
Skipped gates were optional cmux, authenticated Claude/Codex/Cursor/Pi launches, live launcher verification, Pi type tooling and the opt-in standing-agent Herdr marker trial.
Focused publication, launch, helper-path, evidence-validation, native lifecycle compatibility and session-identity regressions also passed.
Initial full-run failures exposed stale test doubles and inventory expectations; fixtures now execute inert startup and retain their existing safety assertions.
Independent source review findings were resolved before the integrated run.

Clean full instrumentation passed with **93.10% line coverage** (70,841 of 76,091 lines), with every instrumented test target passing.
The exact command was:

```sh
cargo llvm-cov --locked --workspace --all-targets --no-fail-fast \
  --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime|workspace_tui)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' \
  --fail-under-lines 93
```

The diagnostic log is `/private/tmp/mx-transcript-coverage-final.log`.
The final fixture-only PID-log adjustment was separately verified with the process-liveness and direct-caller dispatch-profile suites before this clean coverage run.
[PR #56](https://github.com/KashyapTan/Multplx/pull/56) records the current hosted check results and any platform-specific fixture follow-up.
The first hosted Linux run identified the presentation fixture's Bash sleeper as a shell, correctly refusing to treat it as a provider; the follow-up uses an inert Python process with the same lifetime and retains all presentation assertions.
An earlier instrumented attempt was invalidated by a concurrently edited shell fixture and incomplete startup fixtures; its diagnostic 92.80% measurement is not acceptance evidence.
The 93% line coverage gate and its existing exclusions remain unchanged.

## Evidence limits

Real Herdr evidence uses an isolated guarded lab and an inert harness process, without provider credentials or model calls.
The original long-command failure was reproduced with a synthetic PATH of approximately 18 KB; the short script launches the inert worker and a repeated stable request does not launch a second worker.
This establishes terminal transport and process startup, not model acceptance, successful authentication or task completion.
Publication and provider-hook checks use isolated deterministic fixtures; no live PR was retargeted by those checks.
Installed runtime, private operational homes and user project state were not upgraded or migrated by this repair work.
