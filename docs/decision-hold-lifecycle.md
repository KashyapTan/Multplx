# Decision hold lifecycle mechanism

The normative policy is owned by `.agents/skills/decision-hold-lifecycle/SKILL.md` and is not restated here.
This document records the deterministic mechanism, structured surfaces, and privacy-safe regression evidence.

## Mechanism

`bin/mx-decision-hold.sh` remains the only lifecycle command for an investigation or visual review's unresolved maintainer decisions and selects the Rust authority entry before mutation.
`multplx-domain::decision_hold` owns typed hold identities, sorted inventory unions, and exact resolution retry identities.
The command uses the owned backlog library in the active `MX_HOME`, so the existing backlog remains the only durable work database and a daemon-owned decision stays in the daemon home.
It never reads report bodies, review artifacts, terminal output, or chat.

The `hold` subcommand maps an originating work id and stable decision key to `<origin-id>-decision-<decision-key>`.
It creates a Multplx kind `maintainer` backlog item when absent and invokes the library's hold operation on every retry.
It rejects an identity collision, a changed title, and attempts to reopen an already resolved identity.

The `complete` subcommand unions the reviewed keys into `decision_keys=` and appends `decisions_reviewed=1` while originating task metadata is live.
A post-teardown visual review can complete against the surviving report and durable holds without recreating volatile task metadata.
It accepts `--none` as an explicit semantic inventory result, not as inferred absence.
It verifies every listed identity against the owned backlog before recording completion.
For an open keyed status decision, it appends a `maintainer-held [key=<key>]: ...` transfer event only after the matching backlog hold is durable.
`bin/mx-classify-lib.sh` recognizes that transfer as closing the live status copy without claiming that the maintainer has answered it.

Scout teardown calls the script's read-only `verify` subcommand after checking for the report and before removing any source state.
The `--force` path remains the explicit maintainer-approved discard escape hatch.

The `resolve` subcommand requires a decision file and at least one existing dependent task whose structured `blocked-by` edge points to the hold.
It records the decision digest and routed task identities as a retry identity in the hold body, clears each dependency edge through the backlog library, and marks the hold Done only after those writes succeed.
An exact retry can finish a partial routing operation, while a changed decision or routed-task set is rejected.
A failed intermediate step leaves the hold open.

## Structured read surfaces

`bin/mx-system-snapshot.sh` parses canonical `(hold: ...)` and `(hold-kind: maintainer)` metadata alongside existing backlog fields.
It resolves every repeated `blocked-by:` edge against structured Done records, keeps missing blockers unresolved, and classifies only an unblocked maintainer hold as actionable.
Its daemon-home summary classifies an actionable maintainer hold as `maintainer_decision` and preserves blocked maintainer holds as queued work in the owning home.

`bin/mx-status-snapshot.sh` projects actionable maintainer holds into `decisions_open` and leaves blocked maintainer holds in ordinary queued gates.
It excludes completed kind `maintainer` records from Recently Landed.
The projection remains read-only and does not inspect historical prose.

## Verification

Current dated regression proof lives in [`verification/guards.md`](verification/guards.md#decision-hold-completion-gate).
