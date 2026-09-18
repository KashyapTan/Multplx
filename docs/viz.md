# Live system dashboard

`mx-viz` is a disposable, maintainer-facing web view over Multplx's canonical system snapshot.
It is a read-only convenience surface rather than a source of truth or a control plane.
Agents continue to use the snapshot, catchup projection, and owning lifecycle commands instead of consulting the dashboard.

## Commands

Start or rediscover the dashboard for the current Multplx home:

```sh
bin/mx-viz.sh serve
```

The command prints only the loopback URL and never opens a browser.

Inspect the live process and its last snapshot poll:

```sh
bin/mx-viz.sh status
```

Stop the dashboard:

```sh
bin/mx-viz.sh stop
```

`serve` is singleton and idempotent per `MX_HOME`.
The Rust local-service command help and `multplx-services::local_services::viz` own the exact command, environment, and validation contract.
The stable shell entry point selects the Rust `multplx-services` implementation before it reads or mutates lifecycle state.
The service is Rust-native and does not start Node.

## Lifecycle and ports

The server binds only `127.0.0.1`.
It tries `MX_VIZ_PORT`, which defaults to `4890`, and then the next 19 ports in ascending order.
The default range is therefore `4890` through `4909` and does not overlap vplan's range.
Failure to bind the whole range is a hard error and leaves no run record.

`MX_VIZ_IDLE_SECS` controls inactivity shutdown and defaults to 1800 seconds.
Every request resets the timer, while a forgotten page eventually lets the server remove its record and exit.
The server keeps no authoritative state, so terminating it loses nothing.
HTTP framing, headers, bodies, child-command output, concurrent connections, and child-command runtimes are bounded by the Rust service.
`MX_VIZ_COMMAND_TIMEOUT_MS` sets the snapshot, doctor, and timeline reader deadline and defaults to 10000 milliseconds.
This outer deadline leaves room for the collector's own per-provider deadlines to publish a bounded partial snapshot when one provider stalls.

## Snapshot polling and cache

The page polls `/api/state` at `MX_VIZ_POLL_MS`, which defaults to 2500 milliseconds.
It schedules the next request only after the prior request settles, so client polls never overlap.
Hidden tabs wait at least 15 seconds between requests and resume promptly when shown.
The server invokes `bin/mx-system-snapshot.sh --json` only on demand and permits one refresh at a time across every viewer.
`MX_VIZ_REFRESH_SECS`, which defaults to 2 seconds, bounds repeated snapshot work while clients are active.
No service mutex is held while a reader command runs.
The first request performs one bounded refresh; concurrent requests report an explicitly unavailable initial cache instead of launching more readers.
After a successful refresh, an expired last-good snapshot is served immediately while one background refresh runs, and remains available if that refresh fails.
Failed refreshes respect the refresh interval before retrying.

Responses carry `ETag`, `X-Multplx-Content-Hash`, `X-Multplx-Snapshot-Hash`, `X-Multplx-Observation-Age-Ms`, `X-Multplx-Cache`, and `X-Multplx-Refresh`.
The ETag and content hash omit collection timestamps and freshness ages while retaining identified event, observation, decision, and delivery timestamps, so age-only refreshes receive `304 Not Modified` without hiding a meaningful state transition.
The observation-age header continues to advance on a `304` response.
`X-Multplx-Cache` is `fresh`, `stale`, or `unavailable`; `X-Multplx-Refresh` is `idle`, `in-flight`, or `failed`.
Failed state responses also carry a bounded, single-line `X-Multplx-Refresh-Error` value so a client can explain why its last-good snapshot remains stale.
`/api/meta` exposes refresh attempts, successes, failures, cache hits, stale serves, initial unavailability, conditional responses, and last/maximum refresh latency for verification.
The canonical snapshot JSON is embedded byte-for-byte without a dashboard-side parser or reshaping pass.

The page uses the canonical `mx-portfolio.v1` task projection as its overview.
Tasks are grouped by stable project identity and ordered by numeric priority, while task, session and attempt counts remain distinct.
Search and project, state, role and priority filters operate on that projection.
Expanding a task shows its owner, current and prior attempts, sessions, workflow stage, dependencies, nested task lineage, revision-bound decisions, allocation observation, evidence and delivery.
Domain cards use the canonical domain projection and show coordinator identity, parent-channel delivery backlog and partial observation.
Task deep links use the home-qualified task key, so same-named tasks from different homes remain distinct.
Focus, expanded rows and filter values survive meaningful snapshot updates.
Up and Down move between task summaries, Enter or Space expands the focused task, `/` focuses search, and Escape clears an active search.
Narrow layouts collapse detail and side panels to one column without hiding role or freshness labels.
Loading, empty, unavailable, partial, stale and refresh-error states are shown explicitly.
Decision and delivery items link to their task without exposing approve, defer, merge or write actions.
Displayed timestamps use the browser's local time zone, a 12-hour clock, and omit the year.

