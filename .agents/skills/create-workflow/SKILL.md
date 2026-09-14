---
name: create-workflow
description: Author and validate declarative workflows when the user requests a reusable process.
user-invocable: true
metadata:
  internal: true
---

# Create a workflow

Read [`docs/workflows.md`](../../../docs/workflows.md), the current schema owner.
Use the supplied purpose, ordered stages, inputs, outputs and interaction points; clarify only genuinely missing requirements.
Write declarative `workflows/<name>.workflow.md` using supported fields and one stage body per declared stage.
Never generate a per-workflow script or duplicate the engine state machine.
Keep input data out of shell command interpolation.

Validate with `bin/mx-workflow.sh validate workflows/<name>.workflow.md` and preview with `bin/mx-workflow.sh dry-run <name> --input "<representative task>"`.
Show the ordered stages, command effects and explicit interaction points before a requested run.
Preserve selected `fresh_session` requirements and all declared stage outputs.
Do not add deep-review, vplan, approval or credentialed delivery merely because an old template contains them.
Implementation stages belong to sub-agents; discussion, research and planning may use the orchestrator context.
Phase 08 updates legacy executor names and headless coupling; do not invent unsupported schema fields.
