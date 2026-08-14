# Primary-session delegation guard

This document is the authoritative human-readable contract for the guard that stops a broker primary from routing work outside the system.

The delivered mechanism is the Rust supervision domain behind `bin/mx-subagent-pretool-check.sh`, a PreToolUse guard that denies a delegation-SHAPED tool name in a genuine primary home.
Claude primaries should also use an untracked per-home local `permissions.deny` list as hardening for known Claude delegation tools, because it removes them from the model's schema entirely.
That deny list must not land in tracked `.claude/settings.json` because it is Claude-only rather than harness-agnostic, and because tracked project settings propagate into linked worktrees where they disarm legitimate actors.

## Why this exists

On 2026-07-22 a broker primary ran four workers through Claude Code's built-in subagent tool instead of `bin/mx-spawn.sh`.
Three consequences were observed, not hypothesized.

- The system view showed zero work under way for the whole run, because no `state/<id>.meta` and no `data/<id>/brief.md` were ever created.
- When the primary session restarted, two of those workers died mid-flight and their work was lost.
  A real actor lives in its own backend session with durable state and survives a primary restart.
- The supervision cycle then stayed down for 73 minutes unnoticed, which silently killed the maintainer's Workflowy intake channel, since that channel only fires while a watch cycle runs.

The deeper defect is that the bypass did not merely skip dispatch, it made the in-flight-work branch of the guard stack structurally inert.
Only `bin/mx-spawn.sh` writes `state/<id>.meta`, so untracked project work contributes nothing to the in-flight count used by `bin/mx-supervision-lib.sh` and `bin/mx-turnend-guard.sh`.
Work started through the harness's own delegation tool writes no metadata, so the in-flight count stayed at zero and the turn-end guard never blocked a blind turn end.

That is the reason the fence has to sit on the harness tool surface, before the primary can create untracked work.
No additional guard keyed on task metadata can catch this class of failure, because the failure is precisely the absence of that metadata.

## Purpose and boundary

The guard addresses one concrete, mechanically identifiable event: the primary session reaching for a tool that creates work the system will not know about.

It deliberately does **not** address the broader question of whether a given piece of work should be routed at all.
That question is a judgment boundary over read-and-think work, it has no tool-shape signal, and a hook that tried to police it would degrade into an advisory nag.
The scope line is therefore: wrong tool reached for, deny; wrong amount of thinking done before reaching for a tool, out of scope.

The guard is also not a dispatch-quality check.
It says nothing about whether the resulting brief, project, or delivery mode is correct.

## Delivered mechanism

`multplx_domain::supervision::subagent_guard` is the decision owner and `bin/mx-subagent-pretool-check.sh` is its stable hook adapter.
It classifies the tool NAME by shape rather than against a fixed list.
The tracked Claude PreToolUse matcher is `.*`, so every Claude tool name reaches the script and the script is the single owner of classification.
A stem-enumerating matcher would reintroduce the fail-open-by-enumeration problem this guard exists to solve, because any future tool name outside the matcher would be silently missed before the script could inspect it.
A tool is delegation-shaped when its normalized lowercase name contains one of these stems:

```text
agent  subagent  task  workflow  cron  schedul  worktree
delegate  spawn  dispatch  handoff  remote  sendmessage  monitor
```

Two exclusions keep the shape test from producing false positives.

- A name beginning `mcp__` is never classified.
  An MCP server chooses its own tool names, a task or agent noun there is common, and it has no bearing on system dispatch.
- The exact names `taskoutput`, `taskstop`, `taskget`, `tasklist`, `cronlist`, `bashoutput`, and `killshell` are allowed.
  These observe or stop work that already exists rather than creating it, and denying them at this layer could strand already-running work with no way to inspect or end it.
  A Claude primary's optional local deny list may still remove them from the schema.
  The delivered guard stays narrower on purpose so it can never be the reason a runaway task cannot be stopped.

The delivered guard fires on every delegation-shaped name that reaches it, including future names that no deny list knows about yet.
That future-name behavior is the reason the tracked matcher must match all tools and let the script filter.

## Recommended Local Claude Deny List

Claude primaries should add this deny list in untracked per-home local settings, never in tracked `.claude/settings.json`:

```json
{
  "permissions": {
    "deny": [
      "Task",
      "Agent",
      "Workflow",
      "RemoteTrigger",
      "Monitor",
      "ScheduleWakeup",
      "SendMessage",
      "EnterWorktree",
      "ExitWorktree",
      "CronCreate",
      "CronDelete",
      "CronList",
      "TaskCreate",
      "TaskGet",
      "TaskList",
      "TaskUpdate",
      "TaskStop",
      "TaskOutput"
    ]
  }
}
```

