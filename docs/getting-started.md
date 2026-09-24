# Getting started

Install the full runtime once, register a repository and start one orchestrator conversation.
You do not need a prebuilt release archive.
[Documentation index](README.md) · [Human command reference](commands.md) · [User guide](user-guide.md).

## Prerequisites

Use macOS or Linux with Bash, Git and current stable Rust/Cargo for the source build.
The runtime's [toolchain](configuration.md#toolchain) includes Git, `gh` and `jq`; use `tmux` for the reference backend and `lsof` for safe worktree cleanup.
Install and authenticate at least one harness: Codex CLI (`codex`), Claude Code (`claude`), Cursor CLI (`agent` or `cursor-agent`) or Pi (`pi`).
The Multplx installer does not install an agent CLI or sign you into a provider.

On macOS with Homebrew:

```sh
brew install rust git gh jq tmux
```

macOS normally supplies `lsof`.
If the compiler reports missing developer tools, run `xcode-select --install`, finish that installation and retry.

On Ubuntu/Debian:

```sh
sudo apt-get update
sudo apt-get install -y build-essential git curl gh jq tmux lsof
```

Install current stable Rust using [rustup](https://rustup.rs/) if `cargo --version` is unavailable or the distribution's Rust is too old.
An older compiler may not support this repository's Rust 2024 edition or locked dependencies.
After installing Rust, open a new terminal so Cargo is on `PATH`.
For other Linux distributions, install the same tools with the system's package manager.

Check the build tools before starting:

```sh
git --version
cargo --version
```

## Install from a clone

```sh
git clone https://github.com/KashyapTan/Multplx.git
cd Multplx
./install.sh
export PATH="$HOME/.local/bin:$PATH"
multplx paths
```

If you already have this checkout, start at `cd /path/to/Multplx` and run `./install.sh`.
Do not clone a second copy just to install it.
The command builds the locked release binary, assembles matching assets and runs the transactional installer.
It installs the command, operating contract, harness integration, skills, workflows and dashboard together.
The clone is not used as the operational home, and no agent is launched during installation.
The first compile may take several minutes.

Defaults are:

| Item | Default location |
| --- | --- |
| Command | `~/.local/bin/multplx` |
| Installation records | `~/.config/multplx` |
| Runtime assets | `~/.local/share/multplx/runtime` |
| Operational home | `~/.local/share/multplx/home` |

The corresponding XDG environment variables can change these defaults.
The installer prints the actual paths; `multplx paths` confirms them.
For future zsh terminals, add this line once to `~/.zshrc` (or `~/.bashrc` for Bash):

```sh
export PATH="$HOME/.local/bin:$PATH"
```

If you used a custom binary directory, add that printed directory instead.
For example, an entirely separate installation is:

```sh
./install.sh --bin-dir "$HOME/.local/multplx-test/bin" \
  --config-dir "$HOME/.local/multplx-test/config" \
  --data-dir "$HOME/.local/multplx-test/data"
```

Use `./install.sh --help` for supported options.
Keep the source checkout for future `git pull` and upgrades, or use the download bootstrap below once published.

## Download and install from anywhere

After `install.sh` and `install-from-github.sh` are published on public `main`:

```sh
curl -fsSL https://raw.githubusercontent.com/KashyapTan/Multplx/main/install-from-github.sh | bash
```

The bootstrap downloads a temporary clone of `main`, runs the same installer and removes its temporary checkout afterward.
It needs the same prerequisites and builds from source; it is not a prebuilt binary download.
The new URL does not work until these local changes have been pushed.
To inspect the script before executing it, download it to a file, read it and run it with Bash.

## Register your first project

Use an existing Git checkout with at least one commit:

```sh
multplx projects register ~/dev/my-app --alias my-app
multplx projects list
```

Replace `~/dev/my-app` with your repository's actual path.
Registration leaves its files and branch alone.
A project can be remote-free for local work.
Uncommitted files are preserved but are not automatically included in the task's committed starting revision.

## Start the orchestrator

With your chosen harness installed and authenticated:

```sh
multplx chat codex
```

Alternatives are `multplx chat claude`, `multplx chat cursor` and `multplx chat pi`.
The launcher remembers the selection; later use `multplx chat`.
Complete the harness's own authentication or trust prompt if it appears.
An existing live conversation remains authoritative; the launcher connects through a supported route or tells you where that conversation is.
It does not create a second orchestrator to bypass an attachment limit.
[Launcher verification](verification/launcher.md) records tested provider boundaries.

Ask in the chat:

```text
Fix the login issue in my-app. Run the relevant tests and open a PR.
```

For a local-only outcome, say so instead of asking for a PR.
GitHub publication needs ordinary authentication, such as `gh auth login`; only the human merges PRs.

## Follow the work

In another terminal:

```sh
multplx
```

Use arrows to browse, `Tab` to change sections, `c` to enter chat and `v` to open MX Viz.
The terminal workspace is a dashboard for the same orchestrator, not another AI chat.
MX Viz is the browser dashboard, with a detailed Tasks view and a searchable, collapsible Agents graph.
Closing the workspace or browser does not stop independent tasks.
For a plain terminal summary or command-line request:

```sh
multplx workspace --plain
multplx task --project my-app "Investigate the flaky test"
```

The request receipt means accepted, not started or completed.
See the [command reference](commands.md) for discovery, dependencies, scoped coordinators, workflows and recovery.

## Upgrade

From your source checkout, after stopping or reconciling active runtime users:

```sh
git pull --ff-only
./install.sh --upgrade
```

If you installed through the download bootstrap, rerun it with `--upgrade` once that script is published:

```sh
curl -fsSL https://raw.githubusercontent.com/KashyapTan/Multplx/main/install-from-github.sh | bash -s -- --upgrade
```

Use the same custom path options if you selected non-default installation directories.
Upgrade preserves operational data and repositories; live or uncertain runtime users can prevent it.
For package-mode installs created by this installer, use this upgrade command rather than `multplx update`, which owns legacy source-mode updates.
For an old installation that points directly at a source checkout, uninstall its registered application first and make a fresh full installation.
Before reusing a legacy operational home, follow [home migration](state-migration.md); a new default home does not import old task state automatically.

## Uninstall

```sh
multplx launcher-install --uninstall
```

Supply the same `--bin-dir`, `--config-dir` and `--data-dir` options for a custom installation.
Uninstall removes owned application files and installation records while preserving operational data and repositories.
It refuses unrecognized binaries and active or uncertain packaged runtime users.
An older launcher without this subcommand can be removed from a built checkout using `target/release/mx launcher-install --uninstall` with its installation directory options.
Do not delete your source checkout or operational home to uninstall the command.

## If something fails

| Symptom | What to do |
| --- | --- |
| `cargo` missing or compiler too old | Install/update stable Rust, reopen the terminal and rerun `./install.sh`. |
| `multplx: command not found` | Add the installer's printed binary directory to `PATH`, then run `multplx paths`. |
| Existing installation refused | Use `./install.sh --upgrade` for the same full installation; uninstall an old source-mode installation before switching modes. |
| Harness missing or unauthenticated | Install and sign into the selected provider CLI, then retry `multplx chat NAME`. |
| Project unavailable | Check `multplx projects list`; repair a moved checkout as described in the command reference. |
| Upgrade/uninstall says runtime is in use | Stop or reconcile the named owner; do not delete its lock to force replacement. |
| Runtime trouble after setup | Run `multplx doctor` and follow its specific findings. |

## Advanced: prebuilt packages

If a release distributor supplies a platform archive and matching checksum, verify and extract that archive, then run its bundled `bin/mx launcher-install --package /path/to/extracted-package`.
The normal source installation above does this packaging automatically.
Do not guess archive names or assume a public prebuilt release exists.
