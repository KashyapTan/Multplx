# Native session-start nudge

`AGENTS-PORTING.md` section 3 is the authoritative behavioral contract during the Rust port, and the exact root `AGENTS.md` filename resumes that role only after the final restoration gate.
The tracked native adapters inject one instruction and never run the digest, acquire the lock, perform bootstrap work, drain notifications, or arm supervision themselves.
The payload starts with U+2063 and the stable `MULTPLX_OP: ` label, carries the current `session-start` protocol kind, and retains exactly ``Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.`` as its body.
The Recap skill owns the rule that this marked operational input is never a maintainer-authored session boundary, including its narrow legacy compatibility cases.

## Shared wrapper and safety

`bin/mx-sessionstart-nudge.sh` is the single command every harness adapter invokes and enters the Rust Portion 09 runtime by default.
The Rust handler reuses the typed deep-review refusal, primary-scope, process-ancestry, and operational-input owners and stays silent whenever a gate agent carries the `DEEP_REVIEW_GATE` marker.
The explicit legacy selector retains `bin/mx-gate-refuse-lib.sh` and `bin/mx-primary-scope-lib.sh` only for differential rollback, so both implementations use the same behavioral contracts.
The Shared Predicate section of [`turnend-guard.md`](turnend-guard.md#shared-predicate) owns marker validation, plain-checkout detection, and required Multplx-shaped paths.

Before printing, the wrapper reads `state/.lock` and walks at most eight parents from its own pid, matching `bin/mx-lock.sh` and Pi's `lockOwnership()` ancestry depth.
If the lock names a live pid in that ancestry, session start already ran in this harness session and the wrapper stays silent.
Every path exits 0, including malformed state and adapter errors, because a Claude SessionStart exit 2 blocks session initialization.

## Harness transports

| Harness | Tracked transport | Current compatibility |
| --- | --- | --- |
| Claude | `.claude/settings.json` registers `SessionStart` for `startup`, `resume`, and `clear`, excludes `compact`, and invokes the wrapper through `CLAUDE_PROJECT_DIR`. | Native stdout context injection is supported. |
| Codex | `.codex/hooks.json` anchors to the hook process working directory, verifies a Multplx-shaped hook-bearing root, and executes the wrapper. | Native stdout context injection is supported. |
| Cursor | `.cursor/hooks.json` registers `sessionStart` through `bin/mx-cursor-hook.sh`, while the always-applied project rule independently requires the same idempotent session-start command. | Native `additional_context` delivery is verified on Cursor CLI `2026.08.04-aaa8809`; startup correctness does not depend on hook timing alone. |
| Pi | `.pi/extensions/mx-primary-turnend-guard.ts` handles `session_start` reasons `startup`, `new`, and `resume`, then injects the wrapper output with `pi.sendMessage`. | The custom message reaches model context without racing an initial positional prompt. |

## Regression coverage

`tests/mx-sessionstart-nudge.test.sh` proves wrapper silence for both gate signals, an unmarked linked worktree, a missing state directory, and an already-owned lock.
It proves exact U+2063 `MULTPLX_OP:`-prefixed, `session-start`-typed one-line output for a plain primary and a marked linked daemon primary.
It also verifies tracked wrapper registration for Claude, Codex, Cursor, and Pi.
`tests/mx-cursor-adapter.test.sh` covers Cursor's transport translation and `tests/mx-cursor-live-e2e.test.sh` owns the installed-version authentication and surface check.
`tests/mx-maintainer-translation-contract.test.sh` proves Recap's current marker rule, narrow legacy compatibility exclusions, genuine maintainer-message near misses, and the shared marker on supported user-role operational injections.
`tests/mx-pi-primary-live-e2e.test.sh` exercises the native startup path with first-message and later-message Recap regressions.
`tests/mx-turnend-guard.test.sh`, `tests/mx-pi-watch-extension.test.sh`, and `tests/mx-daemon.test.sh` cover marked guard, monitoring, and away-mode delivery.

[`verification/supervision.md`](verification/supervision.md#native-session-start-delivery) records the active version-scoped transport evidence.
