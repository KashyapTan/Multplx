# Runtime backend verification

Audience: maintainer verification.

This record contains reusable version-scoped evidence for active runtime guarantees.
The backend guides own current setup, safety boundaries, and limitations.
Exact task chronology, branch names, temporary homes, local paths, process ids, thread ids, and delivery transcripts remain in private reports or PR evidence.

## Viz and vplan Rust services

Rust-port Portion 12 verification ran on 2026-08-12 with `mx 0.1.0` on macOS 26.5.2 arm64.
The stable `bin/mx-viz.sh` and `bin/mx-vplan.sh` entry points use the Rust `multplx-services` boundary before lifecycle state access.

```sh
cargo build --workspace --release --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo llvm-cov --workspace --all-targets \
  --ignore-filename-regex 'herdr_(cleanup|presentation|tools)\.rs' \
  --fail-under-lines 93
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-viz.test.sh
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-vplan.test.sh
bin/mx-test-run.sh --check-coverage
```

The focused Cargo integration suite covered both service lifecycles, route and method boundaries, cache and conditional requests, token and content-type refusal, port exhaustion, stale-record recovery, identity-bound stop, artifact containment, encoded traversal, symlink escape, output and timeout bounds, review round trips, and every atomic publication fault seam.
The workspace coverage gate passed at 93.02 percent lines without excluding the new service code.
The complete portable behavior run passed all 115 selected scripts in 342.743 seconds, with nine declared environment-gated skips and no failure.
The unfiltered 125-script run passed every portable script; its ten real-Herdr scripts could not provision their required isolated Herdr lab on this host and were the only failures.

The in-app browser loaded the Rust dashboard from a foreground release service and rendered the maintainer-to-broker tree, system state, structured backlog, and read-only artifact panel with no console warning or error.
It then loaded the unchanged Portion 12 artifact through the Rust vplan service, observed exactly one injected base and one injected SDK script, rendered the review panel and zero-comment confirm action, and reported no console warning or error.
No confirmation was submitted, so the reviewed artifact was not mutated.

The frozen browser assets retained these SHA-256 digests:

```text
9cfbdce58b58444e3e90bf2325d90f5b0eff7fdc0d96a41f68d61233d6c9ef2c  share/viz/app.css
3ea2a9b8f4aec0c1171c1a4d3833e1d2172583e3bb769d9203b0f94f4d06712e  share/viz/app.js
cd54d12672e19fce1bf4500500f2e8af43bd532027eb459913a7689c0e1a17ea  share/viz/index.html
ca7d639537dd0d650c61151385b045b392aa5ebca3654770c755f21b501d02d5  share/vplan/manifest.json
07fb9c98a9718885cb4b68c29bdfdbd1e96bc6e731f5387cdc70ce8aadd4b2a6  share/vplan/mermaid.min.js
222e335a33a9f8e411277eff5146035f837feee2237d389efeeccf42ad8dfd06  share/vplan/sdk.css
9969d8ebeb2847efe2422d167c8b82898da026f3a9c96b11f464f5ecc7abefe9  share/vplan/sdk.js
c5f91527fda4355a2543393c855b047f0f0f8a0b04c48119dbc93f9c281d72b9  share/vplan/template.html
```

Release-path focused-suite timing used the same release binary and local APFS workspace with all safety checks enabled.
`tests/mx-viz.test.sh` took 3.62 seconds through legacy and 3.14 seconds through Rust, a 13.3 percent reduction.
`tests/mx-vplan.test.sh` took 3.78 seconds through legacy and 2.65 seconds through Rust, a 29.9 percent reduction.
No request, identity, path, token, timeout, publication, or cleanup bound was disabled.

## Recorded-backend state regression

The native release entry points were verified on 2026-09-10 with Rust 1.97.1 on macOS arm64 using isolated homes and fake tmux, Herdr, and cmux transports.
The source copy and fixture temporary directory were siblings, and validation children had actor routing removed.

