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
The adapter therefore does not rely on that event alone: fail-closed `preToolUse` applies the shared delegation-shape guard before the subagent tool invocation, and deterministic tests cover both translations.

## Supervision and reporting

`docs/supervision-protocols/cursor.md` uses bounded foreground checkpoints, matching the verified Codex control shape.
Tracked `sessionStart`, `preToolUse`, `subagentStart`, and `stop` commands route through `bin/mx-cursor-hook.sh`.
The primary stop adapter converts shared exit status 2 into one native follow-up only at `loop_count=0`.

Actor stop signaling uses a task-private plugin below `/tmp/mx-<id>/cursor-turnend-plugin`, so project stop hooks do not collide with another actor's marker.
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
