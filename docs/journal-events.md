# Task journal events

Task journals are append-only, best-effort observability projections at `state/<task-id>.journal`.
They are never operational truth and no reconciliation, classification, supervision, gate, workflow, or delivery decision may read them.
The per-purpose state records remain authoritative and reconstructable when a journal is missing, incomplete, malformed, or unwritable.

`bin/mx-journal-lib.sh` owns append mechanics, task-id validation, the executable event allowlist, and warn-once failure behavior.
This page is the single prose owner of the event vocabulary and detail contracts.
Production writers call `mx_journal_try`, so journal failures cannot alter the operation being observed.
`bin/mx-timeline.sh` is the only production reader.

Every line is one compact JSON object with these envelope fields:

| Field | Contract |
| --- | --- |
| `ts` | UTC ISO-8601 timestamp at second precision |
| `task` | Privacy-safe task or workflow-run id |
| `source` | Emitting script basename without `.sh` |
| `event` | One event from the closed vocabulary below |
| `detail` | Event-specific JSON object |

Append order is the timeline order.
The timestamp is informative and is used by the reader's `--since` filter, not to reorder records.
Malformed or torn lines are skipped by the reader with one warning count.

## Closed vocabulary

| Event | Source | Required detail |
| --- | --- | --- |
| `task.spawned` | `mx-spawn` | `kind`, `backend`, `worktree`, `branch` |
| `status.reported` | `mx-report` | `raw`, `state`, `validated` |
| `status.classified` | `mx-watch` or `mx-supervise-daemon` | `verdict`, `tier`, `conflicts` |
| `gate.step.started` | `mx-deep-review` | `step`, `round` |
| `gate.step.finished` | `mx-deep-review` | `step`, `round`, `findings`, `outcome` |
| `hold.opened` | `mx-decision-hold` | `decision_key`, `hold_id`, `title` |
| `hold.resolved` | `mx-decision-hold` | `decision_key`, `hold_id`, `routed_to` |
| `workflow.stage.entered` | `mx-workflow` | `run`, `stage` |
| `workflow.stage.gated` | `mx-workflow` | `run`, `stage`, `gate`, `outcome` |
| `delivery.queued` | `mx-deliver` | `branch`, `sha` |
| `delivery.pushed` | `mx-deliver` | `branch`, `sha` |
| `delivery.pr_opened` | `mx-deliver` | `pr_url` |

The `state` value in `status.reported` is one of `working`, `paused`, `blocked`, `needs-decision`, `done`, `failed`, or `resolved`.
The `validated` value is always boolean `true` because the event is emitted only after `mx-report` has accepted and appended the status line.

The `tier` value in `status.classified` is `native-event`, `attributed-run-step`, `validated-report`, or `regex-heuristic`.
The attributed run-step tier records the validation evidence that Plan 5 placed between native events and validated reports after Plan 12 was drafted.
Each `conflicts` entry is an object with `tier` and `signal` fields for a recognized lower-precedence observation whose normalized verdict disagreed with the winner.
An empty array means no weaker signal conflicted.

The `gate` value in `workflow.stage.gated` is `approve` or `auto`.
The `outcome` value records the durable result observed at that gate, such as `passed`, `waiting`, or `failed`.

## Lifecycle and retention

Journals are private task state and are removed by `mx-teardown.sh` with the rest of the task's transient state.
Version 1 does not backfill older tasks, rotate journals, or create a cross-task journal.
Long-term evidence remains in the existing delivery, gate, workflow, backlog, and report owners rather than being promoted from this projection.
