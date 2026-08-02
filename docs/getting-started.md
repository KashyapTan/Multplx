# Getting started

This is the canonical path from a fresh clone to a safe first Multplx request.
Multplx is a repository-based agent distribution, so there is no separate app or package to install.

[Back to the documentation index](README.md).

## Requirements

Use macOS or Linux and install one verified coding-agent harness:

- Claude Code, launched with `claude`.
- Codex CLI, launched with `codex`.
- Pi, launched with `pi`.

Every Multplx home needs Node.js, Git, the official GitHub CLI, `jq`, and Treehouse with durable lease support.
Your runtime backend adds its own CLI requirement.
tmux is the verified reference backend; Herdr and cmux are experimental, and Codex App is not selectable.

You do not need to authenticate GitHub in the broker session for public repositories.
Keep every write-capable GitHub credential outside the broker and all spawned agent sessions.

## Clone and launch

```sh
git clone https://github.com/KashyapTan/Multplx.git
cd Multplx
```

Launch one installed harness from the repository root:

```sh
claude
# or: codex
# or: pi
```

Approve the repository trust prompt when the harness presents one.
Pi needs that approval so the tracked `.pi/extensions/*.ts` files can load.
Codex needs it so the tracked project configuration and hooks load.

The tracked `AGENTS.md` file defines the broker role.
At session start the broker runs `bin/mx-session-start.sh` exactly once, detects missing tools and invalid configuration, reconciles durable work, and emits the supervision instructions for the active harness.
Supported installs happen only after you approve them in that session; manual-only dependencies remain your responsibility.

## Choose a runtime backend

tmux is the reference backend and the fallback when no explicit setting or supported runtime environment selects another backend.
If you require tmux, select it explicitly by putting this value in local gitignored `config/backend`:

```text
tmux
```

The supported selectors are:

| Value | Support level | Best fit |
| --- | --- | --- |
| `tmux` | Reference | Portable terminal operation and daemon homes |
| `herdr` | Experimental | Native agent state and push events |
| `cmux` | Experimental | macOS GUI workspaces; daemon spawns are not supported |

Follow the selected backend's [tmux](tmux-backend.md), [Herdr](herdr-backend.md), or [cmux](cmux-backend.md) setup guide before the first task.
`config/backend` may also be omitted so runtime auto-detection can select the current supported terminal environment before falling back to tmux.

## Make the first request

Ask the broker to add or identify a project and state one concrete outcome.
For example:

```text
Add my project from https://github.com/example/project, then investigate the flaky login test.
```

The broker resolves the project, checks its toolchain and dispatch capacity, creates the appropriate brief, and routes project-specific work to an isolated actor.
It asks for a decision when project identity, delivery posture, or another maintainer-owned choice cannot be inferred safely.

Project delivery modes are explicit:

- `deep-review` runs the full local validation gate before creating an exact-SHA handoff.
- `direct-PR` stops at a clean local commit without the full validation gate, but is currently incomplete because no approved exact-SHA transition owns its delivery handoff.
- `local-only` stays on the machine and waits for the configured fast-forward merge authority.

The broker records project configuration under private gitignored `data/` rather than in the tracked template.
[Configuration](configuration.md) owns the registry, home layout, harness, backend, and dispatch details.

## Private repositories and delivery

If a private repository requires authenticated reads, `MX_AGENT_GH_TOKEN` may supply a remotely enforced read-only token to spawned agents.
It must not grant contents-write or pull-request-write permission.

For `deep-review` projects, remote delivery uses a separate maintainer shell or credentialed scheduler after local validation and approval.
That context runs `bin/mx-deliver.sh`; broker, actor, daemon, and validation sessions never run it and never receive its credential.
`direct-PR` projects cannot use this path until an approved exact-SHA handoff has an owner.
Read [Least-privilege delivery](delivery.md) before configuring private-repository access or automatic delivery.

## Verify the first run

Use these read-only operator entry points after the broker has completed session start:

```sh
bin/mx-doctor.sh
bin/mx-viz.sh serve
```

Doctor reports invariant health and does not mutate state unless you explicitly pass its closed `--fix` option.
The dashboard prints a loopback URL, never opens a browser, and remains a read-only view over the canonical system snapshot.

Next, read [Architecture](architecture.md) for the system model, [Configuration](configuration.md) for local operating choices, and [Delivery](delivery.md) for the credential boundary.