```sh
target/release/mx test-run --jobs 2 tests/mx-actor-state.test.sh tests/mx-doctor.test.sh tests/mx-launcher.test.sh
```

```text
ok - recorded Herdr/cmux actor-state, doctor and snapshot paths preserve live, absent and unreadable observations
ok - clean fixture reports every check OK and exits zero
ok - native installer honors explicit root and home from a non-repository directory
MX_TEST_SUMMARY total=3 failed=0 skipped_gate=0 duration_ms=44679
```

The state fixtures cover backend selection from metadata, daemon liveness projection, unsupported metadata, missing endpoints, transport failures, and malformed cmux inventories.
The doctor fixture prevents Git discovery from escaping its synthetic non-repository root into the surrounding development checkout.
These deterministic checks do not constitute a new live Herdr or cmux verification; the version-scoped live evidence below remains separate.

Passive observation and replacement cmux surface checks were verified on 2026-09-10 with the same isolated fake transports and Rust 1.97.1:

```sh
target/release/mx test-run --jobs 2 tests/mx-actor-state.test.sh tests/mx-headroom.test.sh
```

```text
MX_TEST_SUMMARY total=2 failed=0 skipped_gate=0 duration_ms=4195
```

The Herdr fixture rejects server starts and covers stopped-server diagnostics, capture fallback, daemon liveness, and snapshot capacity reads.
The cmux fixture replaces the recorded surface and requires actor-state, doctor, and snapshot to resolve the same task workspace.

## tmux

### Confirmed upgrade stopping

Upgrade stopping was verified on 2026-10-09 with tmux 3.7c and Rust 1.97.1 on macOS arm64.

```sh
target/release/mx test-run tests/mx-release-package.test.sh tests/mx-source-install.test.sh
```

The acceptance fixture uses a private tmux socket, empty configuration and inert processes, without operational homes or user sessions.
It verifies one aggregated confirmation, No and EOF preservation, exact window stopping, recycled-label identity refusal, failed-stop refusal, child stale-launch reconciliation and receipt-only retired executions.
Original metadata, launch history, home leases, dirty files and an unrelated sentinel window survive.
Real process probes also verify registered-primary stopping and exact launch-reservation reconciliation while retaining connection history.
The tmux proof uses printable field separators because `LC_ALL=C` converts control separators to underscores.

