# Cursor CLI adapter verification

This record contains dated, version-specific evidence for the verified Cursor harness.
Current operating knowledge is summarized in [the harness-adapters skill](../../.agents/skills/harness-adapters/SKILL.md), and launch mechanics remain in `bin/mx-launch-harness.sh` and `bin/mx-spawn.sh`.

## Environment

- Verification date: 2026-08-09.
- CLI: Cursor Agent `2026.08.04-aaa8809`.
- Host: Darwin arm64.
- Canonical executable: `~/.local/bin/agent`, linked into Cursor's versioned installation below `~/.local/share/cursor-agent/versions/2026.08.04-aaa8809/`.
- Authentication: `agent status` returned logged in; no account identifier is retained here.
- Subscription and models: the authenticated Free account exposed `auto` only, so a named-model launch failed rather than silently changing models.

The official installer was used after explicit maintainer authorization.
No test edited `~/.cursor/cli-config.json`, installed shell integration, changed persistent sandbox settings, or logged out the account.

## CLI surface

`agent --help`, `agent models`, and `agent about` established these supported surfaces on the pinned version:

- Interactive and `--print` modes, with `text`, `json`, and `stream-json` print output.
- `--resume [chatId]`, `--continue`, `--model`, `--sandbox enabled|disabled`, `--trust`, `--workspace`, `--add-dir`, and `--plugin-dir`.
- Blanket `--force` and `--yolo` modes and Cursor-owned `--worktree` mode.
- Parameterized model tokens such as `model[effort=high]`.

Multplx always emits `--sandbox enabled` and rejects force, yolo, sandbox-disabled, and Cursor-owned-worktree arguments before the real executable starts.
`agent` is canonical, while the `cursor-agent` shim reaches the same captured executable without recursion.

## Scratch live matrix

All live behavior ran in disposable Git repositories below `/tmp/mx-cursor-plan18-live`, with no remote-write credential and no configured remote.

| Behavior | Command or fixture | Observed result |
|---|---|---|
| Authentication and version | `agent status`; `agent about` | Logged in; exact CLI version and Darwin arm64 matched the record above. |
| Project rule | Tracked `.cursor/rules/multplx.mdc`; interactive exact-reply prompt | Cursor returned `MX_CURSOR_RULE_VERIFIED`. |
| Session start | Lower-camel `sessionStart` hook returning `additional_context` | Context reached the first model turn; removing the hook removed that injected instruction. |
| Command guard | Lower-camel fail-closed `preToolUse` hook | Allowed command ran; forbidden command was blocked before execution. |
| Sandbox write | `agent --print --sandbox enabled --trust` with one worktree write and one outside-home write | Worktree file was written; the outside-home target under the user directory was denied and absent. |
| Actor turn-end | Interactive `--plugin-dir` with a private stop plugin | Plugin stop hook fired and wrote its task-private marker. |
| Bounded follow-up | Interactive tracked stop hook with `followup_message` and `loop_limit: 1` | Exactly one follow-up ran; the second stop finished without another continuation. |
| Daemon-shaped turn | Interactive sandboxed turn with a valid `.mx-daemon-home` marker | Cursor returned `MX_CURSOR_DAEMON_TURN_OK`; the same one-follow-up stop bound remained active. |
| Busy and composer | Interactive processing and idle capture | Busy text was `Working` plus `ctrl+c to stop`; the idle composer used `→`. |
| Interrupt | Interactive turn asked to run a 20-second command; Ctrl-C during `Working` | The turn returned to the composer and never emitted its forbidden completion text. |
| Exit | Ctrl-D from an idle composer | Cursor exited zero and printed `agent --resume=<chat-id>`. |
| Resume | `agent --resume=f3840d4f-2cef-42f3-9536-7b45baf17aba --sandbox enabled --trust` | Prior prompts and replies loaded, the composer became idle, and Ctrl-D printed the same id. |
| Trust | Fresh scratch root with `--trust` | Launch proceeded without a dialog; Multplx made no persistent trust-config edit. |
| Print-mode stop negative | Same stop fixture under `--print` | Stop did not fire, so supervised lifecycle turns remain interactive. |

