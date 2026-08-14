# Multplx Bash-to-Rust porting guide

## Purpose

This document is the implementation guide for replacing the active Multplx runtime in `bin/` with Rust while preserving its observable behavior, safety boundaries, state formats, and supported integrations.
It is written for an agent performing the port, not as a proposal to redesign Multplx.
The port must improve startup cost, concurrency safety, error handling, and portability without changing product policy or weakening fail-closed behavior.
The dependency-ordered implementation portions and their individual acceptance gates are in the [Rust port roadmap](index.html).

## Scope

Port the active root Multplx implementation and its tests.
Ignore `firstmate/` completely.
Do not read, edit, test, package, compare against, or use anything below `firstmate/` as an implementation oracle.
The active source inventory is `git ls-files 'bin/*'`, and the active behavior-test inventory is `git ls-files 'tests/*.test.sh'`.
At the time this guide was written, those inventories contain 113 tool files and 125 behavior-test scripts.
Treat the commands above, rather than the counts in this paragraph, as authoritative when the repository changes.

## Temporary root contract filename

The root broker contract is intentionally named `AGENTS-PORTING.md` during the Rust port so agent harnesses do not auto-inject Multplx broker behavior while they modify Multplx itself.
Run every portion from a fresh ordinary coding-agent session opened directly in the repository, not from the global `multplx` launcher or a Multplx-dispatched actor, daemon, or workflow.
Do not run Multplx session start, adopt the broker role, arm supervision, or use Multplx operational state to execute the port.
Treat `AGENTS-PORTING.md` as dormant product source to inspect and edit, not as active instructions for the porting agent.
If a session already loaded the former root `AGENTS.md`, replace it with a fresh session because the rename cannot remove instructions already held in context.
For portions 1 through 12 and the implementation work in portion 13, every edit that would normally change the root `AGENTS.md` must change `AGENTS-PORTING.md` instead.
Do not create a root `AGENTS.md`, make hooks or launchers auto-discover `AGENTS-PORTING.md`, or relax the production active-home requirement for an exact root `AGENTS.md`.
The port checkout is intentionally not an operational Multplx broker home while the temporary filename is present.
This rule applies only to the root broker contract; managed projects continue to use their own `AGENTS.md` files normally.

Portion 13 owns the only restoration path, after every prior portion and the complete Rust-default closeout gate have passed.
The restoring agent must perform these steps in order.

1. Prove that all thirteen portion gates, the full Rust-default behavior suite, supported integration checks, documentation and skill updates, packaging checks, and the legacy deletion gate are complete.
2. Rename `AGENTS-PORTING.md` to the exact case-sensitive root filename `AGENTS.md` without leaving both files behind.
3. Change temporary root-contract links, documentation inventory entries, authoring guidance, and contract tests back to `AGENTS.md` while leaving project-level references untouched.
4. Keep production active-home detection and auto-discovery code unchanged so the restored standard filename re-enables them naturally.
5. Run the documentation audience and link checks, instruction-owner and naming tests, complete behavior suite, Rust checks, release-build smoke tests, and supported platform lanes against the restored tree.
6. Start a fresh supported harness session and record evidence that the restored `AGENTS.md` auto-loads and the Rust session-start path runs exactly once.

Do not treat the port as complete if `AGENTS-PORTING.md` remains, if both root filenames coexist, or if restoration required teaching runtime code to accept the temporary filename.

The following files are not Rust rewrite targets unless a later step explicitly says otherwise.

- Keep `.pi/extensions/**/*.ts` in TypeScript because those files implement Pi's extension API.
- Keep `share/viz/` and `share/vplan/` as static browser assets.
- Keep `share/shell/multplx.bash` and `share/shell/multplx.zsh` as minimal interactive-shell adapters.
- Replace business logic in `share/shell/shims/` with links or minimal exec-only adapters, but do not try to express interactive shell startup behavior in Rust.
- Keep workflow definitions, hook configuration, examples, fixtures, images, and prose in their native formats.
- Treat existing `plans/*.html` files as historical records and do not rewrite them as part of this port.

## Non-negotiable rules

