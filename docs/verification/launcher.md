# Global launcher verification

This maintained record holds current empirical evidence for the global bootstrap, activated shell, and primary-harness launch path.
`multplx --help` and `multplx launcher-install --help` expose the command grammar and exit statuses owned by `multplx-cli::launcher`.
The public `bin/mx-launcher.sh` and `bin/mx-launcher-install.sh` filenames are transport-only adapters.
The Rust launcher owns verified binary publication, root and home records, update, rollback recovery, and uninstall.

## Simple source installer verification

The root `install.sh` builds the locked release, packages matching runtime assets and delegates installation to the existing transactional package installer.
It supports separate installation directories and explicit upgrade; it does not install system dependencies or launch a model.
`install-from-github.sh` clones public `main` into a disposable directory and forwards the same installation options.
The public curl URL requires these new scripts to be published; local-only validation does not establish that the URL is available.

On macOS, actual `./install.sh` and `./install.sh --upgrade` runs with explicit temporary bin/config/data directories both succeed.
The installed command reports version `0.1.0`, a runtime under `data/runtime` and a separate home under `data/home`.
The installed runtime includes `docs/commands.md`.
The new source-install test covers help, deterministic missing-Cargo refusal and a local Git bootstrap fixture with argument forwarding and temporary-clone cleanup; its bootstrap target is a test script, not a live public download.
The source-install and existing release-package behavior suites pass without failures or gates.
Strict Clippy, formatting, shell syntax, documentation links/classification and the complete 134-script coverage inventory pass.

Verification also exposed an installed-wrapper dispatch bug when `MX_RUST_BIN` names the global `multplx` binary and `MX_MULTICALL_EXPLICIT` is absent.
The workflow, deep-review, timeline, headroom, backlog, system-view and system-snapshot transports now set the explicit dispatch marker only for their own exec.
A package regression invokes their help commands against the installed binary with that marker deliberately absent in the caller.
A rebuilt isolated installation passes those checks plus Viz/vplan help, while normal global help, project help and path resolution still work.
No replacement was installed in the user's default global directories during these tests.

## Phase 11 workspace entry

[Phase 11 implementation evidence](../../plans/lean_redesign/phase11-implementation.md) owns the current workspace, discovery, intake, package and connection validation ledger.
The older measurements below establish only their dated shell/bootstrap behavior; they do not establish the new terminal workspace or reconnection guarantees.
The global launcher now separates workspace entry, explicit shell activation and chat connection.
Package validation uses isolated runtime assets and operational homes, and provider version probes remain distinct from a live conversation trial.

## Verification environment

- Date: 2026-08-02.
- Operating system: macOS Darwin arm64.
- Bash: GNU bash 3.2.57.
- Zsh: zsh 5.9.
- Available verified primary harness: Codex CLI 0.146.0.
- Claude Code and Pi were not installed on this host, so their real-binary rows remain opt-in rather than being represented as live proof.

## Deterministic launcher and shell matrix

Command:

```sh
target/release/mx test-run \
  tests/mx-launcher.test.sh \
  tests/mx-launcher-shell.test.sh \
  tests/mx-launcher-live-e2e.test.sh
```

Result:

```text
MX_TEST_SUMMARY total=3 failed=0 skipped_gate=1
```

The deterministic suites use isolated plain checkouts, homes, managed clones, fake harness binaries, and controlled Bash/Zsh startup files.
They cover spaces, Unicode, shell metacharacters, extra lines and NUL bytes in path records, collision refusal, idempotent reinstall, data-preserving uninstall, independently selected adopted roots and homes, linked-worktree refusal, dirty-managed-runtime refusal, exact argv bytes, ambient backend variables, explicit and automatic backend selection, known-live and stale locks, child-only cwd changes, operator delegation, nested activation, user prompt/alias/option/PATH preservation, one-shot Zsh adapter cleanup, and alternate-screen/color/Unicode byte passthrough.

## Available real-harness smoke

Command:

```sh
MX_LAUNCHER_LIVE_E2E=1 \
  target/release/mx test-run tests/mx-launcher-live-e2e.test.sh
```

Result:

```text
ok - live codex launcher smoke: codex-cli 0.146.0
MX_TEST_SUMMARY total=1 failed=0 skipped_gate=0
```

This smoke executes the installed Codex binary through `mx-launch-harness.sh` from an isolated plain Multplx root and home.
It does not claim interactive trust-dialog, hook-delivery, or terminal-backend proof for unavailable Claude or Pi installations.
Run the same opt-in suite on a host with each verified binary installed before updating their live evidence here.

## Static presentation boundary

The Bash adapter appends one left-prompt marker and the Zsh adapter appends one right-prompt marker after sourcing the user's ordinary rc file exactly once.
Both adapters use shell-native prompt escapes and a constant `multplx` title only on supported interactive terminals.
The suites assert that neither adapter registers a per-prompt hook, calls Git, reads state, invokes a Multplx view, or adds more than one shim directory.
The launcher and harness adapter both use `exec`, so no Multplx proxy remains between the terminal, shell, and selected harness.

## Warm-path performance

The local benchmark used 50 measured warm runs after five warmups on macOS 26.5.2 arm64 with the fixture on the local APFS-backed temporary directory.
It paired an interactive Bash `exit` baseline with the same shell entered through `mx-launcher.sh`, and paired a zero-work fake Codex binary with the same binary entered through the already-validated harness adapter.

Results:

- Incremental activation median: 17.828 ms.
- Incremental activation warm p95: 18.521 ms.
- Incremental harness-shim median: 8.752 ms.
- Incremental harness-shim warm p95: 9.176 ms.

Both paths meet Plan 17's 30 ms median and 75 ms p95 activation targets and its 15 ms median and 30 ms p95 harness-shim targets.
The exact commands, summaries, and 50 paired raw samples are stored in [`launcher-performance.json`](launcher-performance.json).
