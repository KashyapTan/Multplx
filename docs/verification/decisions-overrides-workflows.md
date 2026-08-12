# Decision, override, and workflow verification

This record covers Rust-port Portion 10 on 2026-08-12.

The production selector is `MX_AUTHORITY_IMPLEMENTATION`, defaults to `rust`, and accepts `legacy` only for bounded differential verification before an authority or workflow operation begins.

The Rust release binary was built with `cargo build --workspace --release` on macOS arm64 with Rust 1.97.1 and Cargo 1.97.1.

## Implementation and safety evidence

The five owned public entry points select the Rust command boundary by default for decision holds, maintainer overrides, canonical bindings, exact override execution, and workflows.

The two owned sourced libraries remain source-compatible adapters reached only by the explicitly pinned legacy process for later-Portion callers and rollback.

The Rust `decision_hold` model owns privacy-safe identities, sorted unresolved inventories, exact routed-resolution identity, bounded direct-answer records, and idempotent resolution retry.

The Rust `maintainer_override` model owns the closed twenty-boundary registry, exact schema, request identity, action digest, expiry, primary-only decisions, binding revalidation, atomic single consumption, truthful result recording, and audit parsing.

Override directories are mode `0700`, record files are mode `0600` with one link, writes are same-directory atomic replacements, and transitions use the existing directory-lock identity contract.

The Rust `workflow` model owns the constrained version-one parser, closed stage schema, safe substitutions and outputs, typed contracts, immutable private launch snapshots, definition digests, and ordered run-state validation.

The retained workflow executor composition is process-pinned to `legacy` before snapshot, lock, stage, hold, command, or authority mutation because it still calls deep-review and delivery adapters owned by Portion 11.

The retained decision mutation, canonical-binding, exact-command, and stage-executor bodies preserve their byte-level state formats while the Rust default boundary provides the typed authority records Portion 11 will consume.

An invalid selector returns exit `2` from every public adapter and creates no state directory.

Switching implementation after a published decision, override, or workflow record is unsupported within one process, while a fresh invocation can safely resume the existing byte-compatible state under either implementation.

## Commands and results

`cargo fmt --all -- --check` completed successfully.

`cargo check --workspace` completed successfully.

`cargo build --workspace --release` completed successfully.

`cargo clippy --workspace --all-targets --all-features -- -D warnings` completed successfully.

`cargo test --workspace` passed all 312 Rust unit and integration tests plus the documentation-test targets.

`cargo test -p multplx-domain decision_hold::` passed 2 focused tests.

`cargo test -p multplx-domain maintainer_override::` passed 3 focused tests.

`cargo test -p multplx-domain workflow::` passed 4 focused tests.

`cargo test -p multplx-cli --test authority_runtime` passed 6 integration tests covering native identities, registry and digests, private single-use records, native no-Node validation, compatibility pinning, and pre-mutation selector refusal.

`cargo audit --deny warnings` scanned all 68 locked dependencies against 1,216 RustSec advisories without a finding.

`MX_AUTHORITY_IMPLEMENTATION=rust tests/mx-decision-hold-lifecycle.test.sh` and its `legacy` equivalent each passed 9 cases.

`MX_AUTHORITY_IMPLEMENTATION=rust tests/mx-maintainer-override.test.sh` and its `legacy` equivalent each passed 9 cases.

`MX_AUTHORITY_IMPLEMENTATION=rust tests/mx-workflow-lib.test.sh` and its `legacy` equivalent each passed 6 cases.

`MX_AUTHORITY_IMPLEMENTATION=rust tests/mx-workflow.test.sh` and its `legacy` equivalent each passed 11 cases, including restart resume, per-run locking, actor reconciliation, command truth, skip, and reorder.

`MX_AUTHORITY_IMPLEMENTATION=rust tests/mx-lock-override.test.sh` and its `legacy` equivalent each passed the exact lock-owner decision case.

`bin/mx-test-run.sh --check-coverage` confirmed all 125 behavior scripts are classified.

The complete 125-script local run returned 114 successful entries, one documentation-inventory failure caused only by this new record not yet being tracked, and ten live-Herdr tripwire failures because the machine did not have exactly one running default Herdr session.

Every non-Herdr behavior entry other than that pre-commit inventory condition passed, including all five Portion 10 focused scripts and the ask-user-authority instruction-owner test.

The documentation audience checker passed against a temporary index containing this new tracked record, without changing the working index.

## Release performance

The Portion 01 shell baseline on this macOS arm64 machine recorded workflow parse median and p95 of 56/58 ms and parked workflow resume median and p95 of 338/347 ms.

The Portion 10 release measurement used the shipped `new-feature` definition, warm release binaries, thirty validation samples, twenty parked-resume samples, Perl `Time::HiRes`, and nearest-rank p95.

| Target | Portion 01 shell median | Portion 10 Rust median | Portion 01 shell p95 | Portion 10 Rust p95 |
| --- | ---: | ---: | ---: | ---: |
| Workflow validation | 56 ms | 2.271 ms | 58 ms | 2.774 ms |
| Parked workflow resume | 338 ms | 303.120 ms | 347 ms | 332.574 ms |

The validation command was `target/release/mx authority mx-workflow.sh validate workflows/new-feature.workflow.md`, and the resume measurement used a legacy-created first-stage parked fixture resumed through the Rust-default entry.

Both release medians and p95 values are no worse than the Portion 01 shell baseline.

## Compatibility review

The stable shell filenames remain because skills, hooks, workflow definitions, existing operating homes, and Portion 11 review and delivery code call them directly.

Stock macOS Bash remains covered by an explicit `MX_AUTHORITY_IMPLEMENTATION=legacy` CI lane until Portion 13 removes the rollback bodies.

The command names, arguments, environment overrides, JSON and text schemas, permissions, state locations, lock behavior, stage ordering, and exit meanings are unchanged.

The `ask-user-authority`, `decision-hold-lifecycle`, `maintainer-override`, and `create-workflow` skills were reviewed completely and required no semantic or command change.

Workflow examples and `CLAUDE.md` were reviewed and remain accurate, so neither was changed.

No live backend or harness mutation is owned by Portion 10, and workflow actor reconciliation is covered through the existing lifecycle compatibility-adapter tests.
