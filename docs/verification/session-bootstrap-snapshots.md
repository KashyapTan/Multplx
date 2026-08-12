# Session, bootstrap, health, and snapshot verification

This record covers Rust-port Portion 09 on 2026-08-12.

The production selector is `MX_SESSION_IMPLEMENTATION`, defaults to `rust`, and accepts `legacy` only for bounded differential verification before an operation begins.

The Rust release binary was built with `cargo build --workspace --release` on macOS arm64 with Rust 1.97.1 and Cargo 1.97.1.

## Implementation and safety evidence

All nine owned public entry points select the Rust command boundary by default: bootstrap, doctor, session start, session-start nudge, supervision instructions, status snapshot, system snapshot, system view, and timeline.

The nudge, supervision renderer, typed system-view projection, and timeline reader are native Rust implementations.

The composed bootstrap, doctor, session-start, status-snapshot, and canonical-snapshot bodies remain available behind the explicitly pinned legacy selector for the differential rollback window.

The Rust compatibility boundary sets that selector before lock acquisition, bootstrap mutation, proof-bound doctor repair, wake drain, recursive daemon-home summary, or status projection can begin.

The typed snapshot read model covers roots, backlog records, tasks, endpoints, queues, watcher identity, daemon summaries, lifecycle feeds, and artifact feeds.

The human system view parses one canonical JSON observation and never rereads operational state.

The timeline reader preserves append order and exact matching JSONL bytes, skips malformed rows with one counted warning, supports bounded filters, and atomically publishes mode-`0600` HTML artifacts.

## Commands and results

`cargo fmt --all -- --check` completed successfully.

`cargo check --workspace` completed successfully.

`cargo build --workspace --release` completed successfully.

`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` completed successfully.

`cargo test --locked --workspace` passed all 295 unit, integration, and documentation tests.

The exact CI coverage command passed its enforced floor with 93.01 percent line coverage after applying the CI-owned real-Herdr module exclusions.

`cargo audit --deny warnings` scanned all 68 locked dependencies against 1,216 RustSec advisories without a finding.

`cargo test -p multplx-domain session::` passed 3 tests.

`cargo test -p multplx-cli --test session_runtime` passed 6 tests covering nudge scope and lock suppression, native supervision, typed system view, timeline modes and failures, compatibility pinning, and unknown or missing entry refusal.

`tests/mx-bootstrap.test.sh` passed 20 focused bootstrap cases.

`tests/mx-doctor.test.sh` passed all 5 grouped doctor cases.

`tests/mx-sessionstart-nudge.test.sh` passed all 7 nudge and registration cases.

`tests/mx-supervision-instructions.test.sh` passed all 6 harness-rendering cases.

`tests/mx-system-snapshot-view.test.sh` passed all 17 structured snapshot and human-view cases.

`tests/mx-timeline.test.sh` passed all 4 journal-rendering cases.

The supervision renderer produced byte-identical output under Rust and legacy for claude, codex, cursor, pi, and unknown harnesses across ordinary, read-only/AFK, and repair-line modes.

The timeline text, JSONL, event-filter, and time-filter outputs were byte-identical under Rust and legacy for the committed fixture.

An invalid-selector matrix covered all nine public adapters, verified exit `2` for ordinary commands and the nudge's fail-open exit `0`, and observed no fixture file creation before refusal.

`bin/mx-test-run.sh --check-coverage` confirmed all 125 behavior scripts are classified.

The complete 125-script local run returned 114 successful entries, one documentation-inventory failure caused only by this new record not yet being tracked, and ten live-Herdr tripwire failures because the machine did not have exactly one running default Herdr session.

The documentation audience checker passed against a temporary index containing this new tracked record with 72 surfaces and 229 local links, without changing the working index.

Every non-Herdr behavior entry other than that pre-commit inventory condition passed in the complete run, including all ten session-bootstrap entries.

## Release performance

The Portion 01 shell baseline used isolated empty homes on this same macOS arm64 machine and recorded session-start median/p95 of 1,550/1,602 ms across ten iterations and empty-system-snapshot median/p95 of 150/152 ms across twenty iterations.

The Portion 09 release measurement used the same empty-home shapes, five warmups, twenty session-start samples, thirty snapshot samples, Perl `Time::HiRes`, and nearest-rank p95.

| Target | Portion 01 shell median | Portion 09 Rust median | Portion 01 shell p95 | Portion 09 Rust p95 |
| --- | ---: | ---: | ---: | ---: |
| Session start | 1,550 ms | 407.226 ms | 1,602 ms | 424.580 ms |
| Empty system snapshot | 150 ms | 139.696 ms | 152 ms | 149.721 ms |

The first measured Rust invocations were 515.077 ms for session start and 157.759 ms for the empty system snapshot, compared with the Portion 01 shell run's first samples of 1,546 ms and 151 ms.

The multi-sample release medians and p95 values are no worse for either target; the single snapshot cold observation adds 6.759 ms while entering the typed selector and rollback boundary before the retained canonical body.

## Compatibility review

The stable shell filenames remain because hooks, skills, workflows, operating homes, and later Rust portions call them directly.

Stock macOS Bash remains covered by an explicit `MX_SESSION_IMPLEMENTATION=legacy` CI lane until Portion 13 removes the rollback bodies.

The command names, arguments, environment overrides, output schemas, labels, ordering, and exit codes are unchanged.

The affected bootstrap-diagnostics, catchup, recap, harness-adapters, and stuck-actor-recovery skills retain valid public command pointers and required no-authority rules, so they needed no contract change.
