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

## Snapshot polling and cache

The page polls `/api/state` at `MX_VIZ_POLL_MS`, which defaults to 2500 milliseconds.
The server invokes `bin/mx-system-snapshot.sh --json` only on demand and coalesces concurrent refreshes.
`MX_VIZ_REFRESH_SECS`, which defaults to 2 seconds, bounds repeated snapshot work while clients are active.
Responses carry an ETag and content hash, and matching conditional requests receive `304 Not Modified` without a client rerender.
The canonical snapshot JSON is embedded byte-for-byte without a dashboard-side parser or reshaping pass.

The page uses a maintainer-to-broker-to-worker tree for live tasks and daemons, with broker health in the center, the structured backlog below it, and a sticky artifact browser beside it.
Worker cards are reconciled by task identity so unchanged workers remain mounted across snapshot polls.
Clicking the maintainer node opens a viewer for current decisions without exposing approve, defer, or write actions.
Displayed timestamps use the browser's local time zone, a 12-hour clock, and omit the year.

The snapshot script's header remains the schema owner for its additive watcher, queue, headroom, vplan-review, and later-plan feed fields.
Gate, workflow, and delivery panels appear only when their bounded record feeds contain records.
The gate panel includes current status, step, round, findings, risk, summary, pending decision, approved head, and step history when those fields are present.
Timeline drill-down invokes `bin/mx-timeline.sh --json` and presents typed event details with human-readable labels.
The doctor button invokes `bin/mx-doctor.sh --json` only after an explicit click.
Detail dialogs close from either the close button, Escape, or a backdrop click.

## Read-only and artifact boundary

The HTTP surface accepts only `GET`.
There are no spawn, nudge, stop, approve, drain, or other mutation endpoints.
The only filesystem mutation is the dashboard's private lifecycle record under `state/.viz/`.

Artifact browsing is limited to regular files whose canonical paths remain inside the repository's `data/` or `docs/` directory.
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
