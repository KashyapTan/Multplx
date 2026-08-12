# Review, delivery, and PR-security verification

This record covers Rust-port Portion 11 on 2026-08-12.

The production selector is `MX_REVIEW_DELIVERY_IMPLEMENTATION`, defaults to `rust`, and accepts `legacy` only before a review, delivery, branch, poll, or remote operation begins.

The Rust release binary was built with `cargo build --workspace --release` on macOS arm64 with Rust 1.97.1 and Cargo 1.97.1.

## Contract checklist

Public arguments, help and error streams, exit meanings, environment overrides, task paths, record bytes, file modes, ordering, idempotency, Git subprocess arguments, and credential stripping remain compatible with the pre-port entrypoints.

Task ids are validated before path construction, and historical operational ids longer than the 64-byte creation cap remain accepted under the path-safe legacy grammar.

Pull request URLs are accepted only when every byte reconstructs `https://github.com/<owner>/<repository>/pull/<positive-number>` under the existing owner and repository grammar.

Pull request heads and approved commits remain exact lowercase 40- or 64-character object ids.

Delivery records, poll sidecars, registrations, retirement receipts, task metadata, and custom-check trust records are inert data with closed schemas.

Private artifacts remain ordinary single-link files on the state device with mode `0600`, while intentional custom checks remain mode `0700` and are bound to their SHA-256 bytes.

Publication remains same-directory and atomic, runnable poll names publish last, and terminal poll retirement removes runnable artifacts before provenance and data.

Legacy check programs are never sourced, evaluated, or executed during migration.

Canonical legacy polls are rebuilt only from independently validated task metadata, while ambiguous or replaced artifacts remain unarmed in the private quarantine.

Deep review preserves trusted default-branch command authority, fixed step order, actor binding, isolated fresh review and fix sessions, restart reconstruction, exact-head invalidation, structured findings, and pending approval handoff.

Delivery revalidates the handoff, metadata, clean worktree, branch, exact SHA, gate or waiver, approval, and origin immediately before credentialed operations.

Agent ambience returns exit `3` before credentials, push, PR creation, or remote merge.

Ambient agent and GitHub credentials are removed from the delivery subprocess environment, and only the explicit delivery token or isolated GitHub configuration is admitted.

Local merge remains clean fast-forward-only, remote merge remains explicit and canonical-repository-bound, promotion preserves the task identity, review diff prefers the fetched current PR head, and a validation waiver remains labeled `waived` for one consumed exact-SHA override.

The stable shell filenames and sourced helper ABIs remain for existing-home compatibility and the explicit rollback window.

Rust owns the command selection boundary, typed identifiers, closed record models, no-follow private reads, custom-check registration, static poll validation, and promotion publication.

The larger deep-review, delivery, merge, review-diff, PR publication, migration, and waiver compositions are process-pinned to the byte-compatible rollback bodies after Rust selects the invocation, matching the incremental compatibility pattern used by prior portions.

Portion 13 owns removal of those retained bodies after the full Rust-default rollback window and legacy deletion gate pass.

## Commands and results

`cargo fmt --all -- --check` completed successfully.

`cargo test -p multplx-domain review_delivery --no-fail-fast` passed 8 focused identifier, parser, link, schema, publication, finding, metadata, and sanitization tests.

`cargo test -p multplx-cli --test review_runtime --no-fail-fast` passed 8 command-boundary tests covering unknown-entry refusal, process-wide legacy pinning, missing-body refusal, every public selector adapter, private custom-check registration, long operational ids, no-follow static sidecar reads, and promotion.

`cargo clippy --workspace --all-targets --all-features -- -D warnings` completed successfully.

`cargo build --workspace --release` completed successfully.

`cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'herdr_(cleanup|presentation|tools)\.rs' --fail-under-lines 93` passed at 93.00 percent line coverage.

`cargo audit --deny warnings` scanned the locked dependency graph without vulnerabilities or warnings.

The focused deep-review config, library, and full suites passed 15 cases.

