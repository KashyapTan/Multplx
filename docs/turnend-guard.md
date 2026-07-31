# Primary turn-end supervision guard

This is the authoritative current contract for the "no turn ends blind" primary backstop referenced from `AGENTS.md` section 8.
The predicate lives in `bin/mx-turnend-guard.sh`.
Primary scope lives in `bin/mx-primary-scope-lib.sh`, shared with the native session-start nudge in [`sessionstart-nudge.md`](sessionstart-nudge.md).
Harness hook files only adapt each verified harness's turn-end mechanism to that shared predicate.

Related PreToolUse guards deny unsafe commands before execution rather than detecting a blind turn end afterward.
Their separate owners are [`arm-pretool-check.md`](arm-pretool-check.md), [`cd-guard.md`](cd-guard.md), and [`subagent-guard.md`](subagent-guard.md).
Do not infer this guard's scope, loop safety, or compatibility tradeoffs for those guards.

## Current invariant

`bin/mx-guard.sh` is a pull-based warning that runs only when another supervision command invokes it.
The turn-end guard closes the remaining gap at the primary's own turn boundary.
When work is in flight and no identity-matched watcher has a fresh beacon, the harness integration must either block the turn end or force one bounded follow-up that uses the recovery instruction from the emitted session-start protocol.
The guard remains a backstop; [`watcher-continuity.md`](watcher-continuity.md) owns normal continuity.

## Shared predicate

The guard first calls the shared primary scope.
A daemon home runs its own primary Multplx session, so a genuine `.mx-daemon-home` marker includes it whether the home is a linked worktree or plain clone.
The marker must be a regular non-symlink file whose whitespace-stripped first line is a non-empty identifier containing only letters, digits, dots, underscores, and dashes.
An unmarked checkout or invalid marker falls through to the git-dir check.
That check keeps actor and scout linked worktrees inert because their git dir differs from their git common dir.
It also requires `AGENTS.md`, `bin/`, and the effective state directory.

For an in-scope primary, the guard counts in-flight work from `state/*.meta`.
The default cross-harness mode exits silently with no work in flight.
Otherwise it calls `mx_watcher_healthy <state-dir> <watch-path> [grace-seconds] [home]` from `bin/mx-wake-lib.sh`, the same identity-matched lock and fresh-beacon check used by `bin/mx-watch-arm.sh`.
A stale beacon blocks even when a watcher pid is live.
A fresh leftover beacon blocks when the lock is missing, dead, or identity-mismatched.

`MX_STATE_OVERRIDE` wins over `MX_HOME/state`, and `MX_HOME` wins over repository-root `state/`.
`MX_GUARD_GRACE` controls beacon freshness and defaults to 300 seconds.
If `jq` is missing or hook stdin is empty, the guard exits 0 because it cannot safely read loop-guard fields.

## Harness integrations

- Claude registers two `Stop` hooks in `.claude/settings.json`, both anchored through `CLAUDE_PROJECT_DIR`: `bin/mx-turnend-guard.sh --claude`, and `bin/mx-claude-stop-autoarm.sh` with `asyncRewake: true` and `timeout: 28800`.
- Codex registers a `Stop` hook in `.codex/hooks.json`, anchors the executable to the hook process working directory, verifies a Multplx-shaped hook-bearing root, and passes the original payload to the shared guard.
- Pi listens for `agent_settled` in `.pi/extensions/mx-primary-turnend-guard.ts`, runs once per logical agent run, and calls `pi.sendUserMessage(..., { deliverAs: "followUp" })` once when the guard returns 2.

Claude and Codex can block a Stop directly with exit status 2 and stderr.
Both payloads carry `stop_hook_active`.
In the default Codex mode, a true value lets the second stop finish after one forced continuation.

Claude runs the guard with `--claude`, which ignores `stop_hook_active` and cooperates with the Stop-owned auto-arm.
Claude Code sets `stop_hook_active=true` on every stop after any stop-hook continuation, including `asyncRewake` rewakes, which re-opened the 2026-07-21 blind window under the default one-shot behavior.
The Claude mode waits up to `MX_CLAUDE_AUTOARM_SYNC_WAIT_MS` (default 800 milliseconds) and allows the stop when the watcher is healthy, `state/.claude-autoarm.lock` has a live owner, or `state/.claude-autoarm-epoch` contains a fresh rewake outcome.
When none of those proofs appears, it re-blocks up to `MX_CLAUDE_TURNEND_BLOCK_BUDGET` times (default 3, below Claude's 8-block override), then allows degraded with a visible `systemMessage`.
Any allow resets the budget.

Pi exposes a passive callback for this purpose.
Its adapter fails open at the hook boundary to protect the user session but schedules one bounded follow-up when the predicate blocks.
The generated prompts use the canonical `turn-end-guard` kind after the U+2063 `MULTPLX_OP: ` prefix, so Recap does not treat them as maintainer messages.
The adapter owns a loop latch.
Pi keeps the latch across internal tool turns and clears it only when the generated follow-up settles or delivery fails.

If a passive adapter cannot invoke its SDK, the next pull-based `mx-guard.sh` call reports the problem.
That warning uses `bin/mx-supervision-instructions.sh --repair-line`, so it always points to the active harness protocol rather than embedding another repair command.

## Compatibility limits

- Child actor and scout worktrees are outside scope.
- A valid daemon home is in scope; an idle daemon endpoint remains healthy because it has no supervision need.
- Claude and Codex block directly, while Pi uses bounded passive follow-ups.
- Missing `jq` or unreadable hook input remains fail-open.
- No harness adapter uses a shell ampersand to manufacture supervision.

## Regression coverage

`tests/mx-turnend-guard.test.sh` covers the predicate, main and daemon primary scope, child-worktree exclusion, `MX_HOME` and `MX_STATE_OVERRIDE` precedence, the cooperative `--claude` claim wait, epoch allow, re-block budget, Pi logical-run latching, missing-`jq` behavior, and all three registrations.
`tests/mx-supervision-instructions.test.sh` covers recovery-line ownership.
`MX_PI_LIVE_E2E=1 tests/mx-pi-primary-live-e2e.test.sh` is the opt-in isolated Pi path.
[`verification/supervision.md`](verification/supervision.md#turn-end-guard) records the active cross-harness empirical evidence, including the 2026-07-24 Claude `asyncRewake` revalidation.
