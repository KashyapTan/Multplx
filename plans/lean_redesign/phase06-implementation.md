# Phase 06 implementation evidence

## Status and source boundary

Status: complete; assigned implementation and acceptance checks passed on 2026-09-16 (UTC).
Work started on 2026-09-15 on `lean-phase06-agent-delivery` from `773c76f`, the merged Phase 05 prerequisite.
The checkout was clean before creating the branch.
The [phase plan](06-agent-delivery-human-merges.html) allocates this work and [porting.md](../../porting.md#accepted-architecture-contract) owns shared requirements.

Inspection confirmed the actual task/attempt/brief/project identities, built-in allocation owner, recoverable transitions and durable parent channels from the prerequisite phases.
It also confirmed that ordinary worker launch still suppressed authentication and publication required an approved legacy handoff.
Root `AGENTS.md` remains absent and `AGENTS_E.md` remains dormant.
No source-checkout session start, private operational-home fixture or excluded `firstmate/` inspection was used.
Tests use temporary repositories, homes and backend namespaces.

## Requirement and verification ledger

| Assigned requirement | Implementation and evidence |
| --- | --- |
| Ordinary agent publication | Worker launch preserves ordinary Git/forge/SSH authentication; v4 prepared records bind the exact revision without approval, waiver or mandatory review. Fake-forge publication and the live Codex trial exercise agent publication. |
| Repeat-safe external actions | Private `PublicationOperation` receipts retain task, attempt, generation, brief, branch, base, commit, fingerprint and PR identity through monotonic intent/push/PR/registration/completion stages. Reconciliation precedes PR creation; completed retries observe without another push/create/edit. Partial failures, uncertain responses and changed identities are covered. |
| Canonical PR monitoring | Direct registration validates the current allocation, registered forge, branch/base/head and immutable starting-revision ancestry, then uses the existing static poll owner. Parser, quarantine, trust, retirement and teardown suites retain their checks. |
| A2/A6 revision evidence | `TaskRecord.delivery` contains bounded exact scope, attempt, commit, actual checks, optional review, limitations and immutable history. Multiline accepted scope is preserved. The review queue orders dependencies and priorities, observes HEAD freshness, and keeps checks, review, readiness and merge facts separate. |
| A11 coordinator outcomes | Publication, failure, evidence changes and verified human merges use frozen, task-scoped durable outcome identities and recoverable local transitions. Direct registration and convenience delivery reuse one published identity. An idle coordinator relays child outcomes to the root; interruption tests cover every local transition boundary. |
| Human-only merges | Owned helpers reject agent/workflow markers; generated Claude/Codex/Cursor/Pi hooks cover ordinary merge, auto-merge, merge queue/API and target-branch push forms. Workflow and override runners retain the same refusal; merge-red is retired. Local worktree merge/rebase remains usable. |
| A9 checkout and repository ownership | Publication requires the current task allocation and task-bound project/common Git/remote identity. Concurrent distinct-repository and remote-free fixtures preserve separate outcomes and dirty user checkout state. Local landing refuses to fast-forward user-owned source checkouts. Existing borrowed-checkout system-sync behavior is retained and tested. |
| A10 resource disposal | Open PRs, dirty/unpushed/unknown work remain retained. Recursive descendant cleanup verifies work disposition before returning the exact allocation; stale generation and borrowed source ownership remain protected. |
| Operator surfaces | Updated delivery/authentication/architecture/getting-started/workflow/record documentation, command help, transport headers and generated briefs. Historical verification documents identify superseded approval/authentication assumptions. |

The publication receipt follows Phase 04's external-action identity and reconciliation contract.
Phase 04's existing launch receipt owns spawn-specific endpoint/backend state, so publication uses a forge-specific receipt instead of overloading that owner.
Canonical evidence remains embedded in the task record, with local writes using the existing recoverable-transition primitive.
There is no new task database, broker or scheduler.

Legacy pending handoffs remain inert after upgrade; historical approved service records and receipts remain readable.
Legacy PR registrations retain their existing retirement path without manufacturing canonical delivery evidence.
Canonical human-merge evidence requires the actual merged `headRefOid` and matching retained publication facts.
A merged older head remains historical and cannot mark a newer unpublished commit merged.

## Repository checks

The required repository checks passed with the explicit timing and platform limits below.
Validation logs and source-hash manifests are retained under `/tmp/mx-phase06-validation.1sDAyv`.
The host is Darwin arm64 with Rust 1.97.1, Codex CLI 0.151.0 and tmux 3.7c.
Linux used the isolated `mx-phase06-validation` container, ordinary user `mxvalidate`, Rust 1.97.1, Node 22.23.2, GitHub CLI 2.46.0, UTF-8 locale and `RUST_TEST_THREADS=2`.
The validation container was removed after its results and source-hash manifests were retained.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on macOS and Linux. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed on macOS and Linux. |
| `cargo test --locked --workspace` | Passed: macOS 695 tests and Linux 697 tests, zero failures and zero ignored. |
| `cargo build --release --workspace --locked` | Passed on macOS and Linux. |
| `target/release/mx test-run --check-coverage` | Passed: 129 fixtures, 108 accelerated, 11 serial, ten Herdr. This is fixture inventory, not line coverage. |
| `target/release/mx test-run --all --jobs auto` | Passed: macOS 129 fixtures, zero failures, eight gated skips, 380,809 ms; Linux 129 fixtures, zero failures, 16 gated skips, 190,619 ms. |
| `target/release/mx doc-audience-check` | Passed after final status/documentation updates: 83 maintained surfaces and 432 local links. |
| Unchanged 93 percent instrumented Rust line-coverage gate | Passed: 93.34 percent line coverage, 63,495 lines, 4,230 missed; 695 instrumented tests passed. |
| Focused publication / local landing | Passed 15 publication scenarios, including completed read-only replay, and the local-landing ownership suite. |
| Focused teardown | Passed 35 scenarios after canonical fake-forge fixture corrections. |
| Optional review / merge helper | `review_runtime` 9/9, deep-review 14/14, merge helper 12/12 and domain delivery 8/8 passed. |
| Evidence / provider integration | Thirteen evidence tests, exact merged-head regression, provider configuration matrix, workflow/override refusal and strict Pi extension types passed. |

After the full macOS shell and instrumented-source snapshots, the final pre-Phase06 canonical-empty-record compatibility fix passed the complete Rust, formatting, lint and release checks, followed by all five affected PR-poll/teardown shell fixtures with zero failures.
Those five fixtures were `mx-pr-check-security-fault-quarantine`, `mx-pr-check-security-parser-entrypoints`, `mx-pr-check-security-publication-migration`, `mx-pr-check-security-retirement-teardown` and `mx-teardown`, each run as `tests/<name>.test.sh` through `target/release/mx test-run`.
The patch touches only the already-excluded process-driven CLI supervisor; it changes neither the coverage denominator nor publication/authentication code exercised live.

The exact unchanged coverage command is:

```sh
cargo llvm-cov --locked --workspace --all-targets \
  --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' \
  --fail-under-lines 93
```

The separate strict Pi check was:

```sh
PATH=/private/tmp/mx-phase04-tools/bin:$PATH \
MX_PI_PACKAGE_DIR=/private/tmp/mx-phase04-tools/lib/node_modules/@earendil-works/pi-coding-agent \
bash tests/mx-pi-primary-types.test.sh
```

It passed strict no-emit checking against Pi 0.85.1.
No coverage exclusions, thresholds or production safety checks were relaxed.

## Live publication and merge-boundary trial

The user selected `/Users/kash/Documents/Xpdite` for the trial.
Its primary checkout began clean on `main` at `8d05cbb7a2d8cc951a142c5e96db12fedc5600f8`, with origin `https://github.com/KashyapTan/Xpdite.git`.
The trial uses a temporary runtime/home and isolated allocation, with documentation-only branch `mx/phase06-live-20260916`.
Real Codex CLI 0.151.0 with GPT 5.6 Sol at high reasoning and tmux 3.7c created commit `4c6958f0b390274490ddfa952bb7997fb40c90a7` and opened [Xpdite PR #8](https://github.com/KashyapTan/Xpdite/pull/8).

The actual generated PreToolUse hook blocked ordinary PR merge, auto-merge, direct push to `main` and the human merge helper before execution.
Fail-closed Git/forge sentinels were installed as a final safety barrier; their mutation logs remained untouched during those refusal trials.
No agent merged a PR or sent a merge mutation to GitHub.
This proves the supported provider hook path, not independent remote enforcement against arbitrary code with broad credentials.

The default headless macOS credential-helper attempt did not complete.
Publication succeeded with a secret-free `gh auth git-credential` helper configured only in the temporary clone and the inherited GitHub keyring authentication.
No separate scheduler token was introduced.
The final live runtime SHA256 is `8c914b22bdbb32813ac0491deb796190f9e959f550789b8b6a2df8edab9f95b5`.
Exact agent replay exited zero with one read-only PR-list reconciliation, zero further push/create/edit calls, and byte-identical canonical metadata, delivered receipt, operation receipt and journal.
Exactly one open PR remains at the recorded head and base, with `autoMergeRequest` null.
The clean task worktree is retained; the primary checkout remains clean on `main` and its `.git/config` hash is unchanged.
The dedicated live tmux server was stopped; runtime, evidence and unlanded work remain retained.
Trial artifacts and binary/source fingerprints are retained at `/private/tmp/mx-phase06-live.k6BL5L/evidence`, including `live-summary.md`.

## Development findings and limitations

Early integration checks caught stale legacy approval-field callers, strict-lint issues and changing-tree test expectations; those were repaired before closeout.
Full checkpoint runs exposed legacy poll readers treating inferred records as canonical, incomplete fake-forge identity fields, and old local-landing expectations.
The fixtures now distinguish legacy compatibility from canonical identity and explicitly require borrowed-source preservation.
A final audit added compatibility for Phase 02–05 canonical tasks whose delivery facts are entirely absent, while partial/current delivery identity still requires matching published evidence.
A changing Linux snapshot retained a stale compiled publisher despite matching source hashes; the final run explicitly invalidates the changed source timestamps and rebuilds the release CLI package.
An optional Pi check rejected the supplied 0.85.1 tool environment because that older fixture requires 0.81.1; final broad checks use the normal environment, while the new extension is separately strict-typechecked against 0.85.1.
The live trial found and fixed multiline accepted-scope rejection and completed-retry handling.
These failed development checks are not reported as passing acceptance evidence.

The full macOS suite passed real Herdr presentation coverage (203,970 ms).
Its eight gated skips were optional cmux smoke, five authenticated provider/launcher fixtures, the Pi typecheck and the Pi/Herdr marker trial.
The separate strict Pi typecheck and dedicated live Codex trial passed as described above.
Linux added eight unavailable-Herdr gates to those skips.
The gated fixtures are reported as skipped, not counted as live provider evidence.

Fake forge, configuration-generation and parser tests are local integration evidence.
The live provider evidence is Codex with tmux and GitHub; it does not establish live Claude/Cursor/Pi behavior, SSH authentication, enterprise forge support or independent remote merge protection.
The prior user-approved deferral of other-provider live trials until after all phases remains recorded in the [Phase 04 evidence](phase04-implementation.md).
Unsupported cmux persistent-home and shell-callable Codex Desktop launch combinations remain unsupported.
The command guard remains an operational backstop and does not claim to contain arbitrary programs holding broadly privileged credentials.

## Remaining work and next phase

No assigned Phase 06 implementation or acceptance work remains unresolved.
Phase 07 is ready to begin; Phase 08 still requires Phase 07.
Other-provider live trials remain deferred under the prior user-approved scope, and Phase 12 retains overall release activation.
The test PR is intentionally left open for the human, and its unlanded work remains retained.
