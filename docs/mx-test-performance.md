# Multplx test performance

This is the maintainer-verification owner for the Plan 6.5 baseline, targets, reproduction commands, and latest accepted proof.
The runner owns execution and machine-readable timing.
This document records evidence and does not redefine scheduler behavior.

## Plan-06 boundary

The authoritative Plan-06 confirmation run occurred on 2026-07-29 UTC.
The archived Plan-06 `bin/mx-test-run.sh --all` run executed the complete 96-script inventory in 3,127,519 ms, or 52 minutes 7.5 seconds.
Ninety-five scripts passed.
Nine scripts reported expected environment-gated skips.
One existing feature-branch merge-base conformance case failed without aborting aggregate evidence.
That branch-topology failure is recorded separately from the performance target.

The five slowest scripts were:

| Script | duration_ms |
|---|---:|
| `tests/mx-pr-check-security.test.sh` | 667041 |
| `tests/mx-backend-herdr-presentation-e2e.test.sh` | 406318 |
| `tests/mx-status-snapshot.test.sh` | 205536 |
| `tests/mx-daemon-harness.test.sh` | 195373 |
| `tests/mx-session-start.test.sh` | 161778 |

Those five scripts consumed 52.3 percent of serial time.
The fifteen slowest scripts consumed 75.4 percent.
The machine-readable archive is `docs/mx-test-performance-baseline.json`.
That archive preserves all 96 per-script rows, all family totals, all exits and gate-skip classes, and all 1,494 named assertion labels.
The canonical Plan-06 assertion inventory SHA-256 is `66181f2ddaa32c7efc44e1c10be2c4956b55263affcc9eef9c7bc64185cf3468`.

| Family | scripts | duration_ms | failed |
|---|---:|---:|---:|
| `afk` | 2 | 43130 | 0 |
| `backend-dispatch` | 8 | 149840 | 1 |
| `cmux` | 2 | 16673 | 0 |
| `daemon` | 8 | 386632 | 0 |
| `live-harness-optin` | 4 | 346 | 0 |
| `pr-forge` | 4 | 760667 | 0 |
| `pure-contract-unit` | 33 | 206900 | 0 |
| `real-herdr-gated` | 10 | 559957 | 0 |
| `session-bootstrap` | 8 | 352353 | 0 |
| `snapshot-catchup` | 2 | 228628 | 0 |
| `unclassified` | 4 | 90227 | 0 |
| `watcher-wake-lock` | 11 | 322463 | 0 |

## Targets

The local accelerated full-run median must be at most 15 minutes across three consecutive clean runs.
No one of those three local runs may exceed 18 minutes.
The required CI behavior critical path must be at most 12 minutes after three green main-branch runs.
Serial and accelerated runs must agree on per-script exit class, expected-skip class, and named assertion multiset.
No production safety timeout, retry count, debounce window, or liveness threshold may be reduced for performance.
No new skip, retry-to-green behavior, removed scenario, or weakened fault matrix counts as an optimization.

## Retained real-time smokes

The slow-suite audit distinguished test waiting from child workloads and production-timeout subjects.
PR security retains short publication-race delays and timeout children because ordering and descendant cleanup are the behaviors under test.
Status snapshot retains stalled fake tools that production timeout wrappers must terminate.
Daemon harness retains one filesystem-mtime boundary and one concurrent publication gate.
Session-start retains long-lived child sleeps that the tested teardown path kills immediately.
Watcher suites retain intervals that prove a process stays live and that a wedge timer is not reset.
Real-Herdr presentation retains long-lived workload children while its orchestration waits remain state-based.
The AFK Herdr E2E settling sleeps were replaced with bounded observable-state waits.
None of these retained smokes changes a production timeout or makes the runner wait for the child workload's nominal sleep duration.

## Reproduction

Capture the serial reference:

```sh
target/release/mx test-run --all --jobs 1 --json /tmp/mx-test-serial.json
```

Capture the accelerated run:

```sh
target/release/mx test-run --all --jobs auto --json /tmp/mx-test-accelerated.json
```

For final-tree closeout, preserve three independent accelerated artifacts rather than overwriting one path:

```sh
mkdir -p /private/tmp/mx-plan13-closeout
target/release/mx test-run --all --jobs auto --json /private/tmp/mx-plan13-closeout/accelerated-1.json
target/release/mx test-run --all --jobs auto --json /private/tmp/mx-plan13-closeout/accelerated-2.json
target/release/mx test-run --all --jobs auto --json /private/tmp/mx-plan13-closeout/accelerated-3.json
target/release/mx test-run --all --jobs 1 --json /private/tmp/mx-plan13-closeout/serial.json
target/release/mx test-run --compare-json /private/tmp/mx-plan13-closeout/serial.json /private/tmp/mx-plan13-closeout/accelerated-3.json
target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-plan13-closeout/isolation-proof.json
```

Do not replace the accepted proof table with these rows until all three accelerated runs are clean, serial comparison reports exact parity, and the isolation artifact reports no failed rounds or leaks.

Compare contract parity:

```sh
target/release/mx test-run --compare-json /tmp/mx-test-serial.json /tmp/mx-test-accelerated.json
```

Re-run the resource proof:

```sh
target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /tmp/mx-isolation-proof.json
```

The accepted local runs used Herdr 0.7.4 with the same headless default-session precondition as the required CI lane.
Each run verified that no `mx-lab-*` session survived and stopped only the default server process started for that run.

## Accepted proof

