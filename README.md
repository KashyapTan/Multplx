<h1 align="center">Multplx</h1>
<p align="center">
  <a
    href="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-blue?style=flat-square"
    ><img
      alt="Platform"
      src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-blue?style=flat-square"
  /></a>
  <a href="https://x.com/kunchenguid"
    ><img
      alt="X"
      src="https://img.shields.io/badge/X-@kunchenguid-black?style=flat-square"
  /></a>
  <a href="https://discord.gg/Wsy2NpnZDu"
    ><img
      alt="Discord"
      src="https://img.shields.io/discord/1439901831038763092?style=flat-square&label=discord"
  /></a>
</p>

<h3 align="center">Talk to one agent. Deliver with independent actors.</h3>

<p align="center">
  <img alt="Multplx - talk to one agent, delivery with independent actors" src="assets/banner.png" width="100%" />
</p>

## What it is

You can run one coding agent easily.
But the moment you want three project tasks done in parallel - fixes, investigations, plans, audits - you become a tab-juggler: babysitting sessions, copy-pasting context between repos, forgetting which terminal had the failing test.

Multplx flips the model.
You talk to a single agent - the broker - which routes work to autonomous actors in a visible session backend, gives each a clean git worktree, monitors their state, and brings you finished PRs, approved local merges, or standalone investigation reports.
For larger systems, you can opt in to persistent daemons that run from their own isolated Multplx homes.

Multplx is not a model, harness, skill, MCP server, or CLI.
Multplx is an agent distribution for coordinating independent agents.
An agent distro is a portable directory of instructions, skills, tooling, policies, and state conventions that turns a general-purpose agent into a specialized one.
There is no app to install: the cloned repo is the distribution - `AGENTS.md`, bundled Multplx skills, and helper scripts that any terminal coding agent can follow.
Launching a supported harness inside it instantiates the broker role, while you remain the maintainer.

## Features

