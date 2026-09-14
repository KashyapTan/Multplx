# Phase 01 implementation evidence

Implemented on 2026-09-14 based on planning revision `2d19cb2`; Git history and the Phase 01 pull request identify the delivery revision.
The initial inspection preceded that planning commit and preserved the user's pending roadmap edits.
No operational-home activation or release is claimed.

## Scope clarification

The user explicitly selected: "Validate scaffolding now; defer runtime checks to their owning phases."
Phase 01 therefore verifies prompt content, existing task/report routing, literal handoff preservation and compatible charter consumption.
It does not claim A2 attempt validation, A3 acknowledgement, A9 discovery, A10 allocation or A11 coordinator lifecycle implementation.
Those remain with Phases 02, 04, 11, 03 and 05 respectively.
The user also requested that removal be based on usefulness and excessive narrowness, considering improving models, rather than size alone.
The dispositions below retain domain facts and operational constraints while retiring generic mandatory reasoning and response recipes.
No empirical claim that shorter prompts improve model quality is made.

## Inspected instruction sources

| Surface | Classification and result |
| --- | --- |
| `AGENTS_E.md` | Coordination contract retained: orchestrator coding boundary, scoped/nested delegation, evidence, human merges, selected stages, optional tools, identity, locks, routing and recovery. Generic engineering/response procedures removed. |
| `CLAUDE.md`, `CONTRIBUTING.md` | Repository facts and checkout restrictions retained; contributor conventions replace the runtime coding-policy skill. |
| All 17 internal skills | Assessed individually below; `.claude/skills` remains the existing symlink. Public `skills/stow/SKILL.md` is unchanged. |
| `lifecycle/brief.rs` | Implementation, research and persistent templates retained and shortened; reviewer report variant added. Shared status/constraint text replaces duplicated policy. |
| `lifecycle/home_seed.rs` | Consumer inspection found a parsed charter heading. It accepts one new or legacy project section, rejects ambiguity, and retains project-less preflight and rollback checks. |
| `.cursor/rules/multplx.mdc` | Development instructions respect the dormant file and remove the prompt-level native-delegation prohibition. Sandbox facts remain. |
| `docs/supervision-protocols/*`, session renderer | Harness-specific wait/repair mechanics retained. Cursor's dispatch ban removed; unknown fallback points to checkout restrictions. No startup discovery fallback was added. |
| `workflow_runtime.rs` | Stage wrapper keeps isolated worktree, stage scope/output and validated report binding; replaces its blanket delivery ban with scoped delivery and human-only merging. Stage execution/approval mechanics are unchanged. |
| `deep_review.rs`, `.deep-review.yaml` | The optional tool's specialized headless assess/fix context remains useful within that explicit tool. Document-step references now resolve to contributor conventions. Headless extraction and default-tool coupling remain Phases 07/08. |
| `supervision.rs`, hooks, launch integration | Existing delegation-refusal enforcement and its refusal explanation remain Phase 04 work; prompt changes do not claim these fences are gone. |
| Architecture/configuration/decision/override docs | Active owner pointers repaired; legacy executable checks described as compatibility behavior. Unique inheritance and route mechanics now have a maintainer documentation/code owner. |

## Skill usefulness and breadth assessment

| Original skill | Decision and retained value |
| --- | --- |
| multplx-coding-guidelines | Remove runtime skill; repository conventions, documentation ownership and validation remain in contributor context. Loading a generic procedure before every edit adds little beyond those facts. |
| diagnostic-reasoning | Remove mandatory method. Reports still request evidence, uncertainty and source pointers; models may choose causal diagnosis methods appropriate to the actual problem. |
| ask-user-authority | Remove role/yolo-specific reviewer rank procedure. Accepted scope, missing user choices and revision-bound answers remain in the contract. |
| maintainer-override | Remove universal prompt procedure. Executable legacy command checks and records remain tested and documented for later migration; no safeguards are bypassed. |
| decision-hold-lifecycle | Remove universal report-completion attestation instruction. Durable questions, keyed resolution and the complete legacy decision fault/routing suite remain. |
| harness-adapters | Retain. Verified interrupt/exit/reporting and supervision differences are useful environment facts. Move detailed version evidence to its existing verification owners and drop effort-selection/native-delegation policy. |
| bootstrap-diagnostics | Consolidate into subagent-recovery. Diagnostic-to-owner lookup survives; no mandatory installation interview or whole-system work stoppage is prescribed. |
| stuck-actor-recovery | Consolidate into subagent-recovery. Preserve existing work and execution ownership; remove fixed retry counts and actor-only escalation ladder. |
| daemon-provisioning | Consolidate into persistent-subagents. Keep leases, transactional provisioning, inheritance, parent routes and retirement. Separate persistence from assignment; do not restrict idea scope to the runtime repository. |
| project-management | Retain short reference. Local paths, remote-free projects and multi-repo scope are useful; URL/clone prerequisites and mode/yolo interviews are too narrow. Mark pending registration/discovery grammar honestly. |
| create-workflow | Retain schema/validation/preview guidance. Reuse supplied requirements; no compulsory interview or default review/delivery stage. |
| afk | Retain explicit lifecycle and supervision ownership. Implementation timing/composer details stay with their owners rather than a response script. |
| catchup | Retain bounded canonical reads and freshness. Optional saved artifact and flexible presentation replace fixed four-section output. |
| recap | Retain optional visible-history summary and unanswered questions. Remove fixed response wording; operational input recognition stays with its code owner. |
| stow | Retain optional inspect/update/archive mechanics and private knowledge locations. A memory sweep does not require a new project delivery task or AGENTS.md edit. |
| updatemultplx | Retain guarded fast-forward and instruction refresh discovery; no release activation during this port. |
| multplx-codexapp | Retain optional host-tool integration and honest provider visibility. Replace unsafe raw status appends with validated reporting; do not invent a CLI backend. |

