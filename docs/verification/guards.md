# Guard and decision-lifecycle verification

Audience: maintainer verification.

This record contains current dated, version-scoped evidence for the primary-session watcher-arm, persistent-directory, and delegation guards plus the decision-hold completion gate.
The linked mechanism pages own stable behavior, safety rationale, scope, and limits.
Scratch paths, private task identities, and delivery chronology are intentionally omitted.

[Back to the documentation index](../README.md).

## Watcher-arm command guard

The cross-harness live pass ran on 2026-07-09 in an isolated broker-shaped scratch repository with Claude Code 2.1.206, Codex CLI 0.144.0, and Pi 0.80.5.
No live watcher, operational state, or Herdr lifecycle command was used.

Each harness issued these exact command strings as separate tool calls:

```sh
printf 'UNRELATED_EXECUTED\n'
pgrep -fl '/bin/mx-watch.sh' || true
tmux send-keys -t isolated-pi-lab "printf '%s\n' 'bin/mx-watch-arm.sh &'"; tmux send-keys -t isolated-pi-lab Enter
bin/mx-watch-arm.sh &
```

The launch shapes were:

```sh
claude -p "$PROMPT" --dangerously-skip-permissions --output-format text
codex exec --dangerously-bypass-hook-trust --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check "$PROMPT"
pi -p -e .pi/extensions/mx-primary-turnend-guard.ts --no-context-files --no-session "$PROMPT"
```

All three harnesses ran the unrelated command, the read-only `pgrep`, and the two `tmux send-keys` calls that carried watcher text only as data.
All three blocked the backgrounded arm with exit 2 and stable reason `[watcher-background]`; the dummy-arm sentinel remained absent.
Claude and Pi reported the expected allow/deny split, while Codex recorded `PreToolUse Completed` for all three allowed shapes and `PreToolUse Blocked` only for the final command.

The native supervision paths also succeeded in the same scratch repository: Claude ran `bin/mx-watch-arm.sh --restart`, Codex ran the foreground checkpoint, and Pi called `mx_watch_arm_pi` with both primary extensions loaded.

Current deterministic entry points:

```sh
bash -n bin/mx-arm-pretool-check.sh
node --check bin/mx-arm-command-policy.mjs
tests/mx-arm-pretool-check.test.sh
```

The stable mechanism and complete acceptance matrix remain in [Watcher arm PreToolUse seatbelt](../arm-pretool-check.md).

## Persistent-directory guard

The live pass ran on 2026-07-11 in isolated primary-shaped scratch repositories with Claude Code 2.1.207, Codex CLI 0.144.0, and Pi 0.80.6.
Each harness received a top-level `cd projects/foo && touch <abs>/BLOCKED` and the scoped `(cd projects/foo && touch <abs>/ALLOWED)` control as separate tool calls.

```sh
claude -p "$PROMPT" --dangerously-skip-permissions --output-format text
codex exec --dangerously-bypass-hook-trust --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check "$PROMPT"
pi -p -e .pi/extensions/mx-primary-turnend-guard.ts --no-context-files --no-session "$PROMPT"
```

All three harnesses left the `BLOCKED` sentinel absent and created the `ALLOWED` sentinel.
Claude named the PreToolUse denial, Codex displayed the `[persistent-cd]` deny object and ran both configured PreToolUse hooks, and Pi produced the same top-level-denied/subshell-allowed differential.

Current deterministic entry points:

```sh
bash -n bin/mx-cd-pretool-check.sh
node --check bin/mx-cd-command-policy.mjs
node --check bin/mx-arm-command-policy.mjs
tests/mx-cd-pretool-check.test.sh
tests/mx-arm-pretool-check.test.sh
```

The stable mechanism and accepted non-goals remain in [cd-guard PreToolUse seatbelt](../cd-guard.md).

## Primary-session delegation guard

The live pass ran on 2026-07-22 with Claude Code 2.1.217 in fresh scratch repositories.
The common launch command was:

```sh
claude -p "$PROMPT" --dangerously-skip-permissions --output-format text
```

A match-all observation hook received tool names `Agent` and `Bash`, and an anchored `^(Task|Agent)$` matcher received `Agent` only.
The deny-key A/B used a fresh directory per run and produced this exact result:

| `.claude/settings.json` | `Agent` in tool list? |
| --- | --- |
| `{}` | Yes |
| `{"permissions":{"deny":["Task"]}}` | No |
| `{"permissions":{"deny":["Agent"]}}` | No |
| `{"permissions":{"deny":["ZzzNotARealTool"]}}` | Yes |
| `{"permissions":{"deny":["Task","Agent"]}}` | No |