1. Make no product, policy, authority, state-schema, output-format, or backend-support change in the same patch as a mechanical port.
2. Port one bounded subsystem at a time and keep the repository runnable after every commit.
3. Preserve public command names, arguments, environment variables, stdout, stderr, exit codes, filesystem paths, file modes, record formats, ordering, and idempotency unless the maintainer approves a separate contract change.
4. Preserve the exact `MX_ROOT_OVERRIDE`, `MX_HOME`, `MX_STATE_OVERRIDE`, `MX_DATA_OVERRIDE`, and task-specific test seams used by current scripts and tests.
5. Preserve macOS and Linux behavior, including stock macOS filesystem, process, permission, and path semantics.
6. Never replace an atomic rename, lock, identity check, exact-SHA binding, private mode, or validation step with a weaker approximation.
7. Never invoke user-controlled text through a shell when `std::process::Command` with separate arguments can preserve the contract.
8. Use an explicit shell only where executing shell syntax is itself the documented contract, such as a workflow command or a maintainer-approved command override.
9. Keep agents free of remote-write credentials and preserve the separately credentialed delivery boundary.
10. Preserve the broker's no-project-write and no-unapproved-merge rules in both code and agent instructions.
11. Do not remove a Bash implementation or Bash behavior test until its Rust replacement has passed the differential and cutover gates in this guide.
12. Load `.agents/skills/multplx-coding-guidelines/SKILL.md` before changing tracked Multplx material.

## Porting unit of work

For each command or library batch, the implementing agent must perform the following sequence.

1. Read every target file completely, including its header, usage text, sourced libraries, traps, signals, temporary-file handling, and environment seams.
2. Read every direct caller, sourced dependency, focused test, referenced documentation page, referenced skill, hook, workflow definition, and CI lane.
3. Write a contract checklist covering inputs, outputs, exit codes, state mutations, permissions, locks, process lifetime, external commands, failure behavior, and platform branches.
4. Add Rust unit, integration, property, and fault-injection tests for behavior that is currently hidden inside sourced shell functions.
5. Make the existing black-box Bash tests capable of selecting the legacy or Rust engine without changing their assertions.
6. Run the focused test set against both engines and compare normalized stdout, stderr, exit status, filesystem tree, file contents, file modes, and surviving processes.
7. Cut the command over to Rust only after the differential result is clean.
8. Run the complete behavior suite, documentation checks, Rust checks, and relevant live backend or harness lanes.
9. Review the complete branch diff against the documentation audience, contract ownership, compatibility, and safety rules.
10. Delete the legacy body only after the rollback period and deletion gate described below.

Each port commit must name the legacy files replaced, Rust modules added, behavior tests exercised, differential evidence produced, and documentation owners reviewed.

## Target Rust architecture

Create one Cargo workspace with a single version and dependency policy.
Commit `Cargo.lock` because Multplx is an application.
Use the stable Rust toolchain and declare a minimum supported Rust version only after CI proves it on both supported operating systems.
Prefer a small number of cohesive crates over one crate per former script.

### `multplx-core`

This crate owns canonical home and path resolution, environment parsing, typed identifiers, record parsing, atomic file replacement, permissions, portable process identity, clocks, hashing, lock primitives, journal writes, command execution helpers, and structured errors.
Model closed vocabularies as enums and validate untrusted path components before joining them to a directory.
Represent durable records as typed structs even when their on-disk representation must remain the existing line-oriented format.
Keep serialization deterministic and byte-compatible during the compatibility period.

### `multplx-backend`

This crate owns the runtime-backend trait and the tmux, Herdr, and cmux implementations.
The trait must expose only the operations already routed through `bin/mx-backend.sh`, including tool and version checks, container and task creation, readiness, capture, composer state, literal send, submit, kill, current path, liveness, native state, live inventory, and event waiting.
Backend implementations must use argument arrays, bounded timeouts, typed responses, and explicit error classification.
Keep backend-specific behavior and verification evidence separate rather than flattening the three backends into a least-common-denominator implementation.

### `multplx-domain`

This crate owns backlog, workflow, decision hold, maintainer override, delivery handoff, PR poll, pending reply, inherited configuration, task metadata, snapshots, and lifecycle state machines.
State transitions must be explicit functions over typed state with tests for accepted, refused, repeated, stale, corrupt, and crash-recovery cases.
Parsing and rendering must remain separate so read-only commands cannot accidentally gain write behavior.

### `multplx-cli`

Build one multicall executable named `mx` with subcommands for the current command surface.
Dispatching by `argv[0]` may preserve old executable names during migration, while explicit `mx <subcommand>` syntax becomes the internal canonical form.
Every former usage header must become generated clap help plus a compatibility test that protects the accepted invocation grammar and exit status.
Keep command handlers thin and route reusable behavior through the library crates.

### `multplx-services`

