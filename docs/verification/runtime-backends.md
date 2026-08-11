# Runtime backend verification

Audience: maintainer verification.

This record contains reusable version-scoped evidence for active runtime guarantees.
The backend guides own current setup, safety boundaries, and limitations.
Exact task chronology, branch names, temporary homes, local paths, process ids, thread ids, and delivery transcripts remain in private reports or PR evidence.

## tmux

### Rust Portion 04 shadow adapter

The Rust shadow adapter was verified on 2026-08-11 with tmux 3.7b on macOS 26.5.2 arm64.
Legacy remained the default during this verification.
The Rust path was selected once per command with `MX_BACKEND_IMPLEMENTATION=rust` and used no fallback after backend execution began.

```sh
cargo test --locked -p multplx-backend -p multplx-cli
MX_BACKEND_IMPLEMENTATION=rust MX_RUST_BIN="$PWD/target/release/mx" tests/mx-backend.test.sh
MX_BACKEND_IMPLEMENTATION=rust MX_RUST_BIN="$PWD/target/release/mx" tests/mx-backend-tmux-smoke.test.sh
MX_BACKEND_IMPLEMENTATION=rust MX_RUST_BIN="$PWD/target/release/mx" tests/mx-actor-state.test.sh
MX_BACKEND_IMPLEMENTATION=rust MX_RUST_BIN="$PWD/target/release/mx" tests/mx-composer-ghost.test.sh
MX_BACKEND_IMPLEMENTATION=rust MX_RUST_BIN="$PWD/target/release/mx" tests/mx-tmux-submit-busy.test.sh
MX_BACKEND_IMPLEMENTATION=rust MX_RUST_BIN="$PWD/target/release/mx" tests/mx-daemon-liveness.test.sh
```

The real tmux smoke created a stable window id, pinned its name, verified target readiness and the requested current path, sent literal and submitted text, captured bounded history, resolved a bare name from exact inventory, killed the task window, and classified its absence authoritatively.
The differential Rust test exercised every hidden facade command against an argument-recording fake and compared exact `mx-peek.sh` and `mx-actor-state.sh` status, stdout, stderr, and tmux argument observations with legacy.
The timeout test spawned a descendant process and verified that the bounded runner killed the owned process group without leaving the descendant alive.
The Linux portable-serial CI lane is configured to repeat the focused Rust shadow suite against its real tmux installation.

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

The compatibility floor is protocol 14.
The latest active verification uses Herdr 0.7.5 protocol 16 on macOS aarch64, with earlier 0.7.4, protocol-14, and 0.7.3 evidence retained where they define current behavior or fallbacks.

The Portion 05 Rust-selected required family was reverified on 2026-08-11 with Herdr 0.7.4 protocol 16 on macOS aarch64.
The run used `MX_BACKEND_IMPLEMENTATION=rust`, `MX_HERDR_TOOLS_IMPLEMENTATION=rust`, the release `mx` binary, a short isolated `XDG_CONFIG_HOME`, a PID-owned temporary default server, and the guarded lab and CI cleanup tools.
No real-Herdr test reported the `herdr not found` gate skip, and the pre-suite snapshot plus post-suite teardown found no unowned session eligible for cleanup.

```sh
cargo build --workspace --release --locked
MX_BACKEND_IMPLEMENTATION=rust \
MX_HERDR_TOOLS_IMPLEMENTATION=rust \
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
