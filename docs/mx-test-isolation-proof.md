# Multplx test isolation proof (Phase 2)

This document is the archived concurrent isolation proof for the portable parallel candidate set.
It is the human-readable companion to `bin/mx-test-isolation-proof.sh`.
Phase 4 production portable shards and bounded local `mx-test-run.sh --jobs` for this exact set are owned by `bin/mx-test-run.sh` and documented in [mx-test-portable-shards.md](mx-test-portable-shards.md).
The archived proof JSON below still records the Phase 2 proof-time flags (`production_sharding_enabled` / `mx_test_run_jobs_enabled` false at proof time).

## Owner

- Harness: `bin/mx-test-isolation-proof.sh`
- Contract tests: `tests/mx-test-isolation-proof.test.sh`
- Family labels (Phase 1): `bin/mx-test-run.sh`
- Timing evidence used for planning: CI artifact `mx-test-timing` from Phase 1 PR #825

## Proof posture

| Field | Value |
|---|---|
| `run_id` | `mx-isolation-1784968984050-13742` |
| `started_at` | `2026-07-25T08:43:04Z` |
| `finished_at` | `2026-07-25T08:44:54Z` |
| concurrency | **4** |
| candidates | **26** |
| failed | **0** |
| wall duration_ms | **110623** (~110.6s) |
| `production_sharding_enabled` | `False` |
| `mx_test_run_jobs_enabled` | `False` |
| host proof date | 2026-07-25 (UTC day of archive write) |

Isolation checks that passed with this run:

- Distinct mode-`0700` temporary roots per worker under a proof-owned parent
- Per-worker `TMPDIR`/`TMP` so `mktemp` / `mx_test_tmproot` stay private
- Ambient `MX_HOME` / `MX_*_OVERRIDE` cleared for each worker
- `git config --global` snapshot unchanged before/after the matrix
- Aggregate failure reporting (any non-zero candidate fails the harness; no retry-until-green)

## Exact candidate set

Sorted paths as selected by `bin/mx-test-isolation-proof.sh --list` at proof time:

- `tests/mx-arm-pretool-check.test.sh`
- `tests/mx-backend-herdr.test.sh`
- `tests/mx-brief.test.sh`
- `tests/mx-maintainer-translation-contract.test.sh`
- `tests/mx-cd-pretool-check.test.sh`
- `tests/mx-composer-ghost.test.sh`
- `tests/mx-composer-lib.test.sh`
- `tests/mx-actor-state.test.sh`
- `tests/mx-decision-hold-lifecycle.test.sh`
- `tests/mx-ensure-agents-md.test.sh`
- `tests/mx-herdr-lab.test.sh`
- `tests/mx-instruction-owners.test.sh`
- `tests/mx-nm-test-contract.test.sh`
- `tests/mx-no-mistakes-ownership.test.sh`
- `tests/mx-pi-primary-types.test.sh`
- `tests/mx-pr-merge.test.sh`
- `tests/mx-review-diff.test.sh`
- `tests/mx-send-popup-settle.test.sh`
- `tests/mx-send-settle.test.sh`
- `tests/mx-send-strict.test.sh`
- `tests/mx-spawn-batch.test.sh`
- `tests/mx-stow-contract.test.sh`
- `tests/mx-supervision-instructions.test.sh`
- `tests/mx-test-run.test.sh`
- `tests/mx-tmux-submit-busy.test.sh`
- `tests/mx-transition-lib.test.sh`

## Per-candidate durations (concurrent run)

