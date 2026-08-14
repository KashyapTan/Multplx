# Multplx contributor context

Multplx is an active agent-coordination system built around one broker, independent actors, persistent daemons, isolated worktrees, durable supervision, and maintainer-owned authority.
The implementation at the repository root is the product.
The retired `firstmate/` reference is out of scope for current work.
Do not read, edit, test, package, compare against, or use `firstmate/` as an implementation oracle.
Never hardcode a local absolute checkout path in code or documentation.

## Sources of truth

- [`AGENTS.md`](AGENTS.md) is the operating broker contract and conditional-procedure index.
- [`.agents/skills/multplx-coding-guidelines/SKILL.md`](.agents/skills/multplx-coding-guidelines/SKILL.md) owns knowledge placement, contract ownership, documentation discipline, and repository style.
- Current command help, tests, maintained documentation, and agent skills own observable behavior at their narrowest boundary.
- [`docs/documentation-audiences.json`](docs/documentation-audiences.json) owns the classification of maintained prose.
- The completed Rust-port plans under [`plans/rust_port/`](plans/rust_port/index.html) are historical implementation records, not current runtime instructions.

Do not copy a full contract into another file.
Keep one authoritative owner and use concise pointers everywhere else.

## Runtime implementation

The Cargo workspace builds one release `mx` multicall binary that owns production behavior.
Public `bin/` paths remain only where compatibility or a host integration requires a script pathname, and those adapters must terminate at an exec boundary without domain parsing, policy, locking, durable-state mutation, or lifecycle orchestration.
Bash and Zsh activation adapters may perform only shell-native initialization and presentation.
Existing operational homes and their state formats remain compatible.

Use typed identifiers, explicit state machines, bounded subprocesses, argument arrays, atomic same-directory publication, and identity-bound cleanup.
Never weaken a fail-closed check, exact-SHA binding, destructive-target proof, session boundary, or credential boundary for performance.

## Working rules

Read connected code and contract owners before writing.
Load the agent-only `multplx-coding-guidelines` skill before changing shared tracked material.
Keep one complete sentence per Markdown line and use plain hyphen-minus punctuation.
Preserve unrelated user changes in a dirty worktree.
Update command help, documentation, skills, hooks, workflows, examples, verification records, and CI in the same change when their current behavior or owner changes.
Run the documentation audience checker after maintained prose changes.

Changes to supported harness adapters and runtime backends require empirical checks against the real integration when the environment is available.
Do not represent a mock as live evidence.
Agents remain credential-free; authenticated delivery stays in its separately approved non-agent context.

## Validation

Build the optimized runtime before black-box tests:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo build --release --workspace --locked
target/release/mx test-run --check-coverage
target/release/mx test-run --changed
target/release/mx doc-audience-check
```

Use `target/release/mx test-run tests/<subject>.test.sh` for focused behavior work.
Use `target/release/mx test-run --all --jobs auto` for the complete accelerated inventory and `--jobs 1` for the serial reference.
Use `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json <path>` for resource and leak evidence.
Run `cargo audit --deny warnings` when `cargo-audit` is available.

Verification records may contain dates, exact commands, versions, and output.
Record maintained evidence, not task chronology, temporary paths, branches, failed hypotheses, or one-off process identifiers.