The PR merge suite passed 12 cases, including exact metadata-before-merge ordering, agent ambience refusal, canonical repository derivation, explicit methods, repository-override refusal, merge failure propagation, and one-use exact red-check authority.

The delivery suite passed 7 cases, including pending refusal, exact-SHA push, idempotence, stale-head archive, credential sanitation, inert record parsing, agent credential absence, and truthful waived delivery.

The review-diff suite passed 5 cases covering fresh pull-head preference, recorded-head fallback, local-branch behavior, and explicit degraded-review warning.

The four PR-security suites passed their complete parser, publication, migration, fault-quarantine, retirement, and teardown groups.

The same ten focused shell suites passed again with `MX_REVIEW_DELIVERY_IMPLEMENTATION=legacy`, proving the explicit rollback selection before mutation.

Observed adversarial results included complete raw-byte URL rejection before side effects, static poll silence except for one exact merged line, no partial artifacts after interrupted preparation, no incomplete publication visible to concurrent watchers, symlink and directory refusal at every private destination, and single-link enforcement across live, marker, diagnostic, custom-check, receipt, and quarantine files.

Observed recovery results included watcher exclusion before migration scan, non-executing legacy quarantine, idempotent canonical rebuild, post-rename revocation and retry, durable failure obligations, validated replacement acceptance only with full provenance, runnable-first retirement, queue failure preservation, receipt tamper refusal, and recovery after every fixed-path removal crash point.

The repository-wide portable manifest selected 115 scripts and passed every behavioral script except the documentation-audience wrapper's expected pre-index visibility check for this new untracked verification record.

That exact wrapper passed separately against a temporary index containing the new record, and `bin/mx-test-run.sh --check-coverage` confirmed all 125 scripts remain assigned exactly once.

The ten required real-Herdr scripts could not run because the host did not have exactly one running default Herdr session, while the portable Herdr backend contracts and all Portion 11 tests passed.

The focused commands were:

```sh
cargo test -p multplx-domain review_delivery --no-fail-fast
cargo test -p multplx-cli --test review_runtime --no-fail-fast
tests/mx-deep-review-config-contract.test.sh
tests/mx-deep-review-lib.test.sh
tests/mx-deep-review.test.sh
tests/mx-pr-merge.test.sh
tests/mx-push-service.test.sh
tests/mx-review-diff.test.sh
tests/mx-pr-check-security-parser-entrypoints.test.sh
tests/mx-pr-check-security-publication-migration.test.sh
tests/mx-pr-check-security-fault-quarantine.test.sh
tests/mx-pr-check-security-retirement-teardown.test.sh
```

## Compatibility and integration review

`maintainer-override`, `decision-hold-lifecycle`, and `harness-adapters` were reviewed completely.

Their authority, decision-routing, and headless harness contracts remain unchanged, so no skill procedure changed.

`AGENTS-PORTING.md`, delivery, architecture, configuration, scripts, workflow, hook, and CI references were reviewed.

The root broker contract now points the handoff schema at its Rust owner without adding implementation procedure to the always-loaded surface.

The workflow still invokes the stable deep-review filename and requires credentialed delivery outside agent sessions, so its stage semantics required no change.

No runtime backend transport changed in Portion 11.

The real official `gh` write path was not invoked from this agent session because that would violate the credential boundary.

The black-box suites instead proved exact `gh` argument construction with remote writes disabled or replaced by deterministic fakes and proved that a spawn-shaped agent environment cannot authenticate or push.

## Release performance

The security and behavioral gates take precedence over startup latency for this portion.

The representative operation was custom-check registration because it exercises task validation, private metadata checks, SHA-256 binding, same-directory atomic publication, mode enforcement, and post-publication verification without network variance.

Each selector ran 30 warm release-path iterations against a fresh mode-`0700` check and a unique valid task id in one temporary state directory.

The legacy median was 98.654 ms and nearest-rank p95 was 100.828 ms.

The Rust-default median was 16.791 ms and nearest-rank p95 was 21.522 ms.

The Rust path improved median latency by 83.0 percent and p95 by 78.7 percent without disabling any metadata, content, mode, link-count, device, or atomic-publication check.
