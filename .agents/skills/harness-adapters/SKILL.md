---
name: harness-adapters
description: Operational reference for supported harness launch, send, interrupt, resume and status reporting.
user-invocable: false
metadata:
  internal: true
---

# Harness operations

Use the recorded target harness and endpoint for an existing execution, rather than detecting the caller's harness again.
`bin/mx-harness.sh`, `bin/mx-spawn.sh --help`, `bin/mx-send.sh --help` and `bin/mx-peek.sh --help` own command mechanics.
[Configuration](../../../docs/configuration.md) owns legacy dispatch profiles and persistent-home overrides.
Explicit task choices take precedence over defaults; this reference prescribes no model or effort selection policy.
Consult the installed harness's model discovery and help for currently available values.

| Harness | Interrupt | Exit/resume | Notes |
| --- | --- | --- | --- |
| Claude | Escape | `/exit`; use recorded session identity for resume | `/<skill>` invocation; scoped launch disables prompt suggestions. |
| Codex | Escape | `/quit`, then `codex resume <session-id>` | `$<skill>` invocation; the send adapter handles popup settling. |
| Cursor | Ctrl-C | Ctrl-D prints `agent --resume=<session-id>` | Interactive sessions provide stop events; print mode does not. |
| Pi | Escape | `/quit`; inspect supported resume help | Keep a brief as one positional argument, not multiple queued messages. |

These are existing adapter facts, not fresh live verification.
[Runtime evidence](../../../docs/verification/runtime-backends.md), [Cursor evidence](../../../docs/verification/cursor-cli.md) and [supervision evidence](../../../docs/verification/supervision.md) record tested versions and limits.
Inspect an actual trust dialog before sending its required input and verify that the assignment started.
Use recorded endpoints for lifecycle control; a successful transport send alone does not prove work completed.

## Reporting and supervision

Spawn supplies `MX_TASK_ID` and `MX_REPORT_STATE_OVERRIDE`; the latter keeps a persistent child's parent report route separate from its own home.
Use `report_status` when exposed or the absolute `bin/mx-report` fallback in the brief.
Never append raw status-file lines.
[Supervision protocols](../../../docs/supervision-protocols/) and [turn-end guards](../../../docs/turnend-guard.md) own the harness-specific wait and repair paths.
Claude uses Stop-owned auto-arm, Codex and Cursor use bounded checkpoints, and Pi uses its tracked watcher extension.
Use one home-scoped monitoring owner; retain queue entries and reconcile current state after notifications.

[The redesign](../../../porting.md#sub-agent-model-and-communication) permits native delegation, and the retired delegation-hook executable is now an allowing compatibility entry.
Provider child events use `bin/mx-native-observe.sh`; current adapters record them as session-bound because an identifier alone does not prove resumability. Missing observation never becomes a reason to deny delegation.
The named deep-review tool remains optional; its current headless adapter mechanics and unsupported combinations belong to `bin/mx-deep-review.sh --help` and [delivery documentation](../../../docs/delivery.md).
