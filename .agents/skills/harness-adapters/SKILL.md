---
name: harness-adapters
description: Control recorded Claude, Codex, Cursor or Pi endpoints when launching, sending, interrupting, resuming or troubleshooting harness supervision.
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
Claude uses Stop-owned auto-arm, Cursor uses its tracked stop-hook park, Codex uses its Stop-owned exact-thread queue bridge when `MX_CODEX_IDLE_CLI=1` and a matched native SessionStart readiness receipt are present and `codex queue` is available, and Pi uses its tracked watcher extension.
Managed Multplx Codex CLI launches set the activation flag automatically; direct terminal CLI launches require explicit `MX_CODEX_IDLE_CLI=1 codex` opt-in.
Inactive sessions retain bounded foreground checkpoints; Codex Desktop event delivery is unverified.
If native hook review was skipped or hooks are disabled, retain that fallback until the provider's native hook trust review and SessionStart registration establish readiness.
The integration does not override disabled hooks or change private trust state.
Primary and worker Codex launches share the same canonical worker-hook CLI bundle, including a fingerprint of the actual owned scripts and runtime executable.
Complete its first native review at primary startup; unchanged standing/nested workers reuse that definition across homes, while owned bundle changes require review again.
Tracked project hooks, historical home hook files and unrelated user hooks keep separate native review; CLI hook-array overrides that collide with the bundle receive a setup error.
Claude/Pi native project or extension trust and Cursor primary workspace consent remain provider-owned; do not infer trust from ordinary permission flags.
A created endpoint proves neither assignment acceptance nor model execution; inspect native readiness or the first actual worker response.
Model flags require provider IDs, not display labels; `GPT-6 Luna` is rejected before allocation with canonical Codex ID guidance.
Unknown custom single-token IDs are preserved without claiming account access.

Unsupported Codex queue versions emit a visible warning and retain the explicit foreground checkpoint fallback.
Use one home-scoped monitoring owner; retain queue entries and reconcile current state after notifications.

All delegation, including nested delegation, uses Multplx-managed agents by default.
Native delegation requires an explicit human request for that scope and is never an automatic fallback when managed spawning fails.
[The redesign](../../../porting.md#sub-agent-model-and-communication) keeps native capability available for explicit human selection; the compatible pre-tool executable checks supported remote merge commands, not human-request authorization.
[Delivery](../../../docs/delivery.md) owns the human-only merge boundary and command-check limitations.
Provider child events use `bin/mx-native-observe.sh`; current adapters record them as session-bound because an identifier alone does not prove resumability. Missing observation never becomes a reason to deny delegation.
The named deep-review tool remains optional; its current headless adapter mechanics and unsupported combinations belong to `bin/mx-deep-review.sh --help` and [delivery documentation](../../../docs/delivery.md).
