# Getting started

Install Multplx once, then open the same workspace and orchestrator conversation from any directory.
Use existing local repositories before considering an intentional clone.

[Back to the documentation index](README.md).

## Requirements

Use macOS or Linux with one supported coding-agent harness:

- Claude Code: `claude`.
- Codex CLI: `codex`.
- Cursor CLI: `agent` or `cursor-agent`.
- Pi: `pi`.

Git supplies project identity and isolated worktrees.
The supported runtime backend requires its own CLI; tmux is the reference backend, while Herdr and cmux retain their documented experimental limits.
GitHub publication uses the official GitHub CLI and ordinary authentication; remote-free tasks stay local.
Other existing runtime tools, including `jq`, are listed in [configuration](configuration.md).
Safe worktree cleanup uses `lsof`; unavailable occupant observation retains the allocation.
Only source builds require Rust.
A package does not supply harness authentication or replace its trust prompt.
Codex Desktop is not a shell-callable Multplx backend.

## Install a platform package

Obtain the versioned package for your platform and its SHA-256 file from the release distributor.
Verify that the checksum comes from the intended release before extracting it.
For example, replace `VERSION`, `OS` and `ARCH` with the package's actual values:

```sh
shasum -a 256 -c multplx-VERSION-OS-ARCH.tar.gz.sha256
tar -xzf multplx-VERSION-OS-ARCH.tar.gz
./multplx-VERSION-OS-ARCH/bin/mx launcher-install --package ./multplx-VERSION-OS-ARCH
```

Linux also supports `sha256sum -c` for archive verification.
The installer validates the package inventory, platform, version and file hashes before publishing the binary and matching runtime assets.
It keeps runtime assets separate from the persistent operational home.
Neither a Multplx source checkout nor a Rust build is needed for this path.

The command defaults to `${XDG_BIN_HOME:-$HOME/.local/bin}/multplx`, configuration records to `${XDG_CONFIG_HOME:-$HOME/.config}/multplx`, and managed runtime/home storage below `${XDG_DATA_HOME:-$HOME/.local/share}/multplx`.
Add the printed binary directory to `PATH` if needed.
`multplx paths` shows the selected runtime and home.
`multplx launcher-install --help` documents custom directories and adoption of an existing home.
Before using a new runtime with an existing legacy home, stop its writers and follow [operational-home migration](state-migration.md).

A package can be upgraded with the extracted new version:

```sh
./multplx-VERSION-OS-ARCH/bin/mx launcher-install --upgrade --package ./multplx-VERSION-OS-ARCH
```

Upgrade checks whether the installed runtime is quiescent.
A well-formed ordinary task may remain unfinished when its exact recorded runtime endpoint is confirmed stopped; upgrade preserves that task's state and evidence byte-for-byte.
Live, uncertain, persistent, and coordinator-owned runtime users still prevent upgrade.

Upgrades publish owned installation records transactionally and retain operational data and user repositories.
Upgrade and uninstall first exclude supported launches, then refuse while a primary harness, launch reservation or task record is live or uncertain; stop or reconcile those recorded users before retrying.
Uninstall removes the owned command and installation records without deleting your operational home or repositories:

```sh
multplx launcher-install --uninstall
```

The lean redesign has a validated local candidate; public release publication and deliberate source-checkout cutover are separate steps.
An isolated package validation result is not evidence that a public release has been published.

## Open the workspace

From home, a repository, a subdirectory or an unrelated directory:

```sh
multplx
```

The terminal workspace shows projects, tasks, decisions and connection state.
Use `multplx workspace --plain` for noninteractive output.
Select a supported installed harness explicitly on first use:

```sh
multplx chat codex
# Alternatives: multplx chat claude, multplx chat cursor, multplx chat pi
```

The launcher remembers the selection, and subsequent `multplx chat` uses it.
The named forms `multplx codex`, `multplx claude`, `multplx cursor` and `multplx pi` remain supported.
The harness starts from the validated runtime context so its instructions and integration assets load, while the original directory remains optional next-request context.
Preserve the harness's actual authentication and trust requirements.
An existing live owner remains authoritative; connection uses a supported recorded route or displays where to continue the existing conversation.
An unavailable attachment does not create another orchestrator.
[Launcher verification](verification/launcher.md) separates deterministic tests from real-provider evidence.

## Remember local projects

Register a selected checkout without cloning or changing its working files:

```sh
multplx projects register ~/dev/my-app --alias my-app
multplx projects list
multplx my-app
```

Use an exact alias or path when duplicate names are ambiguous.
Optional recursive discovery roots make nested repositories easier to find; `multplx projects --help` documents roots, exclusions, symlink policy and incremental refresh.
Roots are not required for chat or explicit paths, and the launcher never automatically scans an arbitrary current directory.
The [workspace guide](workspace-entry.md) describes discovery, location repair, unregister and the terminal keys.

Selecting context never changes the project, checkout or starting revision of accepted tasks.
Borrowed repositories retain their branches, index and dirty files; implementation starts in an isolated worktree from a recorded commit.
Uncommitted working changes are excluded unless explicitly handled as part of the request.
Remote-free repositories remain usable for local tasks, and discovering an unversioned folder does not create Git or a remote.

## Make the first request

Ask in the one orchestrator chat:

```text
Fix login in my-app, investigate the flaky test in repo-two, and research feature C in repo-three.
```

The orchestrator delegates each implementation to a sub-agent and keeps each repository's instructions and evidence scoped to its task.
One ambiguous or blocked task does not stop independent work.
Use explicit [scoped coordinator commands](scoped-coordinators.md) when a bounded domain benefits from a coordinator; project selection alone never creates one.

Equivalent durable terminal intake is available:

```sh
multplx task --project my-app "Fix login"
```

Its receipt confirms recorded intake rather than claiming an agent is already implementing it.
Use the request identity printed by the command for an uncertain retry, following `multplx task --help`.
[Durable coordination](durable-coordination.md) explains separate acceptance, delivery, acknowledgement and completion facts.

Workers may commit, push branches and create or update PRs with ordinary Git and forge authentication.
PR merging remains human-only.
Deep-review and vplan remain explicit optional tools rather than publication prerequisites.
See [delivery](delivery.md) for retry and evidence contracts.

## Backend and shell options

Select the supported task backend through local `config/backend` or the launcher's `--backend auto|tmux|herdr|cmux` option.
`auto` leaves normal runtime detection authoritative.
Follow the [tmux](tmux-backend.md), [Herdr](herdr-backend.md) or [cmux](cmux-backend.md) guide for prerequisites and actual persistent-home limits.

Explicit shell activation remains available:

```sh
multplx shell
codex
```

The child shell stays in the caller's directory and adds a static marker and harness shims.
Exit restores the parent environment unchanged.
The marker does not claim an active orchestrator.

After normal session startup, inspect state with `multplx doctor` or open the read-only [MX Viz](viz.md) view from the terminal workspace.
Doctor does not repair state unless its explicit `--fix` option is supplied.
The dashboard is not a second chat or mutation interface.

## Source installation

Developers may retain the source-install path:

```sh
git clone https://github.com/KashyapTan/Multplx.git
cd Multplx
cargo build --release --workspace --locked
bin/mx-launcher-install.sh
```

This registers an operational release checkout; during the lean redesign, contributors must follow `CLAUDE.md` and keep the root contract dormant.
Use `--root PATH --home PATH` to separate an adopted source checkout and home.
The legacy `--managed` source mode remains available for advanced use and requires Git and Rust for source updates.
`multplx update` owns source-mode refresh; package upgrades use a verified new package as shown above.
Do not replace a live installation with an unmerged development binary.