The accepted proof table is updated only from complete runner JSON artifacts.

| Evidence | Date | Result |
|---|---|---|
| Plan-06 serial boundary | 2026-07-29 UTC | 96 scripts, 3,127,519 ms, 1 known branch-topology failure, 9 expected skips |
| Plan-6.5 split assertion map | 2026-07-29 UTC | 140 cases mapped exactly once |
| Resource isolation proof | 2026-08-03 UTC | 100 portable candidates x 2 rounds, 518,489 ms, 0 failed rounds, 0 leaks, 503 conflict pairs |
| Lean Phase-09 resource isolation proof | 2026-09-16 EDT | 108 portable candidates x 2 rounds, 553,872 ms, 0 failed rounds, 0 leaks, 653 conflict pairs |
| Lean Phase-05 resource isolation proof | 2026-09-15 EDT | 107 portable candidates x 2 rounds, 538,460 ms, 0 failed rounds, 0 leaks, 650 conflict pairs |
| Lean Phase-03 resource isolation proof | 2026-09-15 EDT | 106 portable candidates x 2 rounds, 471,803 ms, 0 failed rounds, 0 leaks, 647 conflict pairs |
| Plan-13 resource isolation proof | 2026-08-13 EDT | 105 portable candidates x 2 rounds, 665,226 ms, 0 failed rounds, 0 leaks |
| Plan-13 accelerated local run 1 | 2026-08-13 EDT | 127 scripts, 312,296 ms, 0 failed, 8 declared skips, 1,474 assertions |
| Plan-13 accelerated local run 2 | 2026-08-13 EDT | 127 scripts, 313,461 ms, 0 failed, 8 declared skips, 1,474 assertions |
| Plan-13 accelerated local run 3 | 2026-08-13 EDT | 127 scripts, 312,525 ms, 0 failed, 8 declared skips, 1,474 assertions |
| Plan-13 serial parity reference | 2026-08-13 EDT | 127 scripts, 1,006,685 ms, 0 failed, 8 declared skips, 1,474 assertions |
| Accelerated local run 1 | 2026-07-29 UTC | 106 scripts, 365,595 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Accelerated local run 2 | 2026-07-29 UTC | 106 scripts, 370,606 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Accelerated local run 3 | 2026-07-29 UTC | 106 scripts, 367,851 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Historical serial parity reference | 2026-07-29 UTC | 106 scripts, 1,258,821 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Historical accelerated parity run | 2026-07-29 UTC | 106 scripts, 370,653 ms, 0 failed, 9 expected skips, 1,501 assertions |

The current Plan-13 three-run accelerated median is 312,525 ms, or 5 minutes 12.5 seconds.
The maximum is 313,461 ms, or 5 minutes 13.5 seconds.
The current serial-to-accelerated speedup is 3.22x.
The exact parity command reported `MX_TEST_PARITY ok scripts=127 assertions=1474`.
The historical Plan-13 127-script resource proof was produced at a temporary path with `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json <temporary-proof-path>` and atomically promoted to `docs/mx-test-isolation-proof.json` after validation.
It reported `MX_ISOLATION_SUMMARY total=105 failed_rounds=0 concurrency=4 repeats=2 duration_ms=665226 leaks=0`.
That historical proof recorded manifest SHA-256 `7ad4468ac085aaecc98277c3a2f5dd00498c5314fa744215df82a5c9f1098df8`, 644 declared conflict pairs, and no known-failure observations.

The historical 128-script resource proof was regenerated for lean Phase 03 with `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase03-lock-isolation-proof.json` and its successful JSON atomically copied into the archive.
It reports 106 portable candidates across two rounds, 471,803 ms, no failed rounds, no leaks and no known-failure exceptions.
Its manifest SHA-256 is `afee0940e7b2037ad8116df2c48422d674100574ce9fa547c2a7c9c09a100c0f`, with 647 declared conflict pairs.
[Phase 03 evidence](../plans/lean_redesign/phase03-implementation.md) owns the changed lifecycle validation and costs; this update does not claim a new three-run performance or serial-parity baseline.

The historical 129-script resource proof was regenerated for lean Phase 05 with `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase05-closeout-isolation-proof.json` and its verified JSON atomically copied into the archive.
It reports 107 portable candidates across two rounds, 538,460 ms, no failed rounds, no leaks and no known-failure exceptions.
Its resource manifest SHA-256 is `d00044b128e8837a4c70f77349e7d844f461a910cac44d0622f2d0e7baf7ef11`, with 650 declared conflict pairs.
[Phase 05 evidence](../plans/lean_redesign/phase05-implementation.md) owns coordinator behavior validation; this resource-proof refresh does not establish a new three-run performance or serial-parity baseline.

The current 130-script resource proof was regenerated for lean Phase 09 with `target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /private/tmp/mx-phase09-isolation-proof.json` and its verified JSON copied into the archive.
It reports 108 portable candidates across two rounds, 553,872 ms, no failed rounds, no leaks and no known-failure exceptions.
Its resource manifest SHA-256 is `cd0877f1fbab85d7627fa221fd65283b04c6d99dcdf235fc20541ac8abd85b08`, with 653 declared conflict pairs.
[Phase 09 evidence](../plans/lean_redesign/phase09-implementation.md) owns migration validation; this resource-proof refresh does not establish a new three-run performance or serial-parity baseline.

CI evidence cannot be manufactured locally.
The three-main-branch-run critical-path target is evaluated after merge from uploaded timing artifacts.