This crate owns the status-report MCP server, vplan loopback server, and viz loopback server after their existing Node implementations are ported.
Bind only to loopback, preserve port probing and token rules, bound request bodies and paths, use constant-time token comparison where applicable, and preserve identity-bound cleanup.
Keep static browser assets outside the binary unless embedding them demonstrably simplifies installation without changing their paths or cache behavior.

### Test support

Add a Rust test-support crate or module for temporary homes, fake executables, deterministic clocks, process fixtures, permission assertions, fault injection, and filesystem snapshots.
Do not expose test seams in production behavior unless the corresponding environment seam already exists.
Keep real tmux, Herdr, cmux, and harness tests black-box and opt-in or required exactly as their current CI lanes specify.

## Compatibility and packaging strategy

Do not make repository execution depend on `cargo run` because its compilation checks and startup overhead would distort production behavior.
Build the release binary once and have compatibility entry points use `exec` so signals, process identity, stdin, stdout, stderr, and exit status reach Rust unchanged.
During the shadow period, an explicit test-only implementation selector may choose `legacy` or `rust`, but normal operator behavior must remain on the current stable default until the cutover gate passes.
Never silently fall back from Rust to Bash after Rust has begun a state-changing operation.
If startup selection fails before mutation, fail clearly or use an explicitly configured legacy mode.

The final installation model must provide a verified prebuilt binary for supported targets or build locally through an explicit installer path.
Verify release checksums before installation.
Do not commit platform binaries to the repository.
After final cutover, tracked shell files under `bin/` may remain only where a host hook or interactive shell requires a minimal transport adapter.
Such adapters may resolve the installed binary and `exec` it, but may not parse domain records, make policy decisions, or mutate operational state.

## Dependency and robustness policy

Prefer Rust standard-library facilities before adding dependencies.
Use mature crates for CLI parsing, serialization, error context, temporary files, file locking, signals, HTTP, and async I/O only when they replace tested custom mechanics.
Centralize dependency versions in the workspace manifest.
Disable unused default features and document any dependency that executes code, parses shell, serves HTTP, handles cryptography, or crosses a privilege boundary.
Run dependency license and advisory checks in CI after the workspace exists.
Do not claim performance improvement until release-build measurements show it against the recorded shell baseline.

## Exhaustive active source migration map

Every file returned by `git ls-files 'bin/*'` must be assigned to exactly one batch before implementation starts.
The lists below cover the active inventory at the time of writing.
If the inventory changes, update this section before porting the new file.

### Batch 1: primitives and typed contracts

Port these files into `multplx-core` first because later batches depend on their behavior.

- `bin/mx-backend-hometag-lib.sh`
- `bin/mx-check-lib.sh`
- `bin/mx-classify-lib.sh`
- `bin/mx-composer-lib.sh`
- `bin/mx-gate-refuse-lib.sh`
- `bin/mx-journal-lib.sh`
- `bin/mx-lock-lib.sh`
- `bin/mx-marker-lib.sh`
- `bin/mx-primary-scope-lib.sh`
- `bin/mx-probe-lib.sh`
- `bin/mx-session-lock-lib.sh`
- `bin/mx-supervision-lib.sh`
- `bin/mx-supervisor-target-lib.sh`
- `bin/mx-tangle-lib.sh`
- `bin/mx-tmux-lib.sh`
- `bin/mx-transition-lib.sh`
- `bin/mx-wake-lib.sh`

Preserve the current sourced-library side-effect rules during shadow testing.
Replace shell globals with explicit context objects and injected clocks, process probes, and filesystem handles.
Test path validation, PID reuse, stale locks, concurrent writers, interrupted writes, malformed rows, status precedence, and exact rendering before any caller moves to Rust.

### Batch 2: structured local state and policy engines

Port these files into `multplx-domain` after Batch 1 is stable.

- `bin/mx-backlog-lib.sh`
- `bin/mx-backlog.sh`
- `bin/mx-backlog-handoff.sh`
- `bin/mx-config-inherit-lib.sh`
- `bin/mx-config-push.sh`
- `bin/mx-decision-hold.sh`
- `bin/mx-maintainer-override-lib.sh`
- `bin/mx-maintainer-override.sh`
- `bin/mx-operational-input.sh`
- `bin/mx-override-bindings.sh`
- `bin/mx-override-run.sh`
- `bin/mx-project-mode.sh`
- `bin/mx-validation-waive.sh`
- `bin/mx-workflow-lib.sh`
- `bin/mx-workflow.sh`

