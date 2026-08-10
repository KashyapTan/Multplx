---
name: harness-adapters
description: Agent-only reference for broker harness operations. Use before spawning or recovering an actor or daemon, handling a trust dialog, sending a harness-specific skill invocation, interrupting or exiting an agent, resuming an exited agent, or verifying a new harness adapter. Contains verified facts for claude, codex, cursor, and pi.
user-invocable: false
metadata:
  internal: true
---

# harness-adapters

Use this reference before any harness-specific broker operation: spawn, recovery, trust-dialog handling, skill invocation, interrupt, exit, resume, or adapter verification.

Actors default to the same harness broker is running on unless `config/actor-harness` records an adapter name.
Optional dispatch profiles in `config/actor-dispatch.json` can override that static default for one actor or scout dispatch by selecting concrete harness, model, and effort axes at intake.
The maintainer may override that file at session start or later; a per-task instruction such as "run this one on codex" overrides it for that dispatch only.
`default` means mirror broker's own harness.

Daemons have their own harness knob, so a daemon can run on a different adapter than actors.
`config/daemon-harness` is the harness the primary uses to launch DAEMON agents, resolved through the fallback chain `config/daemon-harness` -> `config/actor-harness` -> broker's own.
An absent or `default` `config/daemon-harness` therefore behaves exactly as the actor harness did before this knob existed (daemons launched on the actor harness); setting it splits the two.
The [`daemon-provisioning` skill](../daemon-provisioning/SKILL.md) owns the complete inherited-local-material allowlist and propagation contract.
This skill owns only the harness-relevant consequence: a daemon's own actors use the primary's inherited dispatch profiles and static harness value, while `config/daemon-harness` is the primary's own setting and is never inherited - daemons do not spawn daemons.
Inheritance copies the literal `config/actor-harness` file, so for a daemon's own actors to run on the primary's actor harness the maintainer must set `config/actor-harness` to a concrete adapter name, such as `codex`.
If `config/actor-harness` is unset or `default`, there is no concrete value to inherit, so the daemon's own actors fall back to the daemon's own/detected harness rather than the primary's effective actor harness.
Inheritance also copies the literal `config/actor-dispatch.json` file, so daemons apply the same best-fit profile rules for their own actors.

Each adapter splits into mechanics and knowledge.
The per-task mechanics, including launch command, autonomy flag, and actor turn-end hook, live in `bin/mx-spawn.sh`.
The primary-session "no turn ends blind" guard contract and harness hook installation paths live in `docs/turnend-guard.md`.
The primary-session watcher wake protocols are rendered from `docs/supervision-protocols/` by `bin/mx-supervision-instructions.sh`.
The supervision knowledge lives here: busy signature, exit command, interrupt, dialogs, resume behavior, skill invocation, and quirks.

