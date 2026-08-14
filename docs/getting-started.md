# Getting started

This is the canonical path from installation to a safe first Multplx request.
Multplx remains a repository-based agent distribution, while a small global launcher makes one configured control plane available from any directory.

[Back to the documentation index](README.md).

## Requirements

Use macOS or Linux and install one verified coding-agent harness:

- Claude Code, launched with `claude`.
- Codex CLI, launched with `codex`.
- Cursor CLI, launched with `agent` or `cursor-agent`.
- Pi, launched with `pi`.

Every Multplx home needs Git, the official GitHub CLI, `jq`, and Treehouse with durable lease support.
Building from source additionally requires the stable Rust toolchain.
Your runtime backend adds its own CLI requirement.
tmux is the verified reference backend; Herdr and cmux are experimental, and Codex App is not selectable.

You do not need to authenticate GitHub in the broker session for public repositories.
Keep every write-capable GitHub credential outside the broker and all spawned agent sessions.

## Install the global command

Clone the repository once, then register that checkout as both the code root and persistent operational home:

```sh
git clone https://github.com/KashyapTan/Multplx.git
cd Multplx
cargo build --release --workspace --locked
bin/mx-launcher-install.sh
```

This existing-checkout mode preserves every current file under `data/`, `state/`, `config/`, and `projects/` in place.
It creates any missing private top-level directories but does not move or rewrite their contents.

For a hidden managed runtime and a separate persistent home, use managed mode instead:

```sh
bin/mx-launcher-install.sh --managed
```

Managed mode clones the configured origin into `${XDG_DATA_HOME:-$HOME/.local/share}/multplx/runtime` and creates the operational home at the sibling `home` directory.
Both modes install `multplx` under `${XDG_BIN_HOME:-$HOME/.local/bin}` and record literal root/home paths under `${XDG_CONFIG_HOME:-$HOME/.config}/multplx`.
The installer prints the directory to add to `PATH` when it is not already visible.
The installer copies the release binary and records its SHA-256 receipt before publication.
Pass `--binary <path> --checksum <sha256>` to install an externally supplied verified artifact, `--upgrade` to replace an owned installation, or `--uninstall` for data-preserving removal.
Run `target/release/mx launcher-install --help` for custom XDG paths, adoption of another checkout or home, managed source selection, and the complete recovery contract.

## Activate and choose a broker harness

From any directory, activate an ordinary child shell:

```sh
multplx
```

The child shell stays in the caller's directory and shows a static `multplx` marker.
The marker reads no state and does not claim that a broker is running.
Exit the child shell to restore the parent environment unchanged.

Inside the activated shell, launch one installed harness:

```sh
claude
# or: codex
# or: agent
# or: pi
```

The harness child alone changes to the configured Multplx code root so project instructions, hooks, skills, and Pi extensions load exactly as they do during a manual root launch.
Approve the repository trust prompt when the harness presents one.
Pi needs that approval so the tracked `.pi/extensions/*.ts` files can load.
Codex needs it so the tracked project configuration and hooks load.
Multplx passes Cursor's scoped `--trust` flag only after validating the configured code root and keeps Cursor sandboxing enabled.

An operational Multplx release uses tracked root `AGENTS.md` to define and auto-load the broker role.
At session start the broker runs `bin/mx-session-start.sh` exactly once, detects missing tools and invalid configuration, reconciles durable work, and emits the supervision instructions for the active harness.
Supported installs happen only after you approve them in that session; manual-only dependencies remain your responsibility.

Use `multplx paths` to inspect the configured code root, operational home, bootstrap, and config directory.
Use `multplx doctor` for the invariant sweep.
Use `multplx update` to fast-forward the configured source, rebuild the release binary, and transactionally replace the installed binary and checksum receipt; a failed build or publication leaves the prior installed generation runnable and records a bounded retry.
`multplx --help` and `multplx launcher-install --help` own the exact command grammar and exit statuses; the public shell filenames are transport-only adapters.
[Launcher verification](verification/launcher.md) records current deterministic, shell, and available real-harness evidence.

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
An activated shell preserves ambient tmux, Herdr, and cmux signals.
For a session-only choice, start it with `multplx --backend auto|tmux|herdr|cmux`; `auto` removes the session override and leaves normal detection authoritative.

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
multplx doctor
bin/mx-viz.sh serve
```

Doctor reports invariant health and does not mutate state unless you explicitly pass its closed `--fix` option.
The dashboard prints a loopback URL, never opens a browser, and remains a read-only view over the canonical system snapshot.

Next, read [Architecture](architecture.md) for the system model, [Configuration](configuration.md) for local operating choices, and [Delivery](delivery.md) for the credential boundary.

## Manual development launch

Contributors may still clone the repository, change to its root, and launch `claude`, `codex`, or `pi` directly without installing the global command.
That path remains useful while editing launcher code or testing an unregistered checkout, but the harness must start from the repository root for project-scoped discovery.