The 29-tool baseline included the ordinary tools plus deferred delegation surfaces.
With the recommended local deny list active, all 18 named delegation tools disappeared while ordinary working tools remained.

For the fixed-list counterfactual, `Workflow` was removed from the local deny list while the delivered shape guard stayed wired.
The primary attempt was blocked with `[subagent-dispatch]`, proving the delivered guard catches a delegation-shaped future name that local hardening does not yet list.
The same hook and checker bytes allowed `Workflow` in a linked actor-shaped git worktree, proving scope rather than a globally broken hook.
Launching the primary with `MX_ALLOW_SUBAGENT=1` allowed the same call, proving the explicit environment-only escape hatch.

Current deterministic entry point:

```sh
bash -n bin/mx-subagent-pretool-check.sh
tests/mx-subagent-pretool-check.test.sh
```

The stable mechanism, exact deny list, harness applicability, and residual gap remain in [Primary-session delegation guard](../subagent-guard.md).

### Codex applicability

Codex CLI 0.146.0 was asked to enumerate its own tools in a fresh ephemeral scratch git repository under the active user configuration on 2026-08-01.

```sh
scratch_dir=$(mktemp -d /tmp/mx-codex-tool-surface.XXXXXX)
git -C "$scratch_dir" init -q
codex exec --ephemeral --sandbox read-only --skip-git-repo-check \
  -C "$scratch_dir" --color never \
  "List the exact names of every tool available to you in this session, one per line, nothing else. Then state on a final line whether you have any tool that spawns a subagent, sub-task, or delegated agent: answer SUBAGENT_TOOL=yes or SUBAGENT_TOOL=no. Do not call any tool; inspect only your provided tool definitions."
```

The exact reported tool set and verdict were:

```text
codex
functions.wait
functions.request_user_input
functions.exec
collaboration.followup_task
collaboration.interrupt_agent
collaboration.list_agents
collaboration.send_message
collaboration.spawn_agent
collaboration.wait_agent
apply_patch
exec_command
list_mcp_resource_templates
list_mcp_resources
read_mcp_resource
request_plugin_install
update_plan
view_image
write_stdin
image_gen__imagegen
web__run
SUBAGENT_TOOL=yes
```

`collaboration.spawn_agent` creates a delegated agent, so Codex was applicable to this guard in that tested configuration.
The tracked `.codex/hooks.json` did not wire the delegation guard at the time of the pass.
The earlier 2026-07-22 enumeration with Codex CLI 0.144.1 reported no delegation tool; applicability must therefore remain version- and configuration-scoped rather than being inferred from older evidence.

## Decision-hold completion gate

The focused synthetic regression was verified on 2026-07-14, extended for quoted `blocked_by` values on 2026-07-17, and extended for plural blocker readiness and mixed-home projection on 2026-07-22.
It uses only synthetic `sample` identities and decision text.

```text
$ bash tests/mx-decision-hold-lifecycle.test.sh
ok - report-only unresolved decision is reproduced and completion refuses before loss
ok - non-forced scout teardown always requires durable inventory verification
ok - maintainer holds are idempotent, distinct, teardown-safe, Catchup-visible, and durably routed before close
ok - completion and verification validate origins before constructing paths
ok - ended visual review follows the same decision-hold completion owner
ok - resolved findings and decision-like prose do not create false holds
ok - terminal single-owner stale status decisions do not block empty inventory
ok - main-home and daemon-home maintainer holds remain correctly routed
ok - resolve matches first/middle/last in quoted blocked_by and rejects a genuinely absent id

$ bash tests/mx-system-snapshot-view.test.sh
ok - backlog normalization preserves strict roles and resolves every blocker compatibly
ok - durable maintainer-held transfer closes the duplicate live status decision
ok - snapshot parses owned backlog rows and respects operational overrides

$ bin/mx-test-run.sh --family snapshot-catchup
ok - a completed scout with decision-like report prose is a pointer, not pending
ok - action-free items (working/done/queued/landed) do not leak into Maintainer's Call
ok - mixed daemon roles, partial state, and maintainer readiness project independently
ok - main and daemon maintainer actionability use the same blocker readiness

$ bash tests/mx-brief.test.sh
ok - mx-brief.sh: investigation and visual-review completions load the shared decision policy

$ bash tests/mx-teardown.test.sh
all teardown safety cases passed
```

The stable mechanism and structured read surfaces remain in [Decision hold lifecycle mechanism](../decision-hold-lifecycle.md).