Cursor's documented `subagentStart` hook key was accepted in configuration but did not fire during the disposable live subagent attempt.
That dated result remains an explicit observation limit.
The current adapter allows native delegation through `preToolUse` and records `subagentStart` when Cursor supplies it. The observation remains session-bound even when Cursor exposes a child identifier, because this version has no verified matching result or resume event.
It does not claim durable child recovery from an event that did not fire.

## Supervision and reporting

### Event-driven stop park recheck, 2026-10-03

The installed interactive Cursor Agent `2026.10.01-e373342` passed the isolated stop-park probe using `--sandbox enabled --trust --model auto`.
No global hook or existing Multplx home was changed.
The fixture used the current Rust `mx-cursor-hook.sh stop` adapter and tracked stop registration, a fixture-only session-start owner, and controlled native watcher-arm output.
The exact replay command is `MX_CURSOR_PARK_LIVE=1 python3 tests/cursor-park-live-probe.py`.
The retained local evidence is `/var/folders/hc/g8p_srxs10z73zqvvxztzkq00000gn/T/mx-cursor-park-live-ra_bf3pl/result.json` and its `transcript.txt`.

- Generation ended before the hook parked on the arm child.
- Human input ran while the older park was pending, and the next stop published a new baton and retired the older capture.
- Two successive controlled watcher events each produced a real marked operational input, a `CURSOR_WAKE_HANDLED` model reply, and a new single parked successor.
- Away-mode entry retired the capture and tracked arm process group.
- The visible transcript contained replies and operational inputs without model checkpoint, manual-arm, or other tool calls.

The portable adapter regression separately passed latest-stop supersession, foreign or changed owner rejection, SIGTERM cleanup, three failure notices, an explicit ceiling notice and quiet idle scope.
Controlled arm output proves the real harness transport and adapter lifecycle; it does not claim a live child reporter, canonical wake acknowledgement, forge poller, or eight-hour timeout trial.
The callback ceiling and bounded failure path intentionally require human input or repair after their explicit notices.
The older August one-follow-up matrix above remains historical evidence for the replaced registration.

Tracked `sessionStart`, `preToolUse`, `subagentStart`, and `stop` commands route through `bin/mx-cursor-hook.sh`.
Before the 2026-10-03 repair, the Cursor protocol used bounded foreground checkpoints, and the primary stop adapter converted shared exit status 2 into one native follow-up only at `loop_count=0`.
The event-driven stop-park evidence above covers the replacement protocol and adapter.

Actor stop signaling uses a task-private plugin below `<recorded tasktmp>/cursor-turnend-plugin`, so project stop hooks do not collide with another actor's marker.
Cursor sessions receive `MX_TASK_ID` and the absolute `mx-report` fallback from the normal generated brief.
No per-run project-scoped MCP configuration contract was verified, so the adapter does not guess or mutate user MCP configuration.

## Explicit unsupported boundary

Cursor is not a deep-review adapter on this version.
Although print JSON output exists, no native JSON-schema constraint and no project-context suppression mode were verified together.
Using Cursor would therefore risk loading target rules or hooks into a gate session, so `dr_agent_oneshot` rejects it and deterministic validation never treats it as an available fallback.

## Automated coverage

`tests/mx-cursor-adapter.test.sh` covers launcher capture, alias safety, sandbox enforcement, refused blanket modes, hook translation, stop bounds, spawn profile axes, turn-end plugin isolation, composer detection, and busy detection.
`MX_CURSOR_LIVE_TESTS=1 tests/mx-cursor-live-e2e.test.sh` rechecks installed executable, authentication, version, and CLI surface without remote writes or user-config mutation.
The full Multplx suite keeps Claude, Codex, Pi, tmux, Herdr, and cmux default paths under their existing regression coverage.