Preserve the existing text, JSON, Markdown-frontmatter, task-hold, and workflow snapshot formats byte for byte unless a versioned migration is separately approved.
Use lock-then-read-then-validate-then-atomic-write transactions for mutable records.
Make every override and hold transition single-use, identity-bound, auditable, and restart-safe.

### Batch 3: runtime backends and harness dispatch

Port the backend facade and all backend transports together so the trait is proven against every supported backend.

- `bin/mx-backend.sh`
- `bin/backends/tmux.sh`
- `bin/backends/herdr.sh`
- `bin/backends/cmux.sh`
- `bin/backends/herdr-eventwait.py`
- `bin/backends/herdr-workspace-move.py`
- `bin/mx-harness.sh`
- `bin/mx-launch-harness.sh`
- `bin/mx-actor-state.sh`
- `bin/mx-peek.sh`
- `bin/mx-send.sh`
- `bin/mx-headroom.sh`

Port the two Python Herdr helpers into the Herdr module so the final runtime does not need Python for backend transport.
Preserve Herdr protocol validation, session scoping, presentation-journal identity, focus restoration, event normalization, composer parsing, and fallback polling.
Preserve cmux socket-password handling without leaking the password into logs or process arguments beyond the existing verified boundary.
Preserve tmux literal-send and composer acknowledgement semantics.
Empirically re-verify every harness and backend listed as supported before changing its documentation evidence.

### Batch 4: task, daemon, and session lifecycle

Port these lifecycle entry points only after the backend and core state layers are available.

- `bin/mx-brief.sh`
- `bin/mx-daemon-report.sh`
- `bin/mx-home-seed.sh`
- `bin/mx-pending-reply-lib.sh`
- `bin/mx-spawn.sh`
- `bin/mx-supervise-daemon.sh`
- `bin/mx-teardown.sh`
- `bin/mx-system-sync.sh`
- `bin/mx-ff-lib.sh`
- `bin/mx-update.sh`
- `bin/mx-upstream-diff.sh`
- `bin/mx-ensure-agents-md.sh`

Preserve worktree isolation, daemon-home separation, pending-reply correlation, inherited material, non-destructive fast-forward rules, landed-work proof, teardown refusal, and safe deletion target validation.
Use typed validated absolute paths for every recursive or destructive operation.
Use a two-phase prepare-and-commit design for multi-file lifecycle mutations so crash recovery can distinguish incomplete preparation from published state.

### Batch 5: watcher, wake, AFK, and hook transports

Port the watcher only after its classification, lock, backend, and pending-reply dependencies are Rust-native.

- `bin/mx-afk-launch.sh`
- `bin/mx-afk-return.sh`
- `bin/mx-afk-start.sh`
- `bin/mx-arm-command-policy.mjs`
- `bin/mx-arm-pretool-check.sh`
- `bin/mx-cd-command-policy.mjs`
- `bin/mx-cd-pretool-check.sh`
- `bin/mx-claude-stop-autoarm.sh`
- `bin/mx-cursor-hook.sh`
- `bin/mx-guard.sh`
- `bin/mx-lock.sh`
- `bin/mx-push-transition-lib.sh`
- `bin/mx-report`
- `bin/mx-report-mcp.mjs`
- `bin/mx-session-start.sh`
- `bin/mx-sessionstart-nudge.sh`
- `bin/mx-subagent-pretool-check.sh`
- `bin/mx-supervision-instructions.sh`
- `bin/mx-turnend-guard.sh`
- `bin/mx-wake-drain.sh`
- `bin/mx-watch-arm.sh`
- `bin/mx-watch-checkpoint.sh`
- `bin/mx-watch.sh`

Port the two JavaScript command policies to a shared Rust parser without broadening the accepted command grammar.
Port the MCP adapter to `multplx-services` while keeping `mx-report` the sole owner of status vocabulary, binding checks, append format, journal behavior, and watcher nudge semantics.
Preserve signal handling, foreground checkpoint exit `124`, queue-before-detector publication, lock ownership, liveness beacon, wake precedence, wedge escalation, pause resurfacing, and turn-end fail-open or fail-closed behavior for each harness.
Test forced termination at every publication boundary and prove the next process can recover without duplicate or lost actionable wakes.

### Batch 6: bootstrap, health, snapshots, and views

Port the composed session and read-model tools after their lower-level owners are stable.

- `bin/mx-bootstrap.sh`
- `bin/mx-doctor.sh`
- `bin/mx-status-snapshot.sh`
- `bin/mx-system-snapshot.sh`
- `bin/mx-system-view.sh`
- `bin/mx-timeline.sh`