Never dispatch an actor or daemon on an unverified adapter.
If `config/actor-harness` or `config/daemon-harness` names an unverified adapter, tell the maintainer under `AGENTS-PORTING.md` section 9 during the Rust port that the requested worker runtime is not verified yet, use broker's own verified runtime for current work, and ask only whether to verify the requested runtime before future use.
Do not pause current work for that future-verification choice, and never launch an unverified adapter.
If the maintainer asks for a new harness, propose verifying it first: spawn a trivial supervised task using `mx-spawn`'s raw-launch-command escape hatch, confirm every fact empirically, then record the mechanics in `mx-spawn`, the busy signature in `mx-watch.sh` and `mx-tmux-lib.sh` defaults, any needed `MX_COMPOSER_IDLE_RE` empty-composer override plus any novel bare agent prompt glyph in `bin/mx-composer-lib.sh`'s shared composer classifier (the one system-wide owner of the empty/dead-shell/pending decision, so a new harness's own idle composer is not misread as a dead shell), the tmux agent-process liveness classification in `bin/backends/tmux.sh` when the harness can launch a daemon, and the verified knowledge here.

## Detection

`bin/mx-harness.sh` prints broker's own harness, using verified env markers first and then process ancestry.
`bin/mx-harness.sh actor` resolves the effective actor harness from `config/actor-harness` (absent or `default` -> own).
`bin/mx-harness.sh daemon` resolves the daemon-launch harness through the chain `config/daemon-harness` -> `config/actor-harness` -> own, so an unset `config/daemon-harness` matches the actor harness.
`bin/mx-spawn.sh` uses `actors` mode for an actor/scout launch and `daemon` mode for a `--daemon` launch, re-resolving on every spawn so the split is durable across respawns; an explicit per-spawn harness arg overrides either.
On `unknown`, ask the maintainer instead of guessing.
A maintainer override always beats detection.
When verifying a new adapter, record its env marker and command name in `bin/mx-harness.sh`.

For stuck recovery, the target window's harness is recorded as `harness=` in `state/<id>.meta`.
Use that value for interrupt, exit, resume, and skill-invocation facts.

## Primary turn-end guard

Every verified primary harness has an empirically validated hook path for the "no turn ends blind" guard.
`claude` and `codex` block directly through Stop hooks that preserve exit status 2 and stderr from `bin/mx-turnend-guard.sh`.
`cursor` translates that same status-2 result into one native `followup_message` and caps the stop hook at `loop_limit: 1`.
`pi` exposes a passive lifecycle callback for this purpose, so its tracked primary adapter forces one bounded follow-up when the shared predicate blocks.
The exact hook files, commands, scoping rules, and fail-open tradeoffs are owned by `docs/turnend-guard.md`.
`docs/verification/supervision.md` "Turn-end guard" owns active validation evidence.
When changing any primary turn-end hook, validate the real harness behavior in a scratch project or throwaway home before trusting it, then update that doc and the relevant concise fact below.

## Primary pre-arm (PreToolUse) seatbelt

Every verified primary harness also has a wired PreToolUse-equivalent hook that denies a watcher-arm anti-pattern (shell `&`, truncating pipe, bundling, broad `pkill -f mx-watch`) before it runs.
`claude`, `codex`, and `cursor` block directly through PreToolUse hooks.
`pi` blocks by returning `{block: true}` from `tool_call`.
The exact hook files, commands, output-shaping quirks (Claude Code only honors the deny when stdout is empty), and validation transcripts are owned by `docs/arm-pretool-check.md`.
When changing any watcher-arm PreToolUse hook, validate the real harness behavior in a scratch project before trusting it, then update that doc.
## Primary delegation-shape guard

Claude exposes built-in delegation, scheduling, and worktree tools that a primary session can use to create work with no `state/<id>.meta`, which makes the whole guard stack inert because every guard counts that metadata.
The delivered mechanism is `bin/mx-subagent-pretool-check.sh`, a primary-home PreToolUse guard that denies a delegation-SHAPED tool name.
Claude primaries should also use an untracked per-home local `permissions.deny` list as hardening for known Claude delegation tools, because it removes them from the model's schema so they are never offered.
That deny list must not land in tracked `.claude/settings.json` because it is Claude-only rather than harness-agnostic, and because tracked project settings propagate into linked worktrees where they disarm legitimate actors.
`docs/subagent-guard.md` owns the full contract, the local deny-list recommendation, the `MX_ALLOW_SUBAGENT=1` escape hatch, and the per-harness applicability review.

Two verified facts worth pinning here.
The subagent tool presents to the model as `Agent`, and on Claude Code 2.1.217 both `Agent` and `Task` work as `permissions.deny` keys, verified by an A/B with a nonsense-name control.
`permissions.allow` is a pre-approval list rather than an availability list, so there is no fail-closed positive allowlist.

## Primary session-start nudge

`AGENTS-PORTING.md` section 3 remains the behavioral owner for session start during the Rust port, while tracked native adapters invoke `bin/mx-sessionstart-nudge.sh` as an idempotent enforcement layer after final restoration.
The wrapper prints one canonically typed `session-start` instruction to run `bin/mx-session-start.sh`; it never runs the digest, wake drain, bootstrap sweeps, lock, or supervision arm itself.
Full mechanics, scoping, and fail-open behavior live in `docs/sessionstart-nudge.md`.
`docs/verification/supervision.md` "Native session-start delivery" owns active dated commands, payloads, and evidence.

- `claude`: verified native `SessionStart` stdout injection; `.claude/settings.json` matches `startup`, `resume`, and `clear`, but not `compact`.
- `codex`: verified on 0.144.4; `.codex/hooks.json` receives `source=startup`, and wrapper stdout reaches model context.
- `cursor`: verified on 2026.08.04-aaa8809; tracked `sessionStart` uses `additional_context` from the shared nudge wrapper.
- `pi`: verified native `session_start`; the existing primary extension handles `startup`, `new`, and `resume` and uses `pi.sendMessage` to inject context without racing a positional launch prompt.

## Primary watcher supervision

At session start, `bin/mx-session-start.sh` prints exactly one watcher supervision block for the detected primary harness.
Do not substitute another harness's wait shape when resuming supervision.
Claude's Stop `asyncRewake` hook (`bin/mx-claude-stop-autoarm.sh`) owns tokenless re-arm around `bin/mx-watch-arm.sh`.
Codex uses bounded foreground checkpoints through `bin/mx-watch-checkpoint.sh` because Codex cannot reason while a foreground tool call is running.
Cursor uses the same bounded foreground-checkpoint contract.
Pi uses the tracked `.pi/extensions/mx-primary-turnend-guard.ts` plus the tracked `.pi/extensions/mx-primary-pi-watch.ts`, both project-local extensions Pi auto-discovers once trusted.
When changing any primary watcher adapter, update `docs/supervision-protocols/`, `docs/turnend-guard.md` if a shared idle or turn-end hook changed, and the relevant concise fact below.

## Launch profile axes

`bin/mx-spawn.sh` accepts concrete `--harness`, `--model`, and `--effort` values chosen by broker at intake.
Do not make the shell scripts parse or match natural-language dispatch rules.

Effort precedence is an explicit per-task maintainer instruction first, then any applicable standing dispatch profile or daemon pin, then the generic fallback below.
Never replace an effort value supplied by either higher-precedence source.
Use the fallback only when neither the maintainer nor applicable standing configuration specifies effort.
Use `low` for well-understood work with an explicit bounded path and `xhigh` for ambiguous investigation or design.
Choose intermediate levels proportionally as complexity, uncertainty, blast radius, or open-ended reasoning increases.
When a verified adapter lacks `xhigh`, cap the choice at its highest supported non-`max` level rather than omitting the intended effort silently.
Never select `max` from this fallback; use it only when the maintainer has explicitly expressed that per-task or standing preference.

The supported launch-profile flags below are verified locally; each row records its evidence.

| Harness | Model flag | Effort flag | Notes |
|---|---|---|---|
| claude | `--model <model>` | `--effort <low\|medium\|high\|xhigh\|max>` | Verified on Claude Code 2.1.196. |
| codex | `--model <model>` | `-c 'model_reasoning_effort="<low\|medium\|high\|xhigh>"'` | Verified on codex-cli 0.142.1. The installed binary schema contains `model_reasoning_effort`, the active config uses it, and the bundled model catalog advertises only low/medium/high/xhigh. `max` is omitted. |
| cursor | `--model '<model>[effort=<level>]'` | Folded into the model token | Verified on Cursor CLI 2026.08.04-aaa8809. Availability remains account-specific; the authenticated free account exposed only `auto`. |
| pi | `--model <model>` | `--thinking <low\|medium\|high\|xhigh\|max>` | Verified 2026-07-13 on Pi 0.80.6. `pi --help` advertises `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`; `pi --print --model openai-codex/gpt-5.6-sol --thinking max 'Reply with exactly OK.'` completed successfully. |

### Model support discovery

Treat model and provider knowledge as current source-of-truth discovery, not as a permanent namespace or provider mapping.
Use the discovery surface in the current authenticated environment because supported and available models can change by version, account, and configuration.

| Harness | Authoritative discovery surface |
|---|---|
| claude | Open the current interactive session's `/model` picker; `claude --help` documents the accepted alias or full-model-name input shape. |
| codex | Open the current interactive session's `/model` picker. |
| cursor | Run `agent models`; model availability reflects the authenticated account. |
| pi | Run `pi --list-models [search]`; Pi's installed `docs/models.md` owns how built-in, extension-registered, and custom provider/model entries reach that list. |

For an unfamiliar harness or model namespace, establish support and provider identity from that harness's authoritative CLI help, model listing, or current documentation rather than guessing from a name or prefix.
If those sources do not establish the relationship needed for dispatch, fail loudly and report the unresolved candidate.

When a requested effort value is outside the harness-specific accepted set, `mx-spawn` records the requested `effort=` in meta but emits no effort flag for that harness.
This preserves launch success instead of passing a known-bad value.

## deep-review headless one-shot

`bin/mx-deep-review-lib.sh` owns the executable `dr_agent_oneshot` contract.
The orchestrator passes `--session new|<id>`, `--schema <file>`, `--prompt <file>`, `--output <file>`, and `--session-out <file>`.
Every child process receives `DEEP_REVIEW_GATE=1`, and the deterministic caller validates the resulting JSON again even when the harness natively enforces the schema.
Review assess and review fix always request fresh sessions, and the orchestrator refuses an identical returned session id.

When trusted `.deep-review.yaml` sets `disable_project_settings: true`, the adapter must keep repository-level identities, instructions, hooks, extensions, and settings out of the gate session.
The gate prompt remains authoritative and the lifecycle marker independently removes Multplx control capability.

- `claude`: use print mode with `--output-format json`, `--json-schema`, and a fresh `--session-id`; resume only when explicitly requested.
  The structured result is extracted from `structured_output` and validated again.
  This command path is fail-closed when the Claude CLI is unavailable.
- `codex`: use `codex exec --json --output-schema --output-last-message`.
  Parse the session id from the `thread.started` JSONL event.
  When project settings are disabled, launch from a fresh external directory with `--skip-git-repo-check`, add the task worktree through `--add-dir`, set `project_doc_max_bytes=0`, clear fallback instruction filenames, and pass `--ignore-rules`.
  This keeps automatic repository discovery outside the gate while the prompt directs work at the explicit isolated worktree.
  The noninteractive flags and output-schema behavior were checked against the installed CLI help and the current official Codex manual on 2026-07-30.
- `cursor`: unsupported for deep-review.
  Cursor 2026.08.04-aaa8809 has print and JSON output modes but no verified native schema constraint and no verified way to suppress tracked project rules and hooks while retaining explicit checkout access.
  `dr_agent_oneshot` therefore refuses Cursor instead of weakening gate isolation.
- `pi`: use `--print --approve --no-session --no-context-files --no-extensions`.
  Pi does not provide a verified native JSON-schema constraint or headless resume path, so deterministic validation is mandatory and a requested resume refuses.
  This command path is fail-closed when Pi is unavailable.

Tests set `MX_DEEP_REVIEW_AGENT` to a fake adapter implementing the same file interface.
That seam records prompts, schemas, sessions, and outputs without a real model or network call.

## Status reporting

Every spawned session receives `MX_TASK_ID` and `MX_REPORT_STATE_OVERRIDE`.
The first binds the writer to one task.
The second keeps a daemon's parent-facing status channel separate from the daemon home's own operational `MX_HOME`.
Actors and daemons use the `report_status` MCP tool when their verified harness wiring exposes it.
The absolute `bin/mx-report` path written into the generated brief is always the fallback.
Both paths append the same validated event grammar, and neither replaces current-state reconciliation.

- `claude`: verified on Claude Code 2.1.220.
  `claude mcp --help` identifies `.mcp.json` as project-scoped configuration and `claude --help` supports additional `--mcp-config` files.
  `mx-spawn.sh` writes a mode-`0600` task fragment under `/tmp/mx-<id>/report-mcp.json` and passes it with `--mcp-config`.
  Claude composes that fragment with existing user and project MCP configuration, so Multplx never edits or clobbers a project's committed `.mcp.json`.
- `codex`: verified on codex-cli 0.146.0-alpha.3.1 and the current Codex configuration reference.
  Codex supports trusted project `.codex/config.toml` files and `mcp_servers.<name>` configuration.
  `mx-spawn.sh` supplies the `multplx_status` stdio server as a per-run `-c` override, so existing project configuration remains untouched and the binding expires with the session.
- `cursor`: use `mx-report`.
  No project-scoped per-run MCP registration contract was verified, so Multplx leaves user configuration untouched.
- `pi`: use `mx-report`.
  No project-scoped MCP registration contract has been verified for Pi, so Multplx does not guess or mutate Pi's user configuration.

## claude (VERIFIED)

| Fact | Value |
|---|---|
| Busy-pane signature | `esc to interrupt` |
| Exit command | `/exit` |
| Interrupt | single Escape |
| Skill invocation | `/<skill>` |

First launch in a fresh worktree, or first ever on a machine, may show a trust or bypass-permissions confirmation.
After every spawn, peek the pane within about 20 seconds.
If such a dialog is showing, accept it from an active broker session using `MX_HOME=<this-broker-home> bin/mx-send.sh <window> --key Enter`, or the choice the dialog requires, unless `MX_HOME` is already set to the active Multplx home; verify the brief started processing.

Claude renders a predicted-next-prompt suggestion as dim/faint text inside an otherwise-empty composer after a turn completes.
A plain `tmux capture-pane` cannot tell that ghost text apart from typed text.
Multplx launches every claude actor and daemon with `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`, scoped to broker-launched agents through `bin/mx-spawn.sh`, so it never touches the maintainer's global config.
The CLI's `--prompt-suggestions` flag is print/SDK-mode only and does not suppress the interactive composer ghost text, verified empirically on v2.1.186.
As defense in depth for any pane that flag cannot reach, including the maintainer's own broker composer that away-mode reads, the shared `mx_composer_strip_ghost` extractor in `bin/mx-composer-lib.sh` removes dim/faint SGR 2 ghost runs before pending-input classification on both ANSI-capable readers (tmux and herdr).
Its broader dark-TRUECOLOR placeholder handling and dark-theme tradeoff are documented in `docs/herdr-backend.md` "Composer and injection safety", with active captures in `docs/verification/runtime-backends.md`.
That styled capture is internal to the boolean detector only.
`mx-peek` and every other human or LLM-facing capture path stays plain `tmux capture-pane` with no escape codes.

**Primary-session guard fact (verified 2026-07-04, Claude Code 2.1.201; preserved 2026-07-08, Claude Code 2.1.204; Stop-owned auto-arm revalidated 2026-07-24, Claude Code 2.1.219).**
This is separate from the per-task actor turn-end hook above (that one just `touch`es a marker file in a task's own `.claude/settings.local.json`).
The broker PRIMARY's own `.claude/settings.json` registers two Stop hooks: `bin/mx-turnend-guard.sh --claude` and the Stop-owned auto-arm `bin/mx-claude-stop-autoarm.sh` (`asyncRewake: true`, `timeout: 28800`), and exiting the guard with status 2 plus stderr reliably forces the model to continue.
Claude Code's stdin payload to a Stop hook carries a `stop_hook_active` boolean that is `true` when the current stop attempt follows ANY stop-hook-driven continuation, including `asyncRewake` rewakes; the primary guard therefore ignores it in `--claude` mode and uses the cooperative claim/epoch check plus a bounded re-block budget instead, while the codex-mode default still treats it as a one-block loop guard.
A project-level `.claude/settings.json` only takes effect when Claude Code's project root is that exact directory - it does not walk up from a subdirectory looking for one, so broker launches the primary from the repo root.
After those settings are loaded, hook command resolution is still cwd-sensitive because Claude Code runs commands through `/bin/sh` against the session's current cwd; keep the tracked commands anchored through `"$CLAUDE_PROJECT_DIR"/bin/...` and see `docs/turnend-guard.md` for the verified Stop-hook details.
Claude Code's primary watcher protocol is Stop-owned: the auto-arm hook fires on every Stop and foregrounds `bin/mx-watch-arm.sh` when the home is eligible and still needs supervision, and its exit-2 `asyncRewake` rewake is the wake; the model drains and handles wakes but never runs a routine re-arm command.

## codex (VERIFIED 2026-06-11, codex-cli 0.139.0)

| Fact | Value |
|---|---|
| Busy-pane signature | `esc to interrupt` (shown as `• Working (Xs • esc to interrupt)`) |
| Exit command | `/quit` (slash popup needs about 1 second between text and Enter; `mx-send` handles it) |
| Interrupt | single Escape |
| Skill invocation | `$<skill>`; `/<skill>` is claude-only and codex rejects it as "Unrecognized command" |

A `$<skill>` invocation opens a `$`-autocomplete (skill) popup, the same hazard as the `/` slash popup: submitting too fast lets the popup swallow the Enter, so the invocation never lands.
`mx-send` handles it the same way it handles `/` - it gives the popup a longer settle (1.2s) between typing and the first Enter, with the target backend's submit retry as the safety net - but the `$` settle is scoped to `harness=codex`, read from the target metadata for exact task ids or legacy `mx-<id>` labels.
That scope matters because, unlike `/`, a leading `$` commonly starts ordinary text (`$5/month`, `$HOME`), so a universal `$` rule would needlessly slow plain steers to claude/pi; only a codex target receiving a `$...` message gets the popup-settle.
An explicit `session:window` target has no meta, so its harness is unknown and treated as non-codex (the safe fast-path default).
This is why a `$<skill>` invocation to a Codex actor lands on the first Enter instead of biting the popup.

Directory trust dialog on first run per repo root: "Do you trust the contents of this directory?"
Accept with Enter.
The decision persists for the repo, so later worktrees of the same project skip it.

Resume after exit with `codex resume <session-id>`.
The session id is printed on quit.

**Primary-session guard fact (verified 2026-07-08, codex-cli 0.142.1).**
The broker PRIMARY's own `.codex/hooks.json` registers a Stop hook that pipes Codex's Stop payload to `bin/mx-turnend-guard.sh`.
Codex Stop hooks block on exit 2 and expose `stop_hook_active` for the same one-block loop safety Claude uses.
Codex's Stop payload includes `cwd`, but the tracked primary hook does not use it to choose the guard executable.
Verified on 2026-07-08: Codex runs the Stop hook command with process PWD set to the hook-loaded project root, and no `CODEX_PROJECT_DIR`, `CODEX_WORKSPACE_ROOT`, or `CODEX_CWD` root variable is set.
The tracked hook anchors to `pwd -P`, verifies that root is broker-shaped and hook-bearing, and then invokes `bin/mx-turnend-guard.sh` with the original payload.
Codex's primary watcher protocol is `bin/mx-watch-checkpoint.sh --seconds "${MX_CODEX_WATCH_CHECKPOINT:-180}"`, not `bin/mx-watch-arm.sh`.
The checkpoint is deliberately foreground and bounded so Codex regains control regularly to process user messages and queued wakes.

## cursor (VERIFIED 2026-08-09, Cursor CLI 2026.08.04-aaa8809)

| Fact | Value |
|---|---|
| Busy-pane signature | `Working` with `ctrl+c to stop` |
| Exit command | Ctrl-D; Cursor prints the exact `agent --resume=<session-id>` command |
| Interrupt | Ctrl-C |
| Composer glyph | `→` |
| Trust | Multplx passes scoped `--trust` for its validated checkout and never mutates persistent trust configuration |

The canonical executable is `agent`; `cursor-agent` is an installation alias and Multplx provides a collision-safe shim for either spelling.
Every launch passes `--sandbox enabled`; `--force`, `--yolo`, `--sandbox disabled`, and Cursor-owned worktrees are rejected by the launcher adapter.
Tracked `.cursor/hooks.json` owns primary session-start, command, delegation-shape, and bounded stop adaptation.
Actors receive a private per-run plugin under `/tmp/mx-<id>/` for collision-free native stop signaling.
Cursor stop hooks do not fire in `--print` mode, so supervised lifecycle turns use interactive mode.
Resume uses the exact id printed on Ctrl-D.
The primary watcher protocol is the same bounded foreground checkpoint used for Codex.
Full commands and negative controls live in `docs/verification/cursor-cli.md`.

## pi (VERIFIED 2026-06-11)

| Fact | Value |
|---|---|
| Busy-pane signature | `Working...` (braille spinner prefix; no `esc to interrupt` text) |
| Exit command | `/quit` |
| Interrupt | single Escape |

Pi has no permission system, so actors are always autonomous.
Keep the brief as one positional argument.
Multiple positional args become separate queued messages; `mx-spawn`'s template already does this correctly.

Project trust dialog can appear on the first pi run in any not-yet-trusted directory, observed even on clean worktrees.
Accept with Enter.
The decision persists per path in `~/.pi/agent/trust.json`, so later spawns in the same worktree slot skip it.

`mx-spawn` keeps the turn-end extension in `state/`, outside the worktree, because project-local extension files make the trust gate strictly worse and pollute the project.
The extension must listen for pi's `turn_end` event, not `agent_end`, so the watcher wakes after each completed turn instead of only when the whole agent run exits.
Pi sets `PI_CODING_AGENT=true` for its children; this is its harness-detection env marker.

**Primary-session guard fact (verified 2026-07-09, Pi 0.80.5).**
The broker PRIMARY's own `.pi/extensions/mx-primary-turnend-guard.ts` listens for logical-run `agent_settled`, not per-tool-loop `turn_end`, and uses `pi.sendUserMessage(..., { deliverAs: "followUp" })` to force one guarded follow-up when `bin/mx-turnend-guard.sh` returns 2.
Without `deliverAs: "followUp"`, Pi rejects the send while the agent is still processing.
Pi's primary watcher protocol also requires the tracked `.pi/extensions/mx-primary-pi-watch.ts` extension, same trust-once discovery as the turn-end guard.
The model arms through `mx_watch_arm_pi`, never a foreground bash arm; the watcher tool result and clean-exit fallback are owned by `docs/supervision-protocols/pi.md`.
`bin/mx-session-start.sh` reports when the live Pi session has not loaded both the turn-end guard and watcher extensions, and points at plain `pi` after project trust as the fix, with `-e` as a trust-free fallback.
When a daemon is launched on Pi, `mx-spawn.sh --daemon` launches Pi with both `-e .pi/extensions/mx-primary-turnend-guard.ts` and `-e .pi/extensions/mx-primary-pi-watch.ts`, both already present in the daemon home's git worktree.
