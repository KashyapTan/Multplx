# Rust port final cutover verification

This record owns the local closeout evidence for Rust-port Portion 13 on 2026-08-13 EDT.
It supplements the subsystem-specific records under `docs/verification/` and does not replace their safety contracts.

## Environment

- macOS Darwin 25.5.0 on arm64.
- `rustc 1.97.1 (8bab26f4f 2026-07-14)`.
- `cargo 1.97.1 (c980f4866 2026-06-30)`.
- `mx 0.1.0` release build.
- Herdr 0.7.4, protocol 16.
- Codex CLI 0.146.1 and Cursor CLI 2026.08.04-aaa8809.
- jq 1.7.1 and Git 2.53.0.

The current closeout host exposes Codex CLI 0.146.1, Cursor CLI 2026.08.04-aaa8809, Herdr 0.7.4, and tmux.
Claude Code, Pi, and cmux are unavailable on this host, and the Herdr default session is stopped.
The version-scoped live Claude, Pi, and cmux observations remain in [`supervision.md`](supervision.md) and [`runtime-backends.md`](runtime-backends.md); this closeout run does not upgrade those historical observations into current-host evidence.

## Launcher and installation

`multplx-cli::launcher` owns literal path records, release-artifact verification, local builds, atomic install and rollback, update, uninstall, recursion refusal, activation, harness launch, and child-only working-directory changes.
The Plan-13-owned retained Bash and Zsh activation files and completed compatibility command names are transport adapters that locate and `exec` the installed binary.
The exhaustive bin inventory gate classifies every executable as a minimal adapter or a documented sourced-shell ABI.

The deterministic launcher suite passed existing-checkout, managed-install, upgrade, pre-mutation rollback, uninstall, broken-download recovery, collision refusal, interrupted publication, nested activation, live-lock, stale-lock, and harness environment cases.

The opt-in live boundary passed with:

```text
MX_TEST_SUMMARY total=1 failed=0 skipped_gate=0 duration_ms=858
ok - live codex launcher smoke: codex-cli 0.146.1
ok - live cursor launcher smoke: 2026.08.04-aaa8809
ok - real harness binaries execute through the child-root launcher
```

## Test tooling and documentation ownership

The release `mx test-run` command owns inventory, families, resources, conflict scheduling, longest-processing-time ordering, lanes, changed-file selection, timing markers, JSON aggregation and comparison, coverage proof, and aggregate exit status.
The release `mx test-isolation-proof` command derives the resource conflict matrix, repeats portable candidates in parallel, detects owned file and process leaks, and emits JSON evidence.
The release `mx doc-audience-check` command owns maintained-prose classification, setup routing, required owner pointers, and local-link validation.

The current documentation check reports `surfaces=75 local_links=229`.
The behavior-test inventory coverage guard classifies every tracked `tests/*.test.sh` path once.
The executable-bin inventory separately enumerates every tracked executable under `bin/`, validates minimal adapter structure, and records sourced-shell ABI owners.

The accepted repeated isolation proof was generated at a temporary path with this command and atomically promoted after validation:

```sh
target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json <temporary-proof-path>
```

It reported:

```text
MX_ISOLATION_SUMMARY total=105 failed_rounds=0 concurrency=4 repeats=2 duration_ms=665226 leaks=0
```

The current JSON SHA-256 is `5e7f6decd176f26f159e42ea54c6f39b64ece331eb9e75ac81ba501e844a68ca`.
The artifact records the 127-script manifest SHA-256 `7ad4468ac085aaecc98277c3a2f5dd00498c5314fa744215df82a5c9f1098df8`, 105 portable candidates per round, 644 conflict pairs, zero failed rounds, zero leaks, and zero known-failure observations.

## Historical complete behavior inventory

Before the executable-bin gate was added, the complete behavior command was:

```sh
target/release/mx test-run --all --jobs 4 --json /private/tmp/mx-plan13-final-current.json
```

It reported:

```text
MX_TEST_SUMMARY total=125 failed=0 skipped_gate=9 duration_ms=670621
```

All ten required real-Herdr tests passed in that inventory.
The nine skips were declared optional-binary or live-harness opt-in gates, not failures or newly introduced skips.
The local JSON artifact SHA-256 was `fee2ce3d7f6f0571bbf129887513ebd187fde2b95ed4834e64a2c499101c80bb`.

## Current-tree closeout commands

The following commands produce the required current-tree artifacts without changing test semantics:

```sh
mkdir -p /private/tmp/mx-plan13-closeout
target/release/mx test-run --all --jobs auto --json /private/tmp/mx-plan13-closeout/accelerated-1.json
target/release/mx test-run --all --jobs auto --json /private/tmp/mx-plan13-closeout/accelerated-2.json
target/release/mx test-run --all --jobs auto --json /private/tmp/mx-plan13-closeout/accelerated-3.json
target/release/mx test-run --all --jobs 1 --json /private/tmp/mx-plan13-closeout/serial.json
target/release/mx test-run --compare-json /private/tmp/mx-plan13-closeout/serial.json /private/tmp/mx-plan13-closeout/accelerated-3.json
target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-plan13-closeout/isolation-proof.json
```

The current-tree isolation, three accelerated, and serial artifacts have been produced and validated.
All four behavior runs covered the same 127 scripts and 1,474 assertions with zero failures and eight declared capability or opt-in skips.
The parity command reported `MX_TEST_PARITY ok scripts=127 assertions=1474`.
The accelerated durations were 312,296 ms, 313,461 ms, and 312,525 ms; the serial duration was 1,006,685 ms.
Their SHA-256 values are `3b9f7bfa080a3a3ddca394667d2e27ab8efbaaafe664aac90e23753725c08bc7`, `2f94e833faec9f882b48e184ea3978f267af28736b7f8b1cb724b23bb39cd1f0`, `48bd93c2cd956a490a8dba27656421a095f72a17a1d84cf6f5e67a361722fd7d`, and `e4c807d2c228b2a290fab5064d9c2b3e0a53d67bd9cf611a0c01f38a88b38850`, respectively.
Required live Claude, Pi, and cmux checks must run on a host where those binaries are installed; they cannot be manufactured on this host.

## Release performance comparison

Measurements used the optimized release binary on the same host as the Portion 01 baseline, warmups before samples, Perl `Time::HiRes`, medians, and nearest-rank p95.
No production timeout, retry, debounce, liveness, locking, or verification bound was reduced.

| Target | Portion 01 median | Final median | Portion 01 p95 | Final p95 | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Backlog help startup | 17.500 ms | 10.519 ms | 18.000 ms | 11.202 ms | Median improved 39.9% |
| Workflow validation | 56.000 ms | 10.746 ms | 58.000 ms | 12.524 ms | Median improved 80.8% |
| Session start | 1,550.000 ms | 1,588.204 ms | 1,602.000 ms | 1,597.084 ms | Median +2.5%; p95 improved |
| Empty system snapshot | 150.000 ms | 183.333 ms | 152.000 ms | 187.702 ms | See paired robustness note |
| Complete behavior suite | 406,860 ms | 312,525 ms | n/a | 313,461 ms | Median improved 23.2%; three clean current-tree samples |

The empty-snapshot host had drifted since the Portion 01 baseline.
A same-run paired comparison measured the retained compatibility reference at 174.749 ms median and 178.772 ms p95, and the Rust-selected path at 182.851 ms median and 188.257 ms p95.
The final path therefore added 8.102 ms median, or 4.6 percent, at this composed system-snapshot boundary while retaining typed selection, bounded subprocess output, and the complete safety projection.
The native typed snapshot and view components remain covered by the faster Portion 09 measurements in [`session-bootstrap-snapshots.md`](session-bootstrap-snapshots.md).

Watcher CPU/RSS, wake transaction latency, runtime backend latency, task lifecycle latency, workflow resume, review security, and launcher activation measurements remain owned by their current subsystem records.

## Root instruction restoration

The temporary root `AGENTS-PORTING.md` path is absent and `AGENTS.md` is the sole exact root contract filename.
Instruction-owner tests prove production discovery does not accept the temporary name.

A fresh ephemeral Codex CLI session was opened directly in the restored worktree.
Codex reported that it loaded the worktree's root `AGENTS.md`, issued exactly one `bin/mx-session-start.sh` command execution as its first and only tool call, received exit 0, and returned `RESTORED_AGENTS_SESSION_START_OK`.
This proves normal exact-name auto-loading and one-time session start through the supported Codex harness.

## Strict closeout gates

The following commands completed successfully on the current tree:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo build --release --workspace --locked
cargo audit --deny warnings
cargo llvm-cov --locked --workspace --all-targets --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' --fail-under-lines 93
target/release/mx test-run --check-coverage
target/release/mx doc-audience-check
```

`tests/mx-bin-runtime-inventory.test.sh` passes after the native spawn and supervise-daemon cutovers.

Cargo Audit scanned the locked dependency graph without a finding.
The narrowed coverage command measures 93.14 percent line coverage and meets the unchanged 93 percent gate.
Pure CLI entrypoint and dispatch code remains in the denominator; process-driven CLI and OS-lifecycle modules remain owned by the complete black-box lanes above.
The Cargo metadata license sweep found no external package missing a declared license expression.