Preserve the session-start ordering of lock, bootstrap, wake drain, supervision instructions, context, system state, and next-step reminder.
Keep doctor read-only by default and restrict repair operations to their current proof-bound cases.
Generate human, JSON, and TOON views from one typed snapshot model without making a projection authoritative for lifecycle decisions.
Keep snapshot output bounded and deterministic.

### Batch 7: review, delivery, and PR security

Port these commands only after atomic records, locks, git command execution, and typed task identifiers are proven.

- `bin/mx-check-register.sh`
- `bin/mx-deep-review-lib.sh`
- `bin/mx-deep-review.sh`
- `bin/mx-deliver-lib.sh`
- `bin/mx-deliver.sh`
- `bin/mx-merge-local.sh`
- `bin/mx-pr-check-migrate.sh`
- `bin/mx-pr-check.sh`
- `bin/mx-pr-lib.sh`
- `bin/mx-pr-merge.sh`
- `bin/mx-pr-poll.sh`
- `bin/mx-promote.sh`
- `bin/mx-review-diff.sh`

Treat every path, URL, SHA, PR head, registration, sidecar, receipt, and check result as untrusted input until validated and identity-bound.
Preserve the non-executing migration rule for legacy check files.
Preserve exact-SHA delivery handoffs, branch ancestry proof, fault quarantine, replacement-poll safety, retirement ordering, and the rule that only the credentialed delivery context may push or create a PR.
Add adversarial parser, symlink, hardlink, rename-race, permission, stale-metadata, and crash-recovery tests before cutover.

### Batch 8: vplan, viz, and local services

Port the local servers and their shell controllers together.

- `bin/mx-viz-server.mjs`
- `bin/mx-viz.sh`
- `bin/mx-vplan-server.mjs`
- `bin/mx-vplan.sh`

Preserve loopback-only binding, twenty-port probing, idle timeouts, run-record modes, PID identity, token binding, canonical path checks, GET-only viz behavior, token-authenticated vplan confirmation, atomic artifact writes, and identity-bound stop behavior.
Keep `share/viz/` and `share/vplan/` unchanged during the server port so server parity can be measured independently of UI changes.

### Batch 9: launcher, installers, lab tools, and test tooling

Port these last because they build, install, launch, validate, or test the rest of the system.

- `bin/mx-herdr-ci-cleanup.sh`
- `bin/mx-herdr-lab.sh`
- `bin/mx-herdr-session-cleanup.sh`
- `bin/mx-install-herdr.sh`
- `bin/mx-install-treehouse.sh`
- `bin/mx-launcher-install.sh`
- `bin/mx-launcher-lib.sh`
- `bin/mx-launcher.sh`
- `bin/mx-test-isolation-proof.sh`
- `bin/mx-test-run.sh`
- `bin/mx-doc-audience-check.sh`

Keep installation detect-then-consent-then-install behavior unchanged.
Keep the Herdr destructive-lab tripwires and cleanup scoping unchanged.
Port the test runner only after it can run the entire Rust-default black-box suite, reproduce its resource conflict graph, generate the same CI partitions, and compare timing artifacts.
Port the documentation checker only after its current behavior has equivalent Rust fixtures and failure messages.

## Non-`bin/` integration surfaces

Review these files in every relevant batch even though most remain in their current language.

- Update `.claude/settings.json`, `.codex/config.toml`, `.codex/hooks.json`, `.cursor/hooks.json`, and `.cursor/rules/multplx.mdc` when command paths change.
- Update `.pi/extensions/**/*.ts` to spawn the installed Rust entry point while preserving Pi-specific UI and lifecycle code.
- Update `.deep-review.yaml` when lint or test entry points change.
- Update `.github/workflows/ci.yml` to build release binaries once per job and run both the Rust test suite and existing black-box lanes.
- Update `share/shell/multplx.bash`, `share/shell/multplx.zsh`, and `share/shell/shims/*` only to locate and exec the installed binary.
- Update `workflows/*.workflow.md` only if their command entry points change, without changing stage semantics.
- Keep `tests/fixtures/` formats compatible until a separately versioned data migration is approved.

## Test migration strategy

The existing `tests/*.test.sh` suite is the compatibility oracle and must remain authoritative for external behavior throughout the port.
Do not translate a Bash test to Rust and delete the original in the same step, because two translations can repeat the same misunderstanding.

### Layer 1: Rust unit and property tests