A denied name is removed from the model's schema entirely.
The model is never offered the tool, so there is no call to intercept, no matcher to get wrong, no fail-open path, and no dependence on the model's cooperation.
This is removal, not interception, and it is strictly stronger than any hook.

This list is recommended local hardening because it closes the known Claude surface before the hook is needed.
It is not tracked for two reasons.

- It is Claude-only, so it can never be the harness-agnostic delivered fix.
- A tracked `.claude/settings.json` propagates into linked worktrees and disarms legitimate actors.
  This was verified when a Claude session in a task worktree of this repo lost its `Agent` tool.

The width of the list remains a maintainer-owned decision, because denying some of these changes how the maintainer works with the primary session.
Keep it as one flat local array that is reviewable at a glance and narrowable in one line.
In particular `TaskOutput`, `TaskStop`, `TaskGet`, `TaskList`, and `CronList` only observe or stop work that already exists, but the recommended local deny list still removes them by default.
The hook deliberately allows those names, so the delivered guard can never strand a runaway task with no way to inspect or end it.

`permissions.allow` is a pre-approval list, not an availability list, so there is no fail-closed positive allowlist available.
That is why any fixed deny list is fail-open against future tools and why the shape-based guard still exists.
The hook cannot re-enable a tool removed from the schema; it only handles a tool name that still reaches PreToolUse.

### Both `Task` and `Agent` are valid deny keys

The tool presents to the model as `Agent`.
A prior investigation recorded that the deny key must be `Task` and that using `Agent` "silently does nothing at all".
That is not what this machine shows.