The same installer suite separately uses a response-shaped Herdr mock to verify stable terminal identity, `pane process-info` shell ownership, private execution-home scope and allocation-null research records.
Those deterministic checks do not claim a live Herdr stop; the [upgrade guide](../getting-started.md#upgrade) documents the compare-and-close limitation.

### Rust Portion 04 shadow-period evidence

The Rust shadow adapter was verified on 2026-08-11 with tmux 3.7b on macOS 26.5.2 arm64.
Legacy remained the default during this verification.
The native path is selected once per command and uses no fallback after backend execution begins.

```sh
cargo test --locked -p multplx-backend -p multplx-cli
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-backend.test.sh
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-backend-tmux-smoke.test.sh
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-actor-state.test.sh
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-composer-ghost.test.sh
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-tmux-submit-busy.test.sh
MX_RUST_BIN="$PWD/target/release/mx" tests/mx-daemon-liveness.test.sh
```

The real tmux smoke created a stable window id, pinned its name, verified target readiness and the requested current path, sent literal and submitted text, captured bounded history, resolved a bare name from exact inventory, killed the task window, and classified its absence authoritatively.
The differential Rust test exercised every hidden facade command against an argument-recording fake and compared exact `mx-peek.sh` and `mx-actor-state.sh` status, stdout, stderr, and tmux argument observations with legacy.
The timeout test spawned a descendant process and verified that the bounded runner killed the owned process group without leaving the descendant alive.
The Linux portable-serial CI lane now repeats the same focused suite against the production Rust tmux implementation.

Release-path timing used the same local APFS workspace, one real tmux session, one task metadata fixture, 100 warm interleaved iterations per implementation, Perl `Time::HiRes`, and nearest-rank p95.

| Command | Legacy median | Rust median | Legacy p95 | Rust p95 |
| --- | ---: | ---: | ---: | ---: |
| `mx-peek.sh perf 4` | 35.039 ms | 35.541 ms | 38.813 ms | 37.792 ms |
| `mx-actor-state.sh perf` | 44.972 ms | 31.359 ms | 47.089 ms | 32.297 ms |

The Rust release path improved p95 for both commands and improved actor-state median by 13.613 ms.
Peek median added 0.502 ms, or 1.4 percent, while its p95 improved by 1.021 ms; that startup-scale tradeoff retains typed selector validation, bounded output, process-group cleanup, and no-fallback execution.
No safety bound was disabled for the comparison.

Foreground-process behavior was verified on 2026-07-07 with tmux 3.6a on macOS.

```sh
tmux new-session -d -s fmtest -n testwin
tmux display-message -p -t fmtest:testwin '#{pane_current_command}'
tmux send-keys -t fmtest:testwin 'sleep 30' Enter
tmux display-message -p -t fmtest:testwin '#{pane_current_command}'
tmux send-keys -t fmtest:testwin C-c
tmux display-message -p -t fmtest:testwin '#{pane_current_command}'
```

Observed output:

```text
zsh
sleep
zsh
```

A persistent parent shell waiting for a child remained reported as the parent process, while a shell that directly execed a simple command changed identity with the process itself.
Claude and Codex were observed under their own process names.
Pi remained a generic `node` process and is intentionally inconclusive.

The busy-queue behavior and the tmux fallback are pinned by:

```sh
tests/mx-tmux-submit-busy.test.sh
```

Expected matrix: pending plus busy is accepted as queued; pending plus idle remains pending; a cleared composer succeeds in either state.

## Herdr

### Explicit unknown registration compatibility (2026-10-09)

Isolated real-Herdr diagnostics on installed `herdr 0.9.3` returned a successful `agent_info` response for an inert Codex fixture with `agent_status: "unknown"`, a nonempty agent name and the exact requested `pane_id`.
The corresponding pane response remained valid; the registered fixture had not produced activity that Herdr could classify as idle or working.
The baseline adapter rejected that registration as an unreadable endpoint during command-submission checks, reproducing the presentation and workspace-per-home fixture failures before this compatibility correction.
The adapter now accepts only that exact successful registration shape as present while retaining unknown activity and an ambiguous agent-liveness projection; mismatched identity, absent registration fields, unexpected status values and malformed responses remain unreadable.
The separate spawn process-start proof remains required and unchanged.
This diagnostic used an isolated named lab, an inert provider executable and the default-session safety tripwire; it does not qualify authenticated Codex execution.
Deterministic regression coverage verifies endpoint readiness, registered-agent presence, unknown native activity and rejection of mismatched or malformed registration.


The compatibility floor is protocol 14.
The latest active verification uses Herdr 0.7.5 protocol 16 on macOS aarch64, with earlier 0.7.4, protocol-14, and 0.7.3 evidence retained where they define current behavior or fallbacks.

On 2026-10-03, Herdr 0.7.4 on macOS arm64 was tested through guarded named labs with an inert Python harness and a synthetic long PATH.
An 18,024-byte direct launch submission returned transport success but left unfinished shell input and no inert startup receipt; a short owned script invocation executed.
The changed integrated spawn submitted an 85-byte script path, matched an exact task/attempt/brief/endpoint/nonce receipt to its live harness PID, and only then recorded Running.
A missing executable retained waiting state and its endpoint; the same request was refused as uncertain on retry, while an already successful request converged through the existing admission receipt.
The lab exercised a synthetic native Codex SessionStart hook on the exact launched PID and rejected a nested provider process inheriting the task environment.
These are inert transport, process-start and hook-schema fixtures, not paid-provider authentication, model acceptance, live native hook trust or resumability verification.
Every provisioned lab was removed through the guarded lifecycle helper and its default-session tripwire passed.
The maintained [audit follow-up verification](transcript-audit-followups.md) records the related scope and checks.

The Portion 05 Rust-selected required family was reverified on 2026-08-11 with Herdr 0.7.4 protocol 16 on macOS aarch64.
The command below records the current direct native family invocation.
The run used the release `mx` binary, a short isolated `XDG_CONFIG_HOME`, a PID-owned temporary default server, and the guarded lab and CI cleanup tools.
No real-Herdr test reported the `herdr not found` gate skip, and the pre-suite snapshot plus post-suite teardown found no unowned session eligible for cleanup.

```sh
cargo build --workspace --release --locked
MX_RUST_BIN="$PWD/target/release/mx" \
bin/mx-test-run.sh --family real-herdr-gated \
  --fail-on-gate-skip 'herdr not found'
```

Observed Rust and client/server evidence:

```text
mx 0.1.0
herdr 0.7.4
client protocol 16
server protocol 16
MX_TEST_SUMMARY total=10 failed=0 skipped_gate=0 duration_ms=332732
MX_TEST_SUMMARY_FAMILY family=real-herdr-gated count=10 duration_ms=332418 failed=0
```

Core read-only probes:

```sh
herdr --version
herdr status --json | jq -c '{client:.client.protocol,server:.server.protocol}'
herdr api schema --json | jq -c '.schemas.subscription_event["$defs"].SubscriptionEventKind.enum'
```

Observed current shapes:

```text
herdr 0.7.5
{"client":16,"server":16}
["pane.output_matched","pane.agent_status_changed","pane.scroll_changed"]
```

The CLI matrix was checked directly:

| Guarantee | Command shape | Result |
| --- | --- | --- |
| Explicit session routing | `herdr <verb> ... --session <name>` | Reached the named session even while another server was running. |
| Literal send | `herdr pane send-text <pane> <text> --session <name>` | Left text unsubmitted until Enter. |
| Keys | `herdr pane send-keys <pane> enter|escape|ctrl+c --session <name>` | Enter and Escape worked; Ctrl-C interrupted foreground work. |
| Capture | `herdr pane read <pane> --source recent --lines N` | Small N could return empty below viewport height; a 200-line request plus local trim was stable. |
| Native state | `herdr agent get <pane>` | Working and done transitions were visible; long foreground tool waits required rendered-busy corroboration. |
| Restart | guarded named-session stop then start | Workspace, tab, pane, and labels persisted; the agent process and registration did not. |
| Close | `herdr pane close <pane> --session <name>` | The exact one-pane task tab closed; closing a final tab could remove the workspace. |

All destructive verification used `bin/mx-herdr-lab.sh` with a non-default `mx-lab-` name and a byte-identical default-session tripwire.
No ambient `herdr server stop` command is a supported test operation.

### Prune and respawn

The real label-collision reproduction is owned by:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-backend-herdr-prune-safety-e2e.test.sh
```

Observed guarantee: a pre-existing maintainer-owned workspace with a seed-shaped tab was adopted for routing but its tab was never eligible for prune because the current create call did not return that seed id.

Restart-husk replacement is owned by:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-backend-herdr-respawn-idem-e2e.test.sh
```

Observed guarantee: a restored no-agent tab was replaced create-before-close, while a registered live agent caused refusal.

### Per-home and presentation topology

Per-home behavior is owned by:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-backend-herdr-workspace-per-home-e2e.test.sh
```

Observed guarantee: the primary and daemon used distinct home workspaces, a child launched by the daemon stayed in that daemon workspace, list-live remained home-scoped, and exact cleanup did not affect sibling homes.

The standing-agent workspace naming change was verified on 2026-10-03 against the installed Herdr 0.7.4 client in guarded, non-default lab sessions:

```sh
target/release/mx test-run tests/mx-backend-herdr-smoke.test.sh tests/mx-backend-herdr-workspace-per-home-e2e.test.sh
```

Both scripts passed without gate skips (`total=2 failed=0 skipped_gate=0 duration_ms=14497`).
The real workspace list showed `agent-<id>` for a newly created standing-agent home.
An independently created historical `daemon-<id>` workspace was adopted with the same workspace id, label, focus, and empty seed-pruning authority, with a byte-identical before/after workspace list.
The per-home spawn, child routing, list-live isolation, exact cleanup, and named-session restart checks passed with the default-session tripwire intact.
These tests use synthetic task harnesses and native pane registration; they do not establish new live provider credential or busy-state evidence.
Focused Rust fixtures additionally cover current and historical label lookup, mixed-prefix presentation ordering, and concise child labels.

The complete isolated presentation suite also passed on 2026-10-03 against Herdr 0.7.4 (`total=1 failed=0 skipped_gate=0 duration_ms=192701`):

```sh
target/release/mx test-run tests/mx-backend-herdr-presentation-e2e.test.sh
```

The suite retained its historical `daemon-alpha` and `daemon-bravo` parent labels and verified all three concurrent create/order/cleanup waves, cross-home recovery, exact focus restoration, and unmigrated legacy spaces.
Its command logger now appends each complete TSV record in one write so concurrent read-only calls cannot corrupt the serialized creation-order evidence.
An eight-process, 640-record local logger stress check reproduced 629 malformed records with the old argument-at-a-time logger and zero with the complete-record logger.
This fixes the test evidence path; the exact workspace-order and focus assertions remain unchanged.

The complete projection suite ran on 2026-07-21 against Herdr 0.7.4 protocol 16:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-backend-herdr-presentation-e2e.test.sh
```

Observed guarantees included:

```text
ok - real Herdr lab: primary and two daemon homes each own a top-level contiguous child block
ok - real Herdr lab: concurrent primary/A/B spawns stay session-locked with zero focus drift
ok - real Herdr lab: session lock contention from a daemon home falls back flat with no journal
ok - real Herdr lab: legacy projection labels and flat daemon tabs are left unmigrated
ok - real Herdr lab: multi-home exact-pane teardowns restore maintainer focus without workspace close authority
ok - real Herdr lab validation completed on Herdr 0.7.4 with the default-session tripwire intact
```

The suite also covers lost or failed move responses, active-tab refusal, restart husks, missing and duplicate tokens, manual renames, concurrent cleanup, and exact focus restoration.

The mandatory projection suite ran again on 2026-07-24 against Herdr 0.7.5 protocol 16:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-backend-herdr-presentation-e2e.test.sh
```

Observed restart-reclaim guarantees:

```text
ok - real Herdr lab: Hi Bit and Wheelhouse-style same-identity restarts reclaim one nested space with exact focus and idempotence
ok - real Herdr lab: daemon restart binding and reclaim stay isolated to the exact child home and parent
ok - real Herdr lab: concurrent cross-home recoveries replace exact husks under one session lock with no focus drift
ok - real Herdr lab: missing, renamed, and duplicate tokens trigger zero destructive or adoptive calls, and live duplicate risk refuses launch
ok - real Herdr lab validation completed on Herdr 0.7.5 with the default-session tripwire intact
```

The restored-shell session-start cleanup ran on 2026-07-24 against Herdr 0.7.5 protocol 17:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-herdr-session-cleanup-e2e.test.sh
```

Observed guarantee: one exact home-local, journal-correlated, one-tab and one-pane childless idle shell was closed after restoration while the exact non-target focus and default system session remained unchanged, and a repeat run was a no-op.

### Composer and operational input

Real captures verified these active distinctions:

- Claude and Codex use bare `❯` and `›` agent composers.
- Pi uses content between complete separator rows and requires exact native Pi identity.
- Dim or faint suggestion text is ghost content, while normally styled text is pending input.
- Dark truecolor placeholders are ghost content, while bright truecolor typed input remains pending.
- A bare shell prompt has no safe agent-composer container and is unknown.

`tests/mx-composer-ghost.test.sh`, `tests/mx-composer-lib.test.sh`, and the Herdr composer cases pin the exact captured ANSI bytes.
The U+2063 operational and routed-request separators were exercised through a real Pi-on-Herdr path; the byte-exact active regression is:

```sh
MX_SEND_MARKER_HERDR_E2E=1 \
  tests/mx-send-daemon-marker-herdr-e2e.test.sh
```

### Native blocked event

The protocol-16 event path was measured on 2026-07-11 with Herdr 0.7.3 and Python 3.13:

```sh
HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-backend-herdr-eventwait-smoke.test.sh
```

Observed output:

```text
ok - real herdr: events.subscribe capability gate passes
ok - real herdr: a driven idle->blocked transition returns the blocked record in 0.129s
ok - real herdr: the watcher fast-path enqueues a stale wake naming the task window
```

Polling remained active and is covered as the fallback for capability, connect, subscribe, and repeated reader failure.

### Away-mode transport

The Pi/Herdr return and injection path was reverified on Herdr 0.7.3 and Pi 0.80.7:

```sh
MX_AFK_PI_HERDR_E2E=1 HERDR_LAB_HELPER=bin/mx-herdr-lab.sh \
  tests/mx-afk-pi-herdr-return-e2e.test.sh
```

Observed guarantees: pending composer input refused injection and raised one alert; idle Pi accepted one marked escalation; the return gate refused ordinary work while a live blocker remained; resolving the blocker allowed the return flow.
The dedicated Herdr daemon workspace topology is covered by `tests/mx-afk-launch.test.sh` and preserves the maintainer tab's pane count.

## cmux

### Rust Portion 06 default adapter

The cmux fake-CLI contract was reverified on 2026-08-11 against both implementations during the bounded shadow period.
The suite covered the 0.64 minimum, missing and stale clients, fresh password reads, authentication classification, no-launch auth refusals, scoped identity, collision refusal, stale-target recovery, marker-delimited cwd, bounded capture, composer and submit behavior, window membership, last-workspace cleanup, best-effort kill, and home-filtered inventory.

```sh
cargo build --workspace --release --locked
target/release/mx test-run tests/mx-backend-cmux.test.sh
target/release/mx test-run tests/mx-backend-cmux-smoke.test.sh
```

The deterministic suite passed under both implementations.
The real smoke explicitly reported `skip: cmux CLI not found on PATH or at the bundle path` in this verification environment, so Portion 06 makes no new live-cmux claim and retains the earlier version-scoped live evidence below.
The Rust-default cross-backend run also passed the real tmux contract; the local real-Herdr autodetect smoke stopped at its pre-existing default-session isolation tripwire, while the completed Portion 05 required Herdr family above remains the active Rust evidence.

The current compatibility floor is cmux 0.64, and the active live evidence uses 0.64.17 build 97 on macOS aarch64.
Real tests use only exact `mx-test-` workspaces guarded by `tests/cmux-test-safety.sh` and never quit or relaunch the maintainer's app.

```sh
cmux version
cmux ping
```

Observed version:

```text
cmux 0.64.17 (97) [9ed29d81a]
```

Source and live checks established the five control modes:

- `off` starts no listener.
- `cmuxOnly` rejects an external Multplx process by ancestry.
- `automation` uses an owner-only 0600 socket with no handshake.
- `password` uses the same 0600 socket plus `auth <password>`.
- `allowAll` uses a 0666 socket with no authentication.

The live default rejection was `Access denied - only processes started inside cmux can connect`.
The live password challenge was `Authentication required - send auth <password> first`.
The app configuration writer did not retain a hand-added socket password, which is why the operator guide requires Settings and a local Multplx password source.

Current active CLI findings:

| Guarantee | Command shape | Result |
| --- | --- | --- |
| Create | `new-workspace --name <title> --cwd <dir> --focus false --id-format uuids` | Created one workspace with one surface without focusing it. |
| Fresh readiness | `list-panes --workspace <id> --json --id-format uuids` | Found a brand-new surface before content existed. |
| Fresh read counterexample | `read-screen` before any write | Returned `internal_error: Failed to read terminal text`. |
| Literal send | `send --workspace <id> --surface <id> -- <text>` | Left text unsubmitted. |
| Keys | `send-key ... enter|escape|ctrl-c` | All shared key operations worked. |
| Nested cwd | `current_directory` plus foreground subshell | Structured cwd froze; the marker-delimited `pwd` probe found the live cwd. |
| Last surface | `close-surface` on the only surface | Refused with `invalid_state: Cannot close the last surface`. |
| Last workspace | `close-workspace` on the only workspace in a window | Printed success but left the workspace present. |

The last-workspace workaround was reverified on 2026-07-10 in Automation mode.
After creating one unfocused unnamed sibling in the same window, `close-workspace` removed the exact task workspace and left only cmux's default sibling.
A selected non-last workspace closed directly, proving that window cardinality rather than selection is the trigger.

Source inspection confirmed each workspace constructor creates a new UUID with no restored-id input.
Recovery therefore remains title-based.
The bundled Claude wrapper was observed stripping `CMUX_*` variables on its failed socket-probe path while retaining the app bundle id, supporting the macOS-only bundle-id and ancestry fallbacks.

```sh
tests/mx-backend-cmux.test.sh
tests/mx-backend-cmux-smoke.test.sh
```

The real smoke proves socket access, fresh readiness, current-path probing, send and keys, bounded capture, title identity, and guarded exact cleanup.

## Harness and dispatch cutover

The Rust-default harness launch and dispatch layer was verified on 2026-08-11.
Codex and Cursor were present and executed their real version commands through the validated child-root launcher during the shadow-period comparison.
Claude and Pi were not installed in this verification environment, so their new empirical launch checks are explicitly blocked here rather than inferred; the retained adapter evidence in `harness-adapters` remains the version-scoped source for those two adapters.

```sh
MX_LAUNCHER_LIVE_E2E=1 tests/mx-launcher-live-e2e.test.sh
tests/mx-launcher.test.sh
tests/mx-headroom.test.sh
tests/mx-dispatch-queue.test.sh
```

Observed real versions:

```text
codex-cli 0.147.0-alpha.6.5
Cursor CLI 2026.08.04-aaa8809
Treehouse v2.0.1 with get --lease (historical pre-Phase-03 measurement)
Claude unavailable
Pi unavailable
```

The focused contracts covered environment-first and bounded-ancestry detection, actor and daemon fallback tokens, literal launcher argv and environment, child-only cwd, lock refusal, recursive-shim refusal, Cursor sandbox refusal, malformed candidate refusal, candidate deduplication independent of array order, configured global and per-harness budgets, queue contention, FIFO, at-limit persistence, cancellation, failed-launch recovery, private record modes, and ignored unpublished temporary records.
The pinned Treehouse Rust module additionally exercised the four supported platform assets, wrong-platform refusal, exact checksum acceptance and mismatch, stale-version refusal, missing-lease refusal, and the installed real `v2.0.1` lease surface.

The complete portable repository manifest passed with the Rust release binary and the Plan 06 defaults active.

```sh
MX_RUST_BIN="$PWD/target/release/mx" \
  bin/mx-test-run.sh --all --exclude-family real-herdr-gated --jobs auto
```

Observed summary:

```text
MX_TEST_SUMMARY total=115 failed=0 skipped_gate=9 duration_ms=340048
```

The nine declared skips were optional live-tool or opt-in harness gates.
The separate ten-test real-Herdr family was attempted without mutation and stopped at its system-state tripwire because the host did not have exactly one running default session.
That current environmental refusal does not replace or weaken the completed Portion 05 Rust evidence recorded above.

Release-path timing used 100 warm interleaved iterations with deterministic inputs on the same local checkout.

| Command | Legacy median | Rust median | Legacy p95 | Rust p95 |
| --- | ---: | ---: | ---: | ---: |
| `mx-harness.sh actor` | 11.411 ms | 13.031 ms | 14.202 ms | 15.865 ms |
| `mx-headroom.sh --json` | 24.942 ms | 11.522 ms | 29.773 ms | 14.454 ms |

Harness resolution adds 1.620 ms median and 1.663 ms p95 for typed parsing and process startup.
Headroom improves median by 13.420 ms and p95 by 15.319 ms while adding locked queue mutation and strict malformed-profile refusal.

## Codex App host tools

A reusable Desktop host-tool smoke ran on 2026-07-06 against Codex Desktop bundle version 26.623.101652, build 4674, bundle id `com.openai.codex`.
Local paths and task-specific ids are intentionally not retained here.

The host-tool sequence was:

1. list a saved project;
2. create a Desktop-owned worktree thread;
3. recover and read the thread while active and after completion;
4. verify the thread appended a Multplx status line and wrote its report;
5. send a follow-up to the same thread;
6. read the completed follow-up;
7. archive the exact thread;
8. read the archived transcript with state `notLoaded`.

Observed guarantee: a Desktop-owned thread can write Multplx lifecycle files when the prompt provides an authorized absolute path, and create, send, read, and archive work at the Desktop host-tool layer.
The missing guarantee remains a supported shell-callable bridge that lets Multplx perform those operations against the same visible Desktop endpoint.
App-server partial methods and raw socket experiments do not satisfy that bridge contract.

## Lean Phase 03 supersession

The installer and external worktree dependency described in earlier measurements above are retired.
The [Phase 03 implementation evidence](../../plans/lean_redesign/phase03-implementation.md) records replacement checks and their current validation status.

## Codex shared hook consent verification - 2026-10-08

Real Codex CLI `0.160.1` was exercised through an isolated temporary `CODEX_HOME`, synthetic API credential and temporary runtime copies; no model request, user session, private operational home or global installation was used.
The primary launch argument capture came from the real Rust `launch-harness` command with a synthetic provider executable.
The actual native Codex startup UI then displayed four new shared hooks and persisted approval through its native review flow.
Native `hooks/list` reported the same four session-flag keys and hashes as trusted at the primary directory and two different worker directories.
A second native TUI reached the worker directory's idle composer without shared-hook review.
These observations establish native trust reuse, not worker assignment acceptance, model execution or backend readiness.

Disabling PreToolUse through native `/hooks` remained disabled at all three directories.
An unrelated hook file in a separately native-trusted fixture project retained its own untrusted project-source hook; the worker TUI asked to review exactly that one new hook, and it was left unapproved.
Changing the owned observer script bytes changed every shared native hash to modified; changing the selected hook executable bytes did the same.
Restoring the original bundle restored trusted status and preserved the native disabled choice.
No trust-store writes, hook-trust bypass flags, fabricated managed policy or automatic product consent were added.
All owned test TUI and app-server processes were stopped.

Deterministic Rust fixtures separately verify exact primary/worker hook argument equality for byte-identical public `multplx` and packaged `mx` executable copies without inherited routing variables, genuine override separation, script/executable fingerprint invalidation, CLI override collision diagnostics, and one observer/merge invocation per event when owned project and CLI sources coexist.
The tracked primary idle and session-start handlers continue through their existing primary-scope predicate: ordinary task workers are excluded, while marked standing homes retain their own supervision.
The exact-argument worker fixture uses a synthetic cmux endpoint and inert readiness adapter; the launcher terminal suite uses a real isolated tmux server with a synthetic provider.
Neither fixture claims a paid worker turn or provider authentication.

Claude and Pi executables were unavailable for this verification, so their adapter flags and extension loading were not changed; launch/setup diagnostics preserve their native project or extension consent and foreground readiness fallback.
Installed Cursor `2026.10.01-e373342` help/source confirmed that its existing worker `--trust` is broader persistent workspace permission; it was not expanded to primary launches, and no new authenticated Cursor trial is claimed.
Historical worker hook files, fresh Multplx worktree project hooks and unrelated user hooks retain independent native review; shared CLI approval does not transfer those approvals.