Eleven operational skill directories remain: nine shortened existing skills plus two consolidated references.
The three consolidated originals and five retired policy skills are no longer discovered.
The templates remain distinct where their output contracts differ; common text is shared rather than removing useful assignment context.
The optional Herdr lab block remains because it encodes a concrete session-isolation limitation and guarded helper behavior, not a general coding method.

## Implemented interfaces

`mx brief` and `bin/mx-brief.sh` retain ordinary, `--scout`, `--daemon`, `--no-projects`, `--herdr-lab`, `--mode` and `--yolo` compatibility.
`--review` produces an exact-revision review report assignment.
`--context-file PATH` preserves a supplied UTF-8 handoff verbatim, including original artifact pointers, accepted scope/revision, known identities, checkout and starting commit.
Missing/empty context, duplicate context flags, conflicting assignment flags, extra project arguments and unknown flags refuse before writing a brief.
An existing brief remains byte-identical after a refused overwrite.
Context is author-supplied scaffold input, not a parallel state schema or proof that an attempt is current.
Legacy mode/yolo resolution still matches spawn; only local-only changes the requested output destination and neither value opts into review or permits a PR merge.

The three-repository fixture uses separate explicit paths with the same basename, separate task ids and distinct research/criteria/revision context, generated from an unrelated cwd.
Implementation, research and review handoffs retain their own context without importing another repository's instructions.
Persistent scaffolds keep the parent task/status owner distinct from the child home, preserve correlation tokens and delegate coding.
Idea charters explicitly leave repository selection unbound until implementation.

## Measured reduction

Baseline and final scaffolds used the same release command, isolated home path, task ids and fixture registry, without added task-specific context.
Counts are physical lines and UTF-8 bytes, including final newlines; longer real task context is additional.

| Surface | Before lines / bytes | After lines / bytes |
| --- | --- | --- |
| Dormant contract | 498 / 52,979 | 82 / 6,583 |
| Ordinary implementation | 53 / 4,727 | 35 / 2,359 |
| Direct-PR implementation | 49 / 4,346 | 35 / 2,353 |
| Local-only implementation | 48 / 4,098 | 35 / 2,349 |
| Research report | 38 / 3,266 | 35 / 2,313 |
| Persistent charter | 49 / 4,032 | 38 / 2,544 |
| Reviewer report | No prior variant | 35 / 2,305 |

This is a measured size reduction, not a model-performance benchmark or an instruction-budget service.

## Checks and results

All checks used the development checkout on macOS and isolated fixtures for mutations.
The release build preceded black-box checks and was rebuilt after the final Rust integration changes.

| Exact check | Result |
| --- | --- |
| `cargo build --release --workspace --locked` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed after final Rust changes. |
| `cargo test --locked --workspace` | Passed: 463 tests, zero failures; doc tests passed. |
| `target/release/mx test-run --check-coverage` | Passed: total=127, accelerated=106, serial=11, herdr=10, manifest=127. |
| `target/release/mx brief --help` | Inspected: actual flags, legacy limits and deferred runtime bindings disclosed. |
| `git diff --check` | Passed. |
| `target/release/mx doc-audience-check` | Passed after evidence addition: surfaces=74, local_links=368. |

Required focused checks all passed with zero gate skips:

```sh
target/release/mx test-run tests/mx-brief.test.sh
target/release/mx test-run tests/mx-instruction-owners.test.sh tests/mx-ask-user-authority.test.sh tests/mx-maintainer-translation-contract.test.sh tests/mx-stow-contract.test.sh tests/mx-documentation-audiences.test.sh tests/mx-decision-hold-lifecycle.test.sh
```

Additional affected integration checks:

```sh
target/release/mx test-run tests/mx-bootstrap.test.sh tests/mx-status-snapshot-catchup-forge.test.sh tests/mx-status-snapshot-projection-reconciliation.test.sh tests/mx-status-snapshot-landed-bounds.test.sh tests/mx-workflow-lib.test.sh tests/mx-supervision-instructions.test.sh tests/mx-daemon.test.sh tests/mx-daemon-safety.test.sh --jobs auto
target/release/mx test-run tests/mx-daemon-safety.test.sh
target/release/mx test-run tests/mx-workflow.test.sh tests/mx-brief.test.sh tests/mx-instruction-owners.test.sh tests/mx-supervision-instructions.test.sh --jobs auto
```

The first additional batch passed seven suites and failed daemon-safety's obsolete unquoted-reporter assertion.
After updating that assertion and inspecting the charter consumer, daemon-safety passed its entire 59-case matrix; no cases were dropped.
The final four-suite batch passed with zero skips, including workflow order, immutable snapshots, exact output contracts and restart behavior.
A mistyped `tests/mx-workflow-runtime.test.sh` invocation was rejected with exit 2 before tests ran; the actual `tests/mx-workflow.test.sh` was then run successfully.
The final phase HTML records the last validation after evidence and prompt assertions were updated.


Pre-PR regression follow-up:

```sh
target/release/mx test-run --all --jobs auto
target/release/mx test-run tests/mx-naming.test.sh tests/mx-sessionstart-nudge.test.sh tests/mx-tangle-guard.test.sh tests/mx-test-split-parity.test.sh tests/mx-status-snapshot-catchup-forge.test.sh --jobs auto
```

The initial full run returned `total=127 failed=4 skipped_gate=8`.
It identified stale Cursor startup and brief wording assertions, the catchup assertion inventory change, and naming checks matching approved research provenance plus the ordinary verb “ship”.
The fixes assert the current dormant-checkout and isolation contracts, allow only the specific upstream research phrases in CLAUDE.md and porting.md, and record the catchup contract replacement explicitly without rewriting the historical baseline or dropping any of the 140 mapped cases.
All five follow-up suites passed with zero gate skips.
The final complete rerun of `target/release/mx test-run --all --jobs auto` passed: `total=127 failed=0 skipped_gate=8 duration_ms=287024`.
The eight existing environment gates were cmux smoke, Claude stop/auto-arm live E2E, Codex continuity live E2E, Cursor live E2E, launcher live E2E, Pi primary live E2E, Pi primary types, and daemon-marker Herdr live E2E; no new skips were introduced.
Shell syntax checks for every `bin/*.sh` and `bin/backends/*.sh` passed; the skills symlink and absence of root AGENTS.md were verified.

## Evidence limits and next work

Prompt generation, literal context preservation, real temporary Git work, local filesystem transitions and protocol rendering are directly exercised.
The focused Phase 01 suites use fixture harnesses, forge responses and Treehouse calls; they do not establish live provider or GitHub behavior.
The full pre-PR suite additionally exercised real Herdr 0.7.4 and real Treehouse in isolated named-session labs, with the default-session tripwire intact; model harnesses in those backend tests remain stubs.
No live model session, live forge publication, human merge, Linux run or packaged release activation was performed.
The full 127-script suite was also run for this PR; Phase 12 still owns its rerun against the combined release tree and the remaining supported live matrix.
No runtime startup was invoked against this development checkout or private operational homes, and no excluded firstmate content was read or used.
The disposable test helper alone copies the dormant contract to its fixture's AGENTS.md; runtime startup discovery remains unchanged.

Phase 02 can begin from these scaffolds and must replace author-supplied identity context with canonical attempt/brief/project records and validation.
Phase 03 implements built-in worktree operations and retires legacy provider dependencies.
Phase 04 removes actual delegation fences and implements durable handling; Phase 05 supplies named coordinator spawn, validated parent identity and runtime outcome relays.
Phases 06-09 replace delivery gates, optional-tool defaults, workflow coupling and legacy decision/override machinery without losing retained work.
Phase 11 implements launch-anywhere discovery and local reuse; Phase 12 verifies and deliberately activates the release contract.
The final eight-suite rerun returned `total=8 failed=0 skipped_gate=0`; the exact command is recorded in the phase HTML.
No unresolved Phase 01 requirement remains under the user's clarified scaffold boundary.
