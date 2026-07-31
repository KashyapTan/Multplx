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
The script header owns the exact command, environment, and validation contract.

## Lifecycle and ports

The server binds only `127.0.0.1`.
It tries `MX_VIZ_PORT`, which defaults to `4890`, and then the next 19 ports in ascending order.
The default range is therefore `4890` through `4909` and does not overlap vplan's range.
Failure to bind the whole range is a hard error and leaves no run record.

`MX_VIZ_IDLE_SECS` controls inactivity shutdown and defaults to 1800 seconds.
Every request resets the timer, while a forgotten page eventually lets the server remove its record and exit.
The server keeps no authoritative state, so terminating it loses nothing.

## Snapshot polling and cache

The page polls `/api/state` at `MX_VIZ_POLL_MS`, which defaults to 2500 milliseconds.
The server invokes `bin/mx-system-snapshot.sh --json` only on demand and coalesces concurrent refreshes.
`MX_VIZ_REFRESH_SECS`, which defaults to 2 seconds, bounds repeated snapshot work while clients are active.
Responses carry an ETag and content hash, and matching conditional requests receive `304 Not Modified` without a client rerender.
The canonical snapshot JSON is embedded byte-for-byte without a dashboard-side parser or reshaping pass.

The snapshot script's header remains the schema owner for its additive watcher, queue, headroom, vplan-review, and later-plan feed fields.
Gate, workflow, and delivery panels appear only when their bounded record feeds contain records.
Timeline drill-down invokes `bin/mx-timeline.sh --json`, and the doctor button invokes `bin/mx-doctor.sh --json` only after an explicit click.

## Read-only and artifact boundary

The HTTP surface accepts only `GET`.
There are no spawn, nudge, stop, approve, drain, or other mutation endpoints.
The only filesystem mutation is the dashboard's private lifecycle record under `state/.viz/`.

Artifact browsing is limited to regular files whose canonical paths remain inside the repository's `data/`, `plans/`, or `docs/` directory.
Traversal segments, malformed escapes, absolute-path attempts, missing files, and symlinks that escape an allowed root are refused.
Static dashboard assets are vendored under `share/viz/` and make no external network requests.

## Run-record contract

The singleton record is `state/.viz/server.run` with mode `0600`.
It contains the canonical home and state paths, bound port, PID, portable PID identity, private cleanup token, and start timestamp.
The exact key set and serialization are owned by the `bin/mx-viz.sh` header.

`stop` signals a process only when its current portable identity matches the record.
A dead or identity-mismatched record is removed without signaling the recorded PID.
The server removes a record during shutdown only when both its token and PID match, so an older generation cannot erase a replacement record.
`bin/mx-doctor.sh` includes this location in its orphan-server invariant check.