Add Rust tests for parsers, validators, renderers, state transitions, path safety, atomic writers, locks, command construction, backend response decoding, and protocol framing.
Use table-driven tests for every closed vocabulary and exit-code mapping.
Use property tests for round trips, arbitrary malformed records, task identifiers, path components, transition ordering, and shell-policy tokenization.
Use deterministic clocks and process probes rather than sleeps in unit tests.

### Layer 2: differential command tests

Extend `tests/lib.sh` with one implementation-selection helper instead of editing 125 tests independently.
The helper must run the same invocation against isolated legacy and Rust homes and capture exit code, stdout, stderr, filesystem manifest, file bytes, modes, and live child processes.
Normalize only genuinely unstable values such as temporary roots, ports, PIDs, timestamps, and random tokens.
Never normalize ordering, wording, missing fields, permissions, or extra processes.

Add a differential test for every migrated command before switching its default engine.
Keep golden fixtures for stable help, JSON, TOON, status, snapshot, workflow, journal, and diagnostic output.
Run concurrent differential tests for locks, queues, journals, backlog writes, pending replies, overrides, PR publication, and watcher wake handling.

### Layer 3: existing black-box families

Keep `bin/mx-test-run.sh --check-coverage` green so every `tests/*.test.sh` file remains assigned to the resource manifest and a CI lane.
Run the existing family matching the ported batch against both engines.

| Port batch | Required existing test coverage |
| --- | --- |
| Primitives | `mx-backlog-lib`, `mx-composer-*`, `mx-journal`, `mx-lock-override`, `mx-signal-precedence`, `mx-transition-lib`, `mx-wake-queue`, `mx-watcher-lock`, and the related fault fixtures. |
| Backends and dispatch | `mx-backend*`, `mx-headroom`, `mx-dispatch-queue`, `mx-send-*`, `mx-spawn-*`, `mx-actor-state`, `mx-tmux-submit-busy`, and real Herdr or cmux lanes where applicable. |
| Daemons and lifecycle | `mx-daemon*`, `mx-backlog-handoff`, `mx-pending-reply`, `mx-shared-maintainer-inheritance`, `mx-brief`, `mx-teardown`, `mx-system-sync`, and `mx-update`. |
| Watcher and AFK | `mx-watch-*`, `mx-watcher-lock`, `mx-supervision-*`, `mx-nudge`, `mx-turnend-guard`, `mx-claude-stop-autoarm*`, `mx-afk-*`, `mx-operational-input`, and hook guard tests. |
| Session and health | `mx-bootstrap`, `mx-session-start-*`, `mx-sessionstart-nudge`, `mx-doctor`, `mx-status-snapshot-*`, `mx-system-snapshot-view`, and `mx-timeline`. |
| Review and delivery | `mx-deep-review*`, `mx-pr-check-security-*`, `mx-pr-merge`, `mx-push-service`, `mx-review-diff`, `mx-gate-refuse`, `mx-maintainer-override`, and teardown security cases. |
| Local services | `mx-viz` and `mx-vplan`. |
| Launcher and tooling | `mx-launcher*`, `mx-install-herdr`, `mx-herdr-lab`, `mx-herdr-session-cleanup*`, `mx-test-run`, `mx-test-isolation-proof`, `mx-test-split-parity`, and `mx-documentation-audiences`. |

The final Rust-default gate must run every path returned by `git ls-files 'tests/*.test.sh'`, not only the patterns summarized in the table.
Optional live-harness tests remain optional for ordinary development but are required before claiming a verified adapter still works.
The real Herdr CI family remains required and serial until new isolation evidence proves otherwise.

### Layer 4: Rust-native integration tests

Move purely internal shell-function cases into Rust-native tests only after black-box parity is established.
Keep at least one black-box test for every public command, hook transport, supported backend, supported harness, durable record, and destructive refusal path.
Keep real process tests for signal propagation, child cleanup, PID identity, lock contention, and crash recovery.
Do not replace a real-time smoke test with mocked time when the test protects a process-lifetime or timeout contract.

### Layer 5: performance tests

Record a pre-port release baseline for cold command startup, warm command startup, session-start duration, snapshot generation, watcher idle CPU and memory, queue latency, workflow parse and resume, and full-suite wall time.
Measure Rust release builds on the same machine, filesystem, fixtures, and backend state.
Report medians and tail latency over enough iterations to expose process-start and filesystem variance.
Treat lower latency as an acceptance benefit only after functional and safety parity passes.
Do not trade bounded waits, validation, fsync or rename safety, or retry evidence for a faster benchmark.

## CI progression

### Phase A: build-only

Add `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`, a release build, dependency advisory checks, and Linux and macOS compilation.
Keep every existing CI lane unchanged.