A five-way A/B with a control, each run in its own directory to rule out settings caching, found that `Task` and `Agent` each independently remove the tool, and that a nonsense name leaves it present.
The dated A/B evidence is in [`verification/guards.md`](verification/guards.md#primary-session-delegation-guard).

Pinning both names in the recommended local deny list is correct regardless of which build is running.
It costs one line and removes the failure mode where a rename or a rollback silently reopens the surface.

## Scope

The delivered hook fires only in a genuine broker primary home, using the shared predicate `mx_primary_scope_matches` from `bin/mx-primary-scope-lib.sh`.
This is the same predicate `bin/mx-sessionstart-nudge.sh` and `bin/mx-turnend-guard.sh` use, so the three tracked primary-scoped hooks cannot drift apart.

A home is in scope when it has `AGENTS.md`, a `bin/` directory, an existing state directory, and either a plain checkout where git-dir equals git-common-dir or a valid `.mx-daemon-home` marker.
The guard accepts only the exact root `AGENTS.md` filename.
A marked daemon home is in scope on purpose: it operates its own system and must dispatch through it for the same durability reasons.

an actor's disposable task worktree is a linked git worktree, which is the shape `bin/mx-spawn.sh` always hands out, so it is out of scope.
an actor using delegation tools inside its own task worktree is legitimate and stays allowed.
A non-Multplx repo is out of scope.
Any failure to confirm the home is inert, never a block, so a broken environment can never deny a tool call.

A local Claude deny list is upstream of hook scope and removes known Claude delegation tools wherever Claude applies it.
Do not put that list in tracked project settings, because linked worktrees inherit those settings and would lose legitimate delegation tools.
The hook scope is the delivered enforcement boundary, and the linked-worktree negative case proves the script itself does not block legitimate actor delegation.

## Escape hatch

`MX_ALLOW_SUBAGENT=1` in the session environment allows the call at the delivered hook.
This is the only escape hatch and the guard fails closed on every other value, including empty, `0`, `yes`, and `true`.

It is an environment variable rather than a flag, a config file, or a state file because that makes it unforgeable in-session.
The variable must be present when the harness process is launched, so no tool call the agent makes can enable it for the call that follows.
A deliberate use therefore requires restarting the session with the variable set, which is a conscious act, while an accidental use is impossible.

The escape hatch does not affect any local Claude deny list.
A tool removed from the schema stays removed, so a genuinely intended use of a locally denied tool also requires narrowing or removing that local entry before launch.

## Output contract

- Allow returns exit 0 with both streams empty.
- Deny returns exit 2 and writes `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"},"systemMessage":"[subagent-dispatch] ..."}` to stderr.
- Default deny mode also writes `{"decision":"deny","reason":"[subagent-dispatch] ..."}` to stdout for adapters that consume a decision JSON.
- `--claude` suppresses stdout completely, because Claude Code ignores a PreToolUse deny when stdout is nonempty.
  This is the same verified quirk recorded in [`arm-pretool-check.md`](arm-pretool-check.md), and the tracked Claude hook therefore passes `--claude`.
- Malformed or empty stdin, invalid JSON, a payload with no tool name, and missing `jq` for stdin transport all fail open with exit 0 and no output.

The deny message names the real dispatch path.
When `bin/mx-scout.sh` exists in the home the message first defers to the `AGENTS.md` intake classification, then routes work already classified as a scout there and authorized delivery work with its bounded research to `bin/mx-brief.sh` then `bin/mx-spawn.sh`.
When that script is absent the message still defers to intake classification and degrades to naming `bin/mx-brief.sh` then `bin/mx-spawn.sh` for dispatched work, rather than pointing at a script that is not there.

## Harness wiring

Every supported primary harness was reviewed.
Applicability turns on one question: does the harness expose built-in delegation tools that a primary session could use instead of `bin/mx-spawn.sh`?

| Harness | Delegation surface | Status |
| --- | --- | --- |
| Claude | 18 known tools, listed above | Scoped guard wired and live-verified; untracked local deny list verified and recommended. |
| Codex | `collaboration.spawn_agent` | Applicable in the empirically tested Codex CLI 0.146.0 tool surface; the guard is not wired in `.codex/hooks.json`. |
| Pi | none reported | Not wired pending live verification. See below. |

### Codex applicability

Codex CLI 0.146.0 exposed `collaboration.spawn_agent` in an ephemeral scratch repository under the active user configuration, so that empirically tested surface is applicable to this guard.
Current dated tool-enumeration evidence lives in [`verification/guards.md`](verification/guards.md#codex-applicability).
The tracked `.codex/hooks.json` does not wire the delegation guard, so a Codex primary with that tool surface can still create work outside `bin/mx-spawn.sh`.
Hook support and matcher behavior must be validated in a scratch project before wiring it.

### Pi, inspected but not wired

The integration surface was inspected and is structurally wireable for the delivered guard.

- Pi's tracked extension gates on `event.toolName !== "bash"` inside `pi.on("tool_call", ...)` and blocks by returning `{block: true}`.
  Swapping that comparison for a call into this checker with `--tool` is the whole change.
  A parallel evaluation reports that Pi exposes no delegation tool at all, which would make it not applicable, but that was not verified here.

It is not wired in this change because the exact tool-name tokens could not be confirmed and the wiring could not be validated against the real harness.
This repo's rule in the `multplx-coding-guidelines` skill is that a harness hook must be validated in a scratch project before it is trusted, and `arm-pretool-check.md` records the concrete cost of guessing: a hook whose `command` string is even slightly wrong fails to launch the hook at all.
Wiring an unvalidated matcher would trade a known gap for an unknown breakage.

The bounded follow-up is identical to the Codex procedure above.
On a host where verification is possible, ask the harness to enumerate its tools, then wire the matcher and re-run the live matrix below.
`bin/mx-subagent-pretool-check.sh` needs no change: it already accepts the `--tool` CLI form Pi uses, and it already emits the stdout decision object by default.

Current dated Claude proof lives in [`verification/guards.md`](verification/guards.md#primary-session-delegation-guard).

## Automated validation

`tests/mx-subagent-pretool-check.test.sh` owns the acceptance matrix and is registered in the Rust runner's `pure-contract-unit` family.
It covers the tracked Claude settings boundary that forbids a `permissions` key; the match-all Claude hook registration; denial of every work-creating delegation tool by shape; denial of twelve hypothetical future tool names that appear on no list; the observe-or-stop and MCP exclusions; the scout-present and scout-absent message variants; the escape hatch including its fail-closed values; inertness in a linked task worktree and in a non-Multplx repo; in-scope enforcement for a marked daemon home; both stdin transports; the empty-stdout requirement; fail-open transport behavior; and the preserved `Bash` seatbelts and `Stop` guard.

Run:

```sh
bash -n bin/mx-subagent-pretool-check.sh
tests/mx-subagent-pretool-check.test.sh
```

## Known residual gap

This change does not close the deeper harness-agnostic defect.
Every broker guard's in-flight-work branch keys off `state/<id>.meta`, and only `bin/mx-spawn.sh` writes that record.
Unaccounted primary work contributes nothing to that predicate, and therefore reads as idle rather than suspicious.

The durable fix for that class is to make the guards treat "the primary is doing project-shaped work with zero `state/*.meta` files" as a suspicious state rather than an idle one.
That would catch this class on any harness, including work created through `Bash`.
This change fences only the Claude tool surface.
That is a separate change to `bin/mx-supervision-lib.sh` and `bin/mx-turnend-guard.sh` and is out of scope here.
