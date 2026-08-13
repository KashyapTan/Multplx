# vplan review artifacts

vplan is Multplx's in-repo, one-shot HTML review surface.
The broker authors an ordinary HTML artifact, serves it only on loopback with an injected comment overlay, and the maintainer confirms a queue of comments.
Confirmation writes an inert JSON block into the artifact and ends the server.
There is no persistent daemon, polling protocol, remote hosting, or external runtime asset fetch.

[`vplan-authoring.md`](vplan-authoring.md) owns how to design the artifact.
This page owns the command lifecycle, port behavior, comment format, run-record contract, and operational safety boundary.

## Commands

Create a task-linked artifact from the vendored seed:

```sh
bin/mx-vplan.sh new data/<id>/plan.html
```

Start or rediscover its review URL:

```sh
bin/mx-vplan.sh review data/<id>/plan.html
```

Print the persisted comments as formatted JSON:

```sh
bin/mx-vplan.sh comments data/<id>/plan.html
```

End a live review without saving a new queue:

```sh
bin/mx-vplan.sh stop data/<id>/plan.html
```

The artifact must be inside the Multplx root.
`new` refuses to overwrite an existing file and rewrites only the seed's Mermaid path so the copied artifact continues to load the vendored renderer by a relative path when opened directly.
`review` returns an already-live identity-matched session's URL instead of starting a duplicate.
`comments` returns `[]` when the artifact has no persisted review block and refuses malformed or duplicate blocks.
The stable shell entry point selects the Rust `multplx-services` implementation before it reads or mutates review state.
The service is Rust-native and does not start Node.

## Server lifecycle and port selection

The server binds only `127.0.0.1`.
It tries `MX_VPLAN_PORT`, which defaults to `4870`, and then the next 19 ports in ascending order.
The default range is therefore `4870` through `4889`.
Failure to bind the whole range is a hard error that names the range and leaves no run record.

The printed review URL is `http://127.0.0.1:<bound-port>/`.
The server injects the comment SDK, its stylesheet, a per-review token, and the relative-asset base into the served bytes.
It does not write those injected tags to the artifact.
Static requests are limited to the artifact's directory and `share/vplan/` under the Multplx root.
The response policy blocks remote script, style, image, font, frame, and connection origins.

`MX_VPLAN_IDLE_SECS` controls the inactivity timeout and defaults to 1800 seconds.
It must be an integer from 1 through 86400.
Confirmation, `stop`, or the idle timeout closes the loopback server and removes its run record.

## Confirmation and atomic persistence

The browser sends one JSON object with a `comments` array to `POST /confirm`.
The request must carry the unguessable per-review token from the served page.
The server accepts at most 500 comments and at most 1 MiB of request data.
It rejects missing fields, unknown fields, duplicate IDs, invalid types, invalid timestamps, oversize values, malformed existing JSON, and duplicate comment blocks without changing the file.

On success, the server reads the current artifact, merges comments by ID, serializes one canonical block, writes a same-directory temporary file, syncs it, and renames it over the artifact.
All bytes outside the inserted or replaced comment block remain unchanged.
An existing `"resolved": true` value can never be downgraded by a later confirm.
Reusing an existing ID with different persisted content is rejected as a collision.
The JSON serializer escapes `<` so comment text cannot terminate the inert script block.

The server then responds with the saved and total comment counts, removes its matching run record, closes every review connection, and exits.
The artifact is the feedback channel, so no agent poll or session reconstruction is required.

## Comment block contract

One block appears immediately before `</body>`:

```html
<script type="application/json" id="vplan-comments">
[
  {
    "id": "c-example",
    "selector": "#delivery-plan > table > tbody > tr:nth-child(3)",
    "anchor_text": "push service owns PR-open",
    "nearest_heading": "Delivery",
    "comment": "Split approval from PR-open.",
    "ts": "2026-07-29T18:00:00.000Z",
    "resolved": false
  }
]
</script>
```

Every object has exactly these fields.

| Field | Contract |
| --- | --- |
| `id` | Non-empty stable identity, unique in the artifact |
| `selector` | Non-empty CSS selector for the owning element |
| `anchor_text` | Selected text or a bounded element-text snippet |
| `nearest_heading` | Nearest section heading, or an empty string |
| `comment` | Non-empty maintainer feedback |
| `ts` | ISO-8601 timestamp |
| `resolved` | Boolean review-history state |

The selector, anchor text, and nearest heading form the location fallback.
The SDK uses the selector first, highlights matching anchor text when it still exists, and falls back to an element pin when the text range has drifted.
Resolved comments remain in the block and render dimmed on later rounds.
The broker marks an addressed comment by changing only its `resolved` value to `true`.

## Run-record contract

Each artifact has one private record at `state/.vplan/<sha256-of-canonical-artifact-path>.run`.
The CLI publishes the record atomically with mode `0600` after the server has bound successfully.
The record contains one `key=value` field per line:

```text
version=1
artifact=/canonical/path/to/data/<id>/plan.html
port=4870
pid=12345
pid_identity=<portable process identity>
token=<per-review random token>
started_at=2026-07-29T18:00:00Z
```

`artifact` is the canonical identity and `port` is the actual bound loopback port.
`pid` alone is never enough to authorize a signal.
`pid_identity` uses the same portable process identity primitive as watcher lifecycle code so PID reuse cannot target an unrelated process.
`token` binds server cleanup to the exact review generation and also authenticates confirmation.
`started_at` supports inspection and the plan-13 orphan-server check.

The server removes a record only when its token and PID still match.
`stop` sends `TERM` only when the live PID's current identity matches the recorded identity.
A dead, malformed, or identity-mismatched record is stale and is removed without signaling any process.

## Operational boundary

The presence of a live run record marks the artifact as under review.
The broker must not edit the file until confirmation, `stop`, or idle timeout ends that review.
Atomic replacement prevents partial-file corruption, while this no-edit rule prevents comments from being attached to content the maintainer did not review.

Ending a vplan review completes no task or decision by itself.
Every unresolved maintainer decision found in the artifact or its comments must follow `decision-hold-lifecycle` before the originating review or investigation is treated as complete.

## Vendored assets

`share/vplan/manifest.json` records the Mermaid version, npm source archive, npm integrity, and SHA-256 of `share/vplan/mermaid.min.js`.
`bin/mx-vplan.sh --self-check` verifies the Rust service boundary, required assets, template reference, and pinned Mermaid hash without launching a server.
Session-start bootstrap runs that self-check and reports `VPLAN_INVALID` when the bundled module is incomplete or corrupt.