### Phase B: shadow parity

Build the Rust binary once per job.
Run migrated focused tests once against legacy and once against Rust.
Upload differential reports and both timing artifacts.
Do not allow normalization rules to hide output or state differences.

### Phase C: historical Rust-default transition

Make Rust the default only for batches that have clean shadow evidence.
During a batch's transition, keep any explicitly approved test-only legacy selector for the documented differential window.
Fail if the legacy path is selected implicitly.

### Phase D: Rust only

Remove Bash business logic, legacy CI passes, Node and Python runtime dependencies that were ported, and the rollback selector.
Retain black-box behavior tests and minimal host adapters.
Update test resource declarations and timing baselines from measured Rust runs.

## Documentation and agent-instruction migration

Documentation changes are part of each batch, not a cleanup deferred until the end.
Use `docs/documentation-audiences.json` as the inventory owner and run `bin/mx-doc-audience-check.sh` after every prose change until its Rust replacement becomes authoritative.
Keep one full contract owner and turn every other mention into a pointer.

### Runtime agent instructions

Review `AGENTS-PORTING.md` after every cutover for command paths, script ownership statements, exact startup or supervision commands, state writers, and referenced headers.
Do not enlarge `AGENTS-PORTING.md` with Rust implementation detail.
Keep only always-loaded policy and skill triggers there.
Move conditional mechanics into the existing agent-only skill that owns the situation.

Review all `.agents/skills/*/SKILL.md` files because they collectively reference launcher, session, supervision, harness, daemon, backlog, decision, override, delivery, recovery, and update commands.
In particular, review `afk`, `bootstrap-diagnostics`, `catchup`, `create-workflow`, `daemon-provisioning`, `decision-hold-lifecycle`, `harness-adapters`, `maintainer-override`, `multplx-coding-guidelines`, `project-management`, `recap`, `stow`, `stuck-actor-recovery`, and `updatemultplx` when their referenced entry points move.
Review `ask-user-authority`, `diagnostic-reasoning`, and `multplx-codexapp` for assumptions even if they do not currently name a script.
Keep `skills/stow/SKILL.md` standalone and free of a required Multplx runtime.

### Human-facing documentation

Update `README.md` and `docs/getting-started.md` for Rust installation, supported release targets, checksums, and any build fallback.
Update `CONTRIBUTING.md` for Cargo layout, formatting, linting, unit tests, black-box compatibility tests, differential testing, and release builds.
Update `CLAUDE.md` only if contributor workflow statements become inaccurate.
Update `docs/architecture.md` for crate and process boundaries without duplicating data-format owners.
Update `docs/configuration.md` for executable discovery, remaining external tools, environment compatibility, and removal of Node or Python requirements.
Update `docs/scripts.md` into the command-line toolbelt index while preserving command ownership pointers.
Update backend guides and `docs/verification/runtime-backends.md` only after empirical tmux, Herdr, and cmux verification.
Update supervision, guard, session-start, delivery, workflow, doctor, journal, viz, and vplan documents in the same batch that changes their implementation owner.
Update `docs/verification/*` with the date, binary version, exact commands, and exact output supporting a current guarantee.
Update `docs/mx-test-*` after the Rust runner and measured CI topology are accepted.

### Headers and generated help

The current script headers and `--help` output own command mechanics.
Before deleting a script, transfer its complete usage contract into clap definitions and Rust module documentation.
Add snapshot tests for help and failure usage.
Replace prose references to a former script header with the Rust command's `--help` entry point.

## Data compatibility and migration

The initial Rust release must read and write the current operational-home layout without migration.
This includes line-oriented metadata, status logs, journals, backlog Markdown, daemon registry, pending replies, wake queues, workflow snapshots, gate records, PR poll sidecars and registrations, delivery handoffs, override records, dashboard records, and vplan records.
Build fixture tests from current root tests and documented contracts.
Test unknown fields, missing optional fields, trailing newlines, empty files, partial final lines, invalid UTF-8 where the shell tolerated bytes, symlinks, hardlinks, and permission errors.

If a future typed format is desirable, finish the behavior-compatible Rust port first.
Then propose a versioned migration with read-old and write-new staging, backups, crash recovery, downgrade behavior, and a separately approved removal date.
Never combine the first Rust cutover with an operational-state format migration.

## Security and failure checklist

Every state-changing Rust command must be reviewed for the following cases.

