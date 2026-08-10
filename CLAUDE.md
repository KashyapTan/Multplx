# Multplx

Multplx is an active agent-coordination system built around one broker, independent actors, persistent daemons, isolated worktrees, durable supervision, and maintainer-owned authority.
The completed Multplx implementation at the repository root is the product.
The retired `firstmate/` reference is out of scope for current work.
Do not read, edit, test, package, compare against, or use `firstmate/` as an implementation oracle.
Never hardcode a local absolute checkout path in code or documentation.

## Current direction

The current engineering program is an incremental port of the active root runtime from Bash, Node, and Python helpers to Rust.
This is a behavior-preserving implementation migration, not a product redesign.
The port must improve robustness and performance without changing authority, safety, state, backend, harness, or delivery contracts.

[`plans/rust_port/PORTING.md`](plans/rust_port/PORTING.md) is the authoritative cross-cutting Rust port guide.
[`plans/rust_port/index.html`](plans/rust_port/index.html) divides the implementation into thirteen dependency-ordered portions.
Work on one portion at a time and satisfy its definition of done before changing the next portion's default implementation.

## Sources of truth

- [`AGENTS.md`](AGENTS.md) is the active broker contract and must retain that exact case-sensitive filename.
- [`.agents/skills/multplx-coding-guidelines/SKILL.md`](.agents/skills/multplx-coding-guidelines/SKILL.md) owns knowledge placement, contract ownership, compatibility review, documentation discipline, and repository style.
- [`plans/rust_port/PORTING.md`](plans/rust_port/PORTING.md) owns port-wide compatibility, security, testing, packaging, and legacy-deletion rules.
- The selected HTML portion under [`plans/rust_port/`](plans/rust_port/index.html) owns that portion's source slice, dependency boundary, implementation sequence, test plan, and completion gate.
- Current script headers, command help, tests, maintained documentation, and agent skills remain authoritative for observable behavior until their portion cuts over.
- [`docs/documentation-audiences.json`](docs/documentation-audiences.json) owns the classification of maintained prose.

Do not copy a full contract into another file.
Keep one authoritative owner and use concise pointers everywhere else.

## Rust port rules

1. Do not attempt a big-bang rewrite.
2. Keep the repository runnable after every commit.
3. Do not combine a mechanical port with a product, policy, authority, schema, or supported-integration change.
4. Preserve command names, arguments, environment variables, stdout, stderr, exit codes, paths, permissions, record bytes, ordering, idempotency, locks, signals, and process lifetime.
5. Preserve existing operational homes without a state migration in the initial Rust release.
6. Use typed identifiers, explicit state machines, bounded subprocesses, argument arrays, atomic same-directory publication, and identity-bound cleanup.
7. Never weaken a fail-closed check, exact-SHA binding, destructive-target proof, session boundary, or credential boundary for performance.
8. Keep shell only where an interactive shell or host hook genuinely requires a minimal transport adapter.
9. Do not leave domain parsing, policy, locking, durable-state mutation, or orchestration inside a retained shell adapter.
10. Do not delete a legacy implementation until differential parity, Rust tests, black-box tests, supported integration checks, documentation updates, and the portion's rollback window all pass.

## Workflow for one portion

### Establish the boundary

Read the selected HTML portion completely before changing code.
Confirm its owned source list still matches the active repository inventory.
Read every owned file, sourced dependency, direct caller, focused test, maintained documentation owner, relevant skill, hook, workflow, and CI lane.
Do not pull later portions into the current change merely because they share a file name or call path.

### Capture behavior

Write a contract checklist for inputs, outputs, exit status, environment, files, modes, locks, processes, external commands, failure paths, recovery, macOS behavior, and Linux behavior.
Treat the current black-box suite as the compatibility oracle.
Normalize only genuinely unstable values such as temporary roots, PIDs, ports, timestamps, and random tokens.
Never normalize ordering, wording, permissions, missing fields, or leaked processes.

### Implement in Rust

Use the crate boundaries defined in `plans/rust_port/PORTING.md` once the Cargo workspace exists.
Keep CLI handlers thin and move reusable behavior into typed library APIs.
Separate parsing, validation, pure decisions, rendering, and operating-system effects.
Use an explicit shell only where executing shell syntax is itself the documented contract.
Never silently fall back to the legacy engine after a Rust command begins a state-changing operation.

### Prove parity

Add Rust unit, integration, property, concurrency, and fault-injection tests for behavior hidden inside shell functions.
Run the existing focused behavior tests against isolated legacy and Rust homes.
Compare exit status, stdout, stderr, filesystem paths, file bytes, file modes, and surviving processes.
Run the portion's required real backend and harness checks rather than relying only on mocks.
Run the complete behavior inventory before changing the portion's default implementation.

### Update instructions and documentation

Update command headers or generated help, human documentation, `AGENTS.md` pointers, skills, hooks, workflows, examples, verification records, and CI in the same portion that changes their current behavior or implementation owner.
Keep implementation detail out of always-loaded broker policy.
Run `bin/mx-doc-audience-check.sh` after maintained prose changes until its Rust replacement becomes authoritative.

### Hand off evidence

Report the exact legacy files and Rust modules covered.
Report the preserved contracts, differential result, Rust tests, black-box tests, live integration checks, documentation changes, and release performance comparison.
Call out every retained adapter and the concrete reason it cannot yet be removed.
Do not silently accept a behavior difference as part of the port.

## Working style

Read connected code and contract owners before writing.
Plan non-trivial changes against the selected portion's dependency and acceptance gates.
Raise concerns when evidence shows that a planned boundary is unsafe or incomplete.
Prefer focused parallel read-only investigation only when it does not fragment the reasoning needed for one coherent port portion.