The snapshot script's header remains the schema owner for its additive watcher, queue, headroom, vplan-review, and later-plan feed fields.
The bounded timeline endpoint remains available to sanctioned read-only clients; the task portfolio uses the canonical latest-change and evidence fields rather than making a second timeline its overview authority.
The doctor button invokes `bin/mx-doctor.sh --json` only after an explicit click.
Detail dialogs close from either the close button, Escape, or a backdrop click.

The compatibility reader can display a legacy system snapshot while an older home is being upgraded.
That view does not infer attempt, workflow, allocation or domain facts that the legacy record does not contain.
Phase 10 publishes the shared project and task projection for later workspace clients; the terminal workspace UI remains Phase 11 scope.

`tests/viz-browser-check.mjs` is the optional responsive browser acceptance runner.
It uses a caller-supplied temporary Playwright module, running fixture-backed server URL, fixture file and output directory, so browser tooling and screenshots do not become packaged dependencies.
It captures 0, 1, 5, 10 and 20-task views at 1440 by 900 and 390 by 844, checks overflow and layout, verifies focus and expansion across a meaningful update, exercises keyboard movement and records 20 post-data interaction samples.
It also delays browser state requests to prove they do not overlap, injects `document.hidden` to verify the scheduled 15-second backoff, checks that observation age advances on a `304`, and makes a cached refresh fail to verify the stale error banner.
When `MX_VIZ_EXPECT_ARTIFACT_TEXT` is set, the runner first opens the source fixture's accepted brief through its exact artifact-reference route and verifies a scriptless Markdown preview before running the scale fixtures.
The recorded reference result is `plans/lean_redesign/phase10-ui-browser-results.json`.

A complete isolated run, including the temporary browser dependency and accepted archived brief, is:

```sh
run_root=$(mktemp -d /tmp/mx-viz-browser.XXXXXX)
tests/fixtures/viz/browser-home.sh "$run_root/home"
home=$(cd "$run_root/home" && pwd -P)
npm install --prefix "$run_root/playwright" playwright@1.63.0
"$run_root/playwright/node_modules/.bin/playwright" install chromium
url=$(MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" MX_DATA_OVERRIDE="$home/data" \
  MX_CONFIG_OVERRIDE="$home/config" MX_PROJECTS_OVERRIDE="$home/projects" \
  MX_VIZ_SNAPSHOT_BIN="$home/snapshot-reader.sh" MX_VIZ_FIXTURE="$home/snapshot.json" \
  MX_VIZ_REFRESH_SECS=2 MX_VIZ_POLL_MS=2500 bin/mx-viz.sh serve)
MX_PLAYWRIGHT_MODULE="$run_root/playwright/node_modules/playwright/index.mjs" \
  MX_VIZ_FIXTURE_FILE="$home/snapshot.json" MX_VIZ_BROWSER_URL="$url" \
  MX_VIZ_BROWSER_OUTPUT="$run_root/evidence" \
  MX_VIZ_EXPECT_ARTIFACT_TEXT='Archived acceptance marker: phase10-browser-brief' \
  node tests/viz-browser-check.mjs
MX_HOME="$home" bin/mx-viz.sh stop
```

## Read-only and artifact boundary

The HTTP surface accepts only `GET`.
There are no spawn, nudge, stop, approve, drain, or other mutation endpoints.
The only filesystem mutation is the dashboard's private lifecycle record under `state/.viz/`.

Artifact browsing is limited to regular files whose canonical paths remain inside the repository's `data/` or `docs/` directory, the configured home's `data/` or `state/brief-revisions/` directory, or those same directories in a currently validated direct or nested coordinator home.
Portfolio brief, research and report pointers receive opaque, exact-file browser routes only when the task's owning home and canonical file pass those containment checks.
The response envelope preserves each artifact's original snapshot `source_path` and supplies its opaque `url` so the client can enrich the matching canonical pointer without changing the shared projection; the server retains the validated canonical path internally.
Nested coordinator homes receive routes only when the canonical projection publishes `coordinator.validated_home`; invalid, unknown and external owner paths remain visible as snapshot pointers and never become file-serving routes.
Traversal segments, malformed escapes, absolute-path attempts, missing files, and symlinks that escape an allowed root are refused.
Artifact links open in a near-fullscreen in-page viewer.
Markdown is rendered into safe DOM nodes, while HTML is loaded in a scriptless sandbox with a deny-by-default content policy that permits same-origin assets, data images and fonts, and authored styles.
Static dashboard assets are vendored under `share/viz/` and make no external network requests.

## Run-record contract

The singleton record is `state/.viz/server.run` with mode `0600`.
It contains the canonical home and state paths, bound port, PID, portable PID identity, private cleanup token, and start timestamp.
The exact key set and serialization are owned by `multplx-services::local_services::viz`; `bin/mx-viz.sh` is a transport-only adapter.

`stop` signals a process only when its current portable identity matches the record.
A dead or identity-mismatched record is removed without signaling the recorded PID.
The server removes a record during shutdown only when both its token and PID match, so an older generation cannot erase a replacement record.
`bin/mx-doctor.sh` includes this location in its orphan-server invariant check.