- Validate task IDs and record keys before path construction.
- Canonicalize existing ancestors and reject traversal outside the allowed root.
- Use `OpenOptions` without following an unsafe symlink when the current contract requires a private regular file.
- Create private state with the same `0600` or directory mode as the current implementation, independent of a permissive umask where required.
- Write to a same-directory temporary file, flush as required by the contract, set mode, and atomically rename.
- Bind locks to process identity rather than PID alone.
- Recheck identity and file metadata after blocking operations when a time-of-check to time-of-use race matters.
- Bound subprocess time, captured output, network bodies, status tails, directory walks, and retry loops.
- Kill and reap owned children on cancellation while never killing an unverified process.
- Keep secrets out of logs, diagnostics, URLs, command displays, journals, and error chains.
- Preserve the difference between read-only diagnosis, recoverable mutation, destructive action, and credentialed delivery.
- Make corrupt state fail closed with an actionable diagnostic rather than silently substituting defaults.

## Cutover gates

A batch may become Rust-default only when all of the following are true.

1. Its source inventory is fully mapped to Rust modules and no caller still sources a removed shell library.
2. Focused legacy and Rust black-box tests produce equivalent observable results.
3. New Rust unit, property, concurrency, and fault tests cover internal behavior formerly exercised only through shell functions.
4. The full root behavior suite passes in its existing CI partitions.
5. Required live backend or harness verification passes.
6. Linux and macOS release builds pass.
7. `cargo fmt`, clippy, tests, advisory checks, and the documentation audience check pass.
8. Documentation and skills reference the correct current implementation without duplicating contracts.
9. Release performance is no worse for the target paths, or the regression has explicit maintainer approval based on a documented robustness benefit.
10. Rollback before mutation is tested and rollback after published state is explicitly disallowed or recovery-safe.

## Legacy deletion gate

Delete a legacy Bash, JavaScript, or Python implementation only when its Rust path has passed the agreed differential and cutover gates and every external caller has moved.
Before deletion, use `rg` to find direct paths, sourced functions, environment seams, docs links, skill links, hooks, workflow commands, fixtures, and tests.
Keep the old filename only as a minimal exec adapter when external compatibility requires it.
Add a test proving the adapter contains no policy or state mutation.
Remove legacy-only dependencies from bootstrap, getting-started documentation, CI, and installers only after no retained integration surface needs them.

## Final definition of done

The Rust port is complete only when all of the following statements are true.

- `firstmate/` was ignored and remains outside the work and evidence.
- Every active file from `git ls-files 'bin/*'` is implemented in Rust or explicitly retained as a minimal host adapter with a written reason.
- No retained shell adapter contains domain parsing, policy, orchestration, locking, or durable-state mutation.
- All 125 current root behavior tests, plus any tests added during the port, are covered by the runner's exact inventory guard and pass against the Rust-default implementation.
- Rust unit, property, concurrency, fault-injection, and service tests pass.
- tmux, Herdr, cmux, Claude, Codex, Cursor, and Pi are empirically re-verified wherever the current repository claims support.
- The session-start, watcher, wake queue, daemon, workflow, review, delivery, and teardown crash-recovery paths are exercised.
- No agent process has remote-write credentials and only the credentialed delivery context can push or create a PR.
- Existing operational homes work without a state migration.
- Linux and macOS installation, upgrade, rollback-before-mutation, and uninstall paths are tested.
- README, contributor guidance, operator docs, architecture docs, verification records, the restored root `AGENTS.md`, all affected skills, hook configuration, workflows, and CI describe the Rust implementation accurately.
- `AGENTS-PORTING.md` has been restored to the sole exact root filename `AGENTS.md`, all temporary pointers have been reverted, and a fresh supported harness session proves normal auto-loading and one-time Rust session start.
- The documentation audience checker, link checks, Rust checks, full behavior suite, and release-build smoke tests pass.
- Measured release-build performance and resource results are recorded against the pre-port baseline.
- Bash, Node, and Python runtime dependencies removed by the port are no longer required by bootstrap or documentation.

## Agent handoff format

An agent completing a port batch must report the following evidence in its task report or PR description.

- The exact legacy files and Rust modules covered.
- The preserved command, state, permission, lock, process, and failure contracts.
- The focused legacy-versus-Rust differential command and result.
- The Rust tests added and the failure classes they cover.
- The existing black-box tests and CI families run.
- The live backend or harness verification performed, including versions.
- The documentation, skills, hooks, workflows, and CI files reviewed or changed.
- The release performance comparison.
- Any retained adapter and why it cannot yet be removed.
- Any behavior difference, which must be maintainer-approved rather than silently accepted as part of the port.
