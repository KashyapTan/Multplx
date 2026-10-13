# Active dashboard verification

The October 2026 repair addresses completed work mixed with active assignments, machine IDs and full worker prompts as titles, competing status badges, stale ready-PR receipts, and missing ordinary worker descendants.
The source observations were human screenshots and supplied transcripts; private task IDs, paths and transcript contents are not reproduced here.
All retained browser examples use synthetic portfolios, including an owned canonical fixture read by the real Rust service.

## Presentation contract

Tasks and Agents default to active work, with completed or closed assignments in initially collapsed History.
Uncertain observations have their own initially collapsed Needs reconciliation group and visible count.
Unanswered human questions, blocking dependencies and actionable current PR reviews remain active even when implementation is complete.
The ownership graph retains completed ancestors as context when they have active descendants.
A worker's done report says Awaiting parent verification; implementation completion and a matched human-merged PR are separate evidence facts.
Agent availability, observation quality, session references, paths, attempts and workflow mechanics live in assignment details.
The root card shows only session availability, with dashboard cache and observation diagnostics in a disclosure.
Dashboard update age describes the viewer's collection, without asserting that every agent is fresh.

Human Attention remains visible on both tabs.
A question names the requested answer and the main orchestrator chat as its reply destination.
A PR card exposes its actual repository and PR-number link immediately.
Descriptions and complete legacy task text remain available in bounded disclosures, rendered as literal text.
Project headings, collapsed rows and selectors display project names or concise checkout basenames. Duplicate names use parent-folder labels and stable numbering; full checkout paths are accessible only in assignment details.

Forge observations supersede stored ready receipts.
Merged and closed observations require an exact published-head match before moving the current task revision into history.
A failed or missing bounded forge observation suppresses an unverified review action and retains the task for reconciliation.
Reopening a current ready PR restores the review action; merging a PR never clears an unrelated human question.
The dashboard is GET-only and does not alter task state, publication receipts or operational homes.

## Browser evidence

![Tasks, desktop](../images/viz-active-dashboard/dashboard-desktop.png)
![Agents, desktop](../images/viz-active-dashboard/agents-desktop.png)
![Tasks, mobile](../images/viz-active-dashboard/dashboard-mobile.png)
![Agents, mobile](../images/viz-active-dashboard/agents-mobile.png)

`tests/viz-dashboard-browser-check.mjs` serves only repository UI assets on an ephemeral loopback port and intercepts state requests with synthetic fixtures.
It never starts an operational home, authenticates a provider or contacts a real forge.
The Chromium desktop (1440 by 900) and mobile (390 by 844) cases verify seven active assignments, two historical assignments and two uncertain assignments, independent of retained session count.
They cover search and state filters, initial disclosures, keyboard skip navigation, visible PR links, question routing, long and sparse titles, nested branch expansion, terminal parent context, exact historical task opening, programmatic merged/closed/reopened PR refresh, agent/task state mismatch, final descendant scroll reachability and absence of horizontal page overflow.
The retained [browser result](viz-active-dashboard-browser-results.json) records these deterministic results.

Run the isolated browser check with an existing Playwright installation:

```sh
MX_PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs \
MX_VIZ_BROWSER_OUTPUT=/tmp/mx-viz-dashboard-browser \
node tests/viz-dashboard-browser-check.mjs
```

The ordinary `tests/mx-viz.test.sh` gate runs the real client selectors, attention renderer and graph normalization tests without an installed browser.
The additional graph fixture verifies two levels of registered standing-worker homes and rejects an unvalidated parent route.
The selector fixture verifies that an old merged head or stale forge result cannot complete a newer delivery revision.
Canonical question authority requires the accepted brief revision; superseded and parent-status-only questions remain historical.
Provider turn end and completed validation remain separate from worker delivery reports.
The existing `tests/viz-browser-check.mjs` remains the service-backed scale, polling and artifact acceptance runner.

`tests/viz-dashboard-service-browser-check.mjs` additionally creates an owned canonical fixture home and standing worker child, invokes the actual release Rust service and system-snapshot collector, and uses a temporary read-only forge command fixture.
It verifies the emitted observation and completion schema, actual merged-PR history, current canonical questions, retained worker reports, exact descendant lineage and two observed native child sessions on desktop and mobile.
The retained [actual-service result](viz-active-dashboard-service-results.json) records both layouts.

![Actual service Tasks, desktop](../images/viz-active-dashboard/service-dashboard-desktop.png)
![Actual service Agents, desktop](../images/viz-active-dashboard/service-agents-desktop.png)
![Actual service Tasks, mobile](../images/viz-active-dashboard/service-dashboard-mobile.png)
![Actual service Agents, mobile](../images/viz-active-dashboard/service-agents-mobile.png)

The native graph distinguishes observed sessions from assignments and marks activity unknown; retained and opaque session evidence does not create extra accepted tasks.
The service-backed scale runner also verifies that repeated failed refreshes remain visible through retry-start responses, including conditional `304` responses.
A service unit regression proves that only a successful refresh clears the retained error.
The failure banner stays visible on both tabs, stale root observations say so, and task labels identify the last known state during an outage.
Nested assignment disclosures, focus and graph navigation survive polling without undoing a newer user interaction.

## Validation boundary

The evidence above proves fixture-based browser behavior, not a live private portfolio or provider session.
Programmatic forge state and nested-home reading are separately covered by the backend's deterministic domain and service tests.
No private runtime migration, installation refresh, operational task mutation or PR merge is included in this repair.

## Preliminary failures retained

A preliminary focused Viz run returned HTTP 503 while loading its synthetic nested-home fixture during concurrent compilation.
The error body was not retained, so its cause remains unproven; an unchanged traced rerun and the later combined focused run passed with the original two-second fixture command budget.
A separate repeatable browser failure exposed a service defect: a retry-start response discarded the previous refresh error while serving the last good snapshot.
The repair preserves that error through the in-flight retry instead of weakening the browser failure-banner assertion.
Final broad gates run in a coordinated host window to avoid interpreting contention between independent full suites and coverage runs as a product timing diagnosis.

## Final gate progress

At source commit `d0983fd`, formatting, the release workspace build, all five focused Viz/snapshot/forge/reconciliation scripts, the synthetic desktop/mobile browser runner, the actual Rust-service browser runner, and the existing service-backed scale/polling/artifact runner passed. The scale runner measured 20 render samples (p95 20.258 ms).
A subsequent frontend-only correction removed absolute paths from missing-project-name fallbacks. The selector regressions and both desktop/mobile dashboard browser runners passed again, including an actual-service assertion that full checkout paths appear only in expanded technical details.
The final local full run was deliberately interrupted (exit 130) on that review finding; its 12 started scripts included five interruptions, so it does not establish a final suite result. Final complete behavior lanes and the unchanged 93% Rust line-coverage gate are provided by exact-head remote CI; no equivalent expensive local gate is claimed.

The earlier broad diagnostic run counted 134 scripts, two failures and eight gate skips. Its landed-bounds failure was fixed by rejecting duplicate legacy status-event questions in canonical catchup. Its local Herdr 0.9.3 presentation negative control could not reproduce the old focus-steal bug; the unchanged CI lane uses the required 0.7.4 pin. Both failures remain recorded rather than skipped or relaxed.

A read-only cross-PR applicability check against wake/bootstrap source `7879ed7` found one adjacent documentation-inventory insertion conflict. Moving the Viz entry beside existing verification documents resolved it; `git merge-tree --write-tree` then completed cleanly (synthetic tree `2957c7d0`). The PRs share no changed Rust or JavaScript source files. This is source integration evidence, without a remote merge or a combined runtime validation claim.
