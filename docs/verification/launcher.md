# Global launcher verification

This maintained record holds current empirical evidence for the global bootstrap, activated shell, and primary-harness launch path.
The command grammar and exit statuses remain owned by `bin/mx-launcher.sh`; path publication and uninstall mechanics remain owned by `bin/mx-launcher-install.sh`.

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