- **One liaison** - you talk only to the broker; it routes work, monitors state, escalates only real decisions, and reports plain outcomes.
- **Visible actors** - every actor works in its own tmux window, experimental herdr tab, or cmux workspace you can watch or type into; the broker reconciles their durable state.
- **Disposable worktrees** - each task runs in a clean [treehouse](https://github.com/kunchenguid/treehouse) git worktree, so parallel work on one repo never collides.
- **Two task shapes** - delivery tasks deliver authorized changes; scout tasks leave standalone investigation reports when the intake contract warrants separate research.
- **Explicit project modes** - each project delivers via `no-mistakes`, `direct-PR`, or `local-only`, with an optional `+yolo` autonomy flag.
- **Optional daemons** - opt in to persistent daemons that run from isolated Multplx homes with their own `MX_HOME`, state, projects, and session lock, coordinate project clones or a project-less Multplx domain, stay aligned with the primary version through guarded local fast-forwards, and receive liveness checks at session start.
- **Event-driven, zero-token supervision** - a bash watcher sleeps on the system and wakes the broker only when something needs you; verified primary harnesses also get a turn-end backstop that blocks or follows up on a blind stop when work is under way and supervision is not live.
- **Guarded by construction** - the broker is read-only over your projects except for the guarded paths authorized by [hard rule 1](AGENTS.md#1-identity-and-prime-directives), with system sync's safe branch pruning remaining part of the system-sync exception; actors make every project change behind the configured merge authority.
- **Restart-proof** - all state lives on disk and in the active session backend (tmux by hard default, herdr or cmux when selected or auto-detected); kill the session anytime and the next one reconciles, including confirmed-dead daemon agents, and carries on.

Full detail on every feature lives in [docs/architecture.md](docs/architecture.md).

## Quick Start

### Requirements

- A verified agent harness: Claude Code, Pi, or Codex.
- Git and the GitHub CLI, authenticated through `gh auth login`.
- The CLI and dependencies for your selected runtime backend; tmux is the reference default.

The broker detects and offers to install supported missing tools after you approve.
Backend-specific setup is linked in [Documentation](#documentation).

### Recommended harnesses

**Claude Code and Pi are equal co-primary recommendations** for running the primary broker session.
Claude Code uses a tracked Stop hook for tokenless watcher re-arm and rewake, and Pi uses its tracked primary watcher extension.
Both have verified turn-end guard paths when launched with their documented setup.
Pick whichever one matches your subscription and workflow.

Codex is also verified and supported as a primary harness; it uses bounded foreground checkpoints, so it carries more harness-specific supervision tradeoffs than the co-primaries.

### Install and launch

```sh
gh auth login
git clone https://github.com/KashyapTan/Multplx.git
cd Multplx
```

Then launch one of the co-primary harnesses; AGENTS.md takes over from there:

**Claude Code**

```sh
claude
```

**Pi**

```sh
pi
```

For Pi, approve the project trust prompt once per clone on first launch so the tracked `.pi/extensions/*.ts` files auto-load.
Pi's `/calm` toggle hides supported transcript chrome, including canonically classified Multplx operational user rows, while retaining native working activity and all model context and session data.
The hidden operational inputs remain ordinary user-role messages with unchanged delivery, ordering, authority, persistence, and exports.
The preference persists for the effective Multplx home, and toggling it off restores ordinary rendering.
[Calm's current behavior and supported limits](docs/calm.md) are separate from its [version-scoped maintainer evidence](docs/calm-mode-feasibility.md).

### Talk to it

```sh
> recap! look at my github project xyz, then fix the flaky login test and add dark mode

# broker checks its toolchain (asking your consent before installing anything),
# clones the project under projects/ and spawns two isolated workers in the active backend.
# Minutes later:

  PR ready for review, maintainer: https://github.com/you/xyz/pull/42
  (fix flaky login test - risk: low - CI green)

> alright merge it
```

### More backends

Setup guides for tmux (the default) and every other supported backend (herdr, cmux) are linked in [Documentation](#documentation) below.

## How It Works

```
            you (the maintainer)
                  │  chat: requests, decisions, "merge it"
                  ▼
 ┌─────────────────────────────────────┐
 │ broker             (this repo)    │
 │ reads projects/ + routes requests │
 │ writes guarded backlog/briefs/state │
 └──┬──────────────┬───────────────┬───┘
    │ backend sends / status files │
    ▼              ▼               ▼
 ┌────────┐   ┌────────┐      ┌────────┐
 │mx-task1│   │mx-task2│  ... │mx-taskN│   tmux windows, herdr tabs, or cmux workspaces
 │actor│   │actor│      │actor│   one autonomous agent each
 └───┬────┘   └───┬────┘      └───┬────┘
     ▼            ▼               ▼
  treehouse worktree or isolated daemon home
     │
     ├─ delivery: project mode ► PR/local merge ► teardown
     │
     └─ scout: report at data/<id>/report.md ► decision inventory ► relay findings ► teardown
```

You chat with the broker.
It routes each request to an actor in its own session endpoint and git worktree, monitors the system with a zero-token event-driven watcher, and brings you finished PRs, approved local merges, or investigation reports.
Optional daemons extend this to persistent daemons, and dispatch profiles let you steer which harness handles which task.
`codex-app` is not a runtime backend yet; [docs/codex-app-backend.md](docs/codex-app-backend.md) owns the Codex App boundary.

Full architecture - the monitoring engine, worktree isolation, daemons, dispatch profiles, project modes, system sync, and self-update - is in [docs/architecture.md](docs/architecture.md).

## Built-in skills

Multplx delivers these user-invocable built-in skills.
Claude uses the slash form shown here; codex uses the same names with `$`, such as `$afk`.

| Skill              | What it does                                                                                                                                  |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `/afk`             | Enter away-mode supervision: the sub-supervisor self-handles routine notifications in bash, escalates maintainer-relevant events and bounded declared-external-wait rechecks as batched digests, and actively alerts if delivery gets stuck while you step away |
| `/recap`            | Recap visible session events since the prior real maintainer message plus visibly unanswered maintainer decisions, falling back to Catchup when invoked as the session's first real maintainer message |
| `/catchup`        | Generate a standalone current-status report from bounded local system and registered-daemon state, with live PR enrichment only when requested, written to a dated file in `data/` and surfaced concisely in chat; read-mostly, mutates no task state |
| `/updatemultplx` | Self-update the running Multplx primary and its daemons to the latest from origin with fast-forward-only pulls, then re-read instructions and nudge daemons |
| `/stow`            | Sweep the session for uncaptured durable knowledge, route each finding to its disk home per AGENTS.md, file undone next steps to the backlog, and report what is now safe to reset |

Agent-only reference skills live under `.agents/skills/` and are loaded by the broker at the trigger points named in [`AGENTS.md`](AGENTS.md).

### Two-tier skill layout

Multplx's skills live in two separate places with different audiences:

- `.agents/skills/` - agent-loaded skills (this section's table, plus the broker's agent-only reference skills). Every one assumes a live Multplx home and is meaningless, or actively misleading, installed anywhere else, so each carries `metadata.internal: true` in its frontmatter. That flag hides them from installer discovery without affecting the agent's own skill loader.
- `skills/` - public, installer-facing skills meant to be installed standalone into any project, independent of Multplx.
  Each one is a self-contained skill with no dependency on Multplx paths, tools, or vocabulary.
  Today that is `skills/stow`, a generic session-knowledge-sweep skill that routes findings by explicit instruction first, then existing local conventions, then a private `.stow-notes.md` fallback in the current directory, and closes with a resume pointer for the next session.
  It intentionally shares no code with the Multplx-internal `.agents/skills/stow` it is named after, so the two can evolve independently.

## Documentation

- [docs/architecture.md](docs/architecture.md) - maintainer architecture for the actors, supervision, worktrees, daemons, and project modes.
- [docs/configuration.md](docs/configuration.md) - environment variables, `MX_HOME`, runtime backend selection, the files you set, and harness support.
- [docs/calm.md](docs/calm.md) - current Pi `/calm` behavior and supported presentation limits.
- [docs/tmux-backend.md](docs/tmux-backend.md) - current setup and limits for the tmux reference backend.
- [docs/herdr-backend.md](docs/herdr-backend.md) - current setup, safety boundaries, and limits for the experimental Herdr backend.
- [docs/cmux-backend.md](docs/cmux-backend.md) - current setup, socket security, and limits for the experimental cmux backend.
- [docs/codex-app-backend.md](docs/codex-app-backend.md) - the current blocked Codex App backend boundary and rollout contract.
- [docs/verification/runtime-backends.md](docs/verification/runtime-backends.md) - active maintainer verification for runtime backend guarantees.
- [docs/turnend-guard.md](docs/turnend-guard.md) - the primary session's current "no turn ends blind" backstop, scope, loop safety, and compatibility limits.
- [docs/verification/supervision.md](docs/verification/supervision.md) - active maintainer verification for session-start, guard, continuity, and wedge integrations.
- [docs/supervision-protocols/](docs/supervision-protocols/) - rendered primary-harness watcher protocols for Claude, Codex, Pi, and unknown harness fallback.
- [docs/scripts.md](docs/scripts.md) - the `bin/` toolbelt reference.
- [docs/documentation-audiences.md](docs/documentation-audiences.md) - documentation audiences and the machine-checked placement boundary.
- [`AGENTS.md`](AGENTS.md) - the distro's always-loaded operating contract and routing index for conditional procedures.
- [CONTRIBUTING.md](CONTRIBUTING.md) - how to contribute, including the dev/test commands.

## Contributing

Contributions are welcome - see [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow, repo conventions, and how to run the tests.

## License

MIT - see [LICENSE](LICENSE).
