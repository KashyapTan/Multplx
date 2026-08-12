# System doctor

`bin/mx-doctor.sh` is the on-demand, read-only health sweep for one Multplx home.
The stable script enters the Rust session command boundary by default and accepts `MX_SESSION_IMPLEMENTATION=legacy` as the explicit rollback selector.
Run it when the system looks inconsistent, after an interrupted lifecycle, or before choosing a recovery procedure.
The default command inspects durable state and live process or backend evidence without changing the Multplx home.

```sh
bin/mx-doctor.sh
bin/mx-doctor.sh --check watcher-lock
bin/mx-doctor.sh --json
bin/mx-doctor.sh --fix
```

The process exits `0` when every selected check is `OK`, `1` when the worst result is `WARN`, and `2` when any result is `FAIL`.
`--check <name>` runs one named check with the same renderer and exit policy.
`--json` emits the `mx-doctor.v1` object with the worst severity, exit code, counts, findings, and applied-fix descriptions.

## Severity meanings

- `OK` means the observed state satisfies the check or the relevant optional state is absent.
- `WARN` means recovery is not immediately required, but an aged, incomplete, or compatibility condition deserves operator review.
- `FAIL` means a required invariant is broken, a durable record cannot be reconciled with live evidence, or uncertainty makes the state unsafe to treat as healthy.

Doctor reports evidence and an owning command instead of inferring permission to complete a lifecycle.
A `FAIL` does not authorize teardown, process termination, hold resolution, gate abandonment, workflow resumption, or link repair.

## Checks

| Check | Severity contract and evidence |
| --- | --- |
| `watcher-lock` | `OK` when the watcher lock is absent or its PID identity matches a live process, and `FAIL` when the lock is malformed, provably stale, or cannot be proved safe. |
| `watcher-beacon` | `OK` while the home is idle or the supervision beacon is fresh, and `WARN` when in-flight task metadata exists without a fresh beacon. |
| `orphan-worktrees` | `FAIL` when a task metadata record names a missing worktree or an active Treehouse worktree has no owning task or daemon record, and `WARN` when Treehouse inventory cannot be read. |
| `dangling-pids` | `FAIL` when a persisted task, watcher, away-mode, or sub-supervisor PID is dead, reused, or missing its required identity. |
| `stateless-sessions` | `FAIL` when task metadata has no live target through its recorded runtime backend. |
| `wake-queue-orphans` | `FAIL` when a task-scoped queue row has no task metadata or a queue row is malformed, while global watcher rows remain valid without task metadata. |
| `open-holds` | `FAIL` when the backlog is invalid or an open maintainer hold has no live task, completed decision attestation, archived report, or backlog origin. |
| `dispatch-queue-age` | `WARN` when a valid parked request under `state/.dispatch-queue/` exceeds the configured age, and `FAIL` when a request is malformed. |
| `gate-runs` | `OK` for terminal runs or nonterminal runs backed by a live task, and `FAIL` for malformed or unowned nonterminal gate records. |
| `workflow-runs` | `OK` for terminal runs, intentional waits, live actor stages, or a running reconcile owner, and `FAIL` for malformed or abandoned nonterminal records. |
| `orphan-servers` | `FAIL` when a vplan or future visualization run record lacks its identity-matched process or a reserved loopback port has an unrecorded listener. |
| `tools` | `FAIL` when a universal or selected-backend tool is absent, the backend is invalid, or Treehouse lacks durable lease support. |
| `primary-tangle` | `FAIL` when the primary checkout is on a named non-default branch, using the same shared tangle probe as bootstrap. |
| `compat-symlinks` | `OK` when retired compatibility paths are absent or resolve, and `WARN` when a configured path dangles or is not a symlink. |

The tools check intentionally verifies the `gh` executable but does not require an authenticated GitHub session.
The agent environment remains uncredentialed by default, and credentialed delivery stays outside agent context.

## Safe repair boundary

`--fix` has exactly two permitted repairs.

- It can clear `state/.watch.lock` only after the shared lock proof establishes that it is stale, the lock signature remains unchanged, and race-safe acquisition succeeds.
- It can remove only wake-queue rows whose task metadata is definitely absent, while holding the queue's own lock and re-evaluating every row under that lock.

The affected checks run after repair, so the report describes the resulting state.
Malformed rows, unreadable evidence, `lsof` errors, identity uncertainty, and concurrent ownership all preserve state and remain findings.
A second `--fix` on the repaired state makes no further filesystem changes.

## Thresholds and compatibility paths

`MX_DOCTOR_WATCHER_GRACE` sets the watcher-beacon grace in seconds and defaults to `MX_GUARD_GRACE` or `300`.
`MX_DOCTOR_LOCK_STALE_SECS` sets the minimum watcher-lock age for stale proof and defaults to `MX_LOCK_STALE_AFTER` or `2`.
`MX_DOCTOR_DISPATCH_MAX_AGE_SECS` sets the parked dispatch warning age and defaults to `172800`, or 48 hours.
`MX_DOCTOR_COMPAT_PATHS` may replace the newline-separated compatibility-path inventory for an installation with different retired paths.

Bootstrap and doctor share tool, Treehouse compatibility, and primary-tangle logic through `bin/mx-probe-lib.sh`.
Bootstrap retains its session-start diagnostics and mutation gates, while doctor provides a separate on-demand read-only report.
