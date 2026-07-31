# Multplx test performance

This is the maintainer-verification owner for the Plan 6.5 baseline, targets, reproduction commands, and latest accepted proof.
The runner owns execution and machine-readable timing.
This document records evidence and does not redefine scheduler behavior.

## Plan-06 boundary

The authoritative Plan-06 confirmation run occurred on 2026-07-29 UTC.
`bin/mx-test-run.sh --all` executed the complete 96-script inventory in 3,127,519 ms, or 52 minutes 7.5 seconds.
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
bin/mx-test-run.sh --all --jobs 1 --json /tmp/mx-test-serial.json
```

Capture the accelerated run:

```sh
bin/mx-test-run.sh --all --jobs auto --json /tmp/mx-test-accelerated.json
```

Compare contract parity:

```sh
bin/mx-test-run.sh --compare-json /tmp/mx-test-serial.json /tmp/mx-test-accelerated.json
```

Re-run the resource proof:

```sh
bin/mx-test-isolation-proof.sh --jobs 4 --repeats 2 --json /tmp/mx-isolation-proof.json
```

The accepted local runs used Herdr 0.7.4 with the same headless default-session precondition as the required CI lane.
Each run verified that no `mx-lab-*` session survived and stopped only the default server process started for that run.

## Accepted proof

The accepted proof table is updated only from complete runner JSON artifacts.

| Evidence | Date | Result |
|---|---|---|
| Plan-06 serial boundary | 2026-07-29 UTC | 96 scripts, 3,127,519 ms, 1 known branch-topology failure, 9 expected skips |
| Plan-6.5 split assertion map | 2026-07-29 UTC | 140 cases mapped exactly once |
| Resource isolation proof | 2026-07-31 UTC | 96 portable candidates x 1 round, 250,253 ms, 0 failed rounds, 0 leaks, 482 conflict pairs |
| Accelerated local run 1 | 2026-07-29 UTC | 106 scripts, 365,595 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Accelerated local run 2 | 2026-07-29 UTC | 106 scripts, 370,606 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Accelerated local run 3 | 2026-07-29 UTC | 106 scripts, 367,851 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Current serial parity reference | 2026-07-29 UTC | 106 scripts, 1,258,821 ms, 0 failed, 9 expected skips, 1,501 assertions |
| Current accelerated parity run | 2026-07-29 UTC | 106 scripts, 370,653 ms, 0 failed, 9 expected skips, 1,501 assertions |

The three-run accelerated median is 367,851 ms, or 6 minutes 7.9 seconds.
The maximum is 370,606 ms, or 6 minutes 10.6 seconds.
The current serial-to-accelerated speedup is 3.40x.
The exact parity command reported `MX_TEST_PARITY ok scripts=106 assertions=1501`.
The archived resource proof is `docs/mx-test-isolation-proof.json`, whose manifest hash matches the current 116-script runner manifest.

CI evidence cannot be manufactured locally.
The three-main-branch-run critical-path target is evaluated after merge from uploaded timing artifacts.
