Mode: Unknown harness fallback.
The named `bin/` commands below select the Rust supervision runtime by default.

This primary harness does not have a verified watcher wake adapter.
Follow the generic supervision contract maintained in `AGENTS.md`.
First cycle: drain queued wakes, then choose a supervision wait that the harness can actually wake from.
Ordinary wake: drain and handle the wake, then repeat that verified wait while supervision is still required.
Use `bin/mx-watch-arm.sh` only when the harness has a tracked background mechanism that survives the tool call and notifies the model on process exit.
Use a bounded foreground wait over `bin/mx-watch.sh` when that wake mechanism is not verified.
Never use shell `&` for watcher supervision.
Failure or missing cycle only: inspect the failure and restore the same verified wait shape.

Record new verification evidence before promoting an unknown harness to a named snippet.
