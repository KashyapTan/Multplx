# Task and daemon lifecycle verification

This record covers Rust-port Portion 07 on 2026-08-11.

Production task and daemon lifecycle have one implementation: the Rust multicall boundary.

The Rust release binary was built with `cargo build --workspace --release` on macOS 26.5.2 arm64 with Rust 1.97.1.

## Behavior and safety evidence

The focused suites passed for brief scaffolding, strict and acknowledged send, pending replies, spawn batching and dispatch profiles, worktree settlement, daemon lifecycle and safety, daemon sync, shared inheritance, system sync, update, upstream review, project memory, and teardown.

The daemon safety suite covered home aliases, ancestors, descendants, nested homes, symlink escape, registry conflicts, failed lease return, unproven Git locks, dirty worktrees, and child cleanup refusal.

The pending-reply suite covered interrupted publication, delivery-unknown reconciliation, wrong-home reports, bounded recovery, duplicate escalation prevention, restart durability, and unrelated-correlation refusal.

The Rust workspace passed `cargo fmt --all --check`, strict Clippy for all targets and features, and `cargo test --workspace`.

The required `cargo llvm-cov` workspace gate passed at 93.02 percent line coverage with focused lifecycle unit tests and instrumented `mx` command-boundary tests.

The documentation audience check and the exact 125-test runner inventory check passed.

The accelerated 125-test run completed in 331,009 ms.

All Plan 07 families passed in that run.

Ten required Herdr tests could not provision their isolated lab because the maintainer-owned `default` session was stopped, and changing that external session was outside this verification run's authority.

The AFK injection concurrency failure passed immediately when rerun alone, and the brief isolation failure was corrected and passed on rerun.

The snapshot projection chmod-000 daemon-home fixture passed after Rust validation restored the legacy operational-directory traversal check.

## Release performance comparison

The measured operation was a successful new delivery brief scaffold into one isolated empty home.

Each implementation ran 20 warm iterations through the public `bin/mx-brief.sh` adapter with a unique valid task id and output redirected away.

The legacy raw milliseconds were `31.742, 31.426, 31.891, 36.205, 35.230, 33.030, 32.108, 32.898, 33.137, 32.312, 32.518, 34.782, 33.949, 34.824, 34.869, 34.929, 34.115, 34.743, 34.047, 35.117`.

The Rust raw milliseconds were `10.923, 10.741, 10.749, 10.872, 10.459, 10.376, 10.174, 10.539, 10.614, 11.660, 11.785, 10.342, 9.988, 10.453, 9.882, 10.547, 10.234, 10.242, 9.933, 10.301`.

The legacy median was 33.998 ms and nearest-rank p95 was 35.230 ms.

The Rust median was 10.456 ms and nearest-rank p95 was 11.660 ms.

The Rust path improved the representative transaction median by 69.2 percent and p95 by 66.9 percent without relaxing file-creation, identity, or overwrite refusal.

## Retained compatibility surfaces

The public shell filenames remain as stable transports for existing operating homes.

`mx-ff-lib.sh` and `mx-pending-reply-lib.sh` retain sourced function ABIs for remaining callers, while their mutation primitives always dispatch to typed Rust operations.

Plan 13 removed the explicit legacy selector and retained executable bodies after the differential window.
