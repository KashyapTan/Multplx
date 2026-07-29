# Decision hold lifecycle mechanism

The normative policy is owned by `.agents/skills/decision-hold-lifecycle/SKILL.md` and is not restated here.
This document records the deterministic mechanism, structured surfaces, and privacy-safe regression evidence.

## Mechanism

`bin/mx-decision-hold.sh` is the only lifecycle command for an investigation or visual review's unresolved maintainer decisions.
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

## Verification record

Verification date: 2026-07-14.
Additional quoted `blocked_by` regression verification date: 2026-07-17.
Plural blocker-readiness and mixed-home projection verification date: 2026-07-22.

The focused end-to-end regression uses only synthetic `sample` identities and decision text.
It begins with a completed investigation and visual review whose genuine unresolved choice exists only in the report.
The initial Catchup snapshot correctly has no open decision, and the new teardown gate refuses to erase the source.
A later regression covers quoted multi-entry `blocked_by` output so `resolve` matches the first, middle, and last ids and rejects a genuinely absent id.

The final verification commands and their exact summarized outputs follow.

```text
$ bash tests/mx-decision-hold-lifecycle.test.sh
ok - report-only unresolved decision is reproduced and completion refuses before loss
ok - non-forced scout teardown always requires durable inventory verification
ok - maintainer holds are idempotent, distinct, teardown-safe, Catchup-visible, and durably routed before close
ok - completion and verification validate origins before constructing paths
ok - ended visual review follows the same decision-hold completion owner
ok - resolved findings and decision-like prose do not create false holds
ok - terminal single-owner stale status decisions do not block empty inventory
ok - main-home and daemon-home maintainer holds remain correctly routed
ok - resolve matches first/middle/last in quoted blocked_by and rejects a genuinely absent id

$ bash tests/mx-system-snapshot-view.test.sh
ok - backlog normalization preserves strict roles and resolves every blocker compatibly
ok - durable maintainer-held transfer closes the duplicate live status decision
ok - snapshot parses owned backlog rows and respects operational overrides

$ bin/mx-test-run.sh --family snapshot-catchup
ok - a completed scout with decision-like report prose is a pointer, not pending
ok - action-free items (working/done/queued/landed) do not leak into Maintainer's Call
ok - mixed daemon roles, partial state, and maintainer readiness project independently
ok - main and daemon maintainer actionability use the same blocker readiness

$ bash tests/mx-brief.test.sh
ok - mx-brief.sh: investigation and visual-review completions load the shared decision policy

$ bash tests/mx-teardown.test.sh
all teardown safety cases passed

$ git diff --check
(no output)

$ for test_script in tests/*.test.sh; do bash "$test_script"; done
ALL 71 TEST SCRIPTS PASSED
```
