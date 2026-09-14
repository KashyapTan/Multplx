# Multplx contributor context

Multplx is a Rust CLI and coordination runtime for an orchestrator and its sub-agents.
The repository is being redesigned to reduce prescriptive agent policy while preserving durable coordination.
Read [porting.md](porting.md) for the accepted direction and [plans/lean_redesign](plans/lean_redesign/index.html) for the implementation phases.
Those documents describe the target; existing code still implements parts of the older operating model.

## This checkout

The user deliberately renamed the root operating contract to [AGENTS_E.md](AGENTS_E.md) to prevent automatic injection.
Treat it as product source to redesign, not as instructions to adopt the old broker role.
Do not recreate root `AGENTS.md` or run Multplx session start while developing the port.
Do not inspect or use `firstmate/` as a reference.
Preserve unrelated user changes and private operational state.

## Design direction

The orchestrator may work directly and freely coordinate sub-agents within the user's request.
Agents may code, commit, push branches, and create or update PRs; PR merges belong to the human.
Selected workflows retain every declared step while agents choose how to execute the work inside each step.
Deep-review and vplan are explicit opt-in tools.
Keep CLI usage guidance and remove generic coding, diagnosis, review and approval procedures from the runtime skill surface.
Keep status reporting, session identity, locks, the durable wake queue, message routing and recovery.
The porting guide owns the detailed migration boundaries.

## Implementation and verification

The Cargo workspace builds one `mx` multicall binary; domain behavior lives in `crates/`.
Most `bin/` scripts are compatibility or host-integration adapters.
Read the affected implementation and callers before changing behavior, and update the corresponding help and tests.
Keep one sentence per Markdown line and use plain hyphens.
Keep documentation classification in [docs/documentation-audiences.json](docs/documentation-audiences.json).

Use focused existing tests for the changed behavior.
Build the release runtime before black-box checks with `cargo build --release --workspace --locked`.
Run `target/release/mx test-run tests/<subject>.test.sh` for a focused behavior test and `target/release/mx doc-audience-check` for maintained prose.
The porting guide lists full release validation.
Report what was actually tested and distinguish live integration evidence from mocks.