| duration_ms | exit | worker | script |
|---:|---:|---:|---|
| 29446 | 0 | 2 | `tests/mx-backend-herdr.test.sh` |
| 26535 | 0 | 1 | `tests/mx-arm-pretool-check.test.sh` |
| 18509 | 0 | 9 | `tests/mx-decision-hold-lifecycle.test.sh` |
| 17218 | 0 | 5 | `tests/mx-cd-pretool-check.test.sh` |
| 15250 | 0 | 8 | `tests/mx-actor-state.test.sh` |
| 11199 | 0 | 12 | `tests/mx-herdr-lab.test.sh` |
| 8900 | 0 | 26 | `tests/mx-test-run.test.sh` |
| 6630 | 0 | 18 | `tests/mx-pr-merge.test.sh` |
| 4496 | 0 | 20 | `tests/mx-send-popup-settle.test.sh` |
| 2410 | 0 | 19 | `tests/mx-review-diff.test.sh` |
| 2179 | 0 | 21 | `tests/mx-send-settle.test.sh` |
| 1845 | 0 | 27 | `tests/mx-tmux-submit-busy.test.sh` |
| 1842 | 0 | 17 | `tests/mx-pi-primary-types.test.sh` |
| 1810 | 0 | 6 | `tests/mx-composer-ghost.test.sh` |
| 1390 | 0 | 22 | `tests/mx-send-strict.test.sh` |
| 973 | 0 | 3 | `tests/mx-brief.test.sh` |
| 626 | 0 | 23 | `tests/mx-spawn-batch.test.sh` |
| 358 | 0 | 10 | `tests/mx-ensure-agents-md.test.sh` |
| 336 | 0 | 25 | `tests/mx-supervision-instructions.test.sh` |
| 297 | 0 | 13 | `tests/mx-instruction-owners.test.sh` |
| 181 | 0 | 4 | `tests/mx-maintainer-translation-contract.test.sh` |
| 180 | 0 | 15 | `tests/mx-nm-test-contract.test.sh` |
| 96 | 0 | 28 | `tests/mx-transition-lib.test.sh` |
| 66 | 0 | 7 | `tests/mx-composer-lib.test.sh` |
| 52 | 0 | 24 | `tests/mx-stow-contract.test.sh` |
| 35 | 0 | 16 | `tests/mx-no-mistakes-ownership.test.sh` |

## Audit notes (why this set)

Source families from the Phase 1 manifest and scout report §3.1:

1. **pure-contract-unit** candidates audited from the Phase 1 family manifest, minus deliberate serial exclusions
2. **Extra hermetic candidates** after static audit: fake backend, private git fixtures, stubbed network

The harness pins this exact archived set and does not automatically admit later family additions.
A candidate-set change requires a new audit and concurrent proof archive.

### Included extras (beyond pure-contract-unit)

| Script | Why included |
|---|---|
| `tests/mx-backend-herdr.test.sh` | Fake Herdr CLI + private temps; no real Herdr binary |
| `tests/mx-send-strict.test.sh` | Fake tmux PATH shim; private `MX_HOME` |
| `tests/mx-spawn-batch.test.sh` | Argument routing only; no real windows/worktrees |
| `tests/mx-pr-merge.test.sh` | Fake `gh`/`gh-axi`; private state |
| `tests/mx-review-diff.test.sh` | Local git fixtures via `mx_git_*`; no live forge |

### Deliberately serial (kept out of this pool)

Run `bin/mx-test-isolation-proof.sh --list-exclusions` for the machine-readable list.
High-signal classes:

| Class | Examples | Reason |
|---|---|---|
| Watcher / wake / locks | `mx-watcher-lock`, `mx-wake-queue`, ... | Intentional process locks and daemon races |
| AFK | `mx-afk-inject-e2e`, ... | Daemon lifecycle and inject path |
| Real Herdr | `mx-backend-herdr-smoke`, presentation e2e, ... | Named labs, session-global locks; Herdr lane is Phase 3+ |
| Real tmux smoke | `mx-backend-tmux-smoke` | Real multiplexer server (even on private socket) |
| Live harness opt-in | `mx-*-live-e2e` | Real interactive agents |
| GUI backends | cmux smoke | Shared GUI app |
| Gray-zone git/spawn | `mx-backend`, spawn settle/profile, teardown | Heavier worktree or lock-race matrices |
| Watcher-adjacent forge security | `mx-pr-check-security` | `.watch.lock` / poll security surface |
| Self | `mx-test-isolation-proof.test.sh` | Must not re-enter the concurrent matrix |

### Small isolation fix landed with this phase

`tests/mx-arm-pretool-check.test.sh` no longer writes Claude deny stderr to a fixed `/tmp/mx-arm-pretool-check-claude-stderr.$$` path.
It uses `mktemp` under `TMPDIR` so concurrent workers cannot collide on a global temp name pattern.

## Failures

None.
Every candidate exited 0 under concurrency=4.

Policy: a script that fails only under concurrency is **removed** from the candidate set and investigated.
It is never retried into green, skipped more broadly, or weakened in assertions.

## What this phase did not do (Phase 2 scope)

- Did not land production CI Behavior matrix / shard jobs (Phase 4)
- Did not add general `bin/mx-test-run.sh --jobs` (Phase 4 enables it only for this proven set)
- Did not land the Herdr install lane (Phase 3)
- Did not re-run the complete local suite as part of this proof (focused matrix only)

## How to re-run

```sh
bin/mx-test-isolation-proof.sh --list
bin/mx-test-isolation-proof.sh --jobs 4 --json /tmp/mx-isolation-proof.json
bash tests/mx-test-isolation-proof.test.sh
```
