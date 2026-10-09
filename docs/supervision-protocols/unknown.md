Mode: Unknown harness fallback.
The named `bin/` commands below select the Rust supervision runtime by default.

This primary harness does not have a verified watcher wake adapter.
Follow the released operating contract and the emitted home-scoped protocol.
During development, [VISION.md](../../VISION.md) prohibits operational startup; the [operating contract](../../AGENTS.md) is packaged for installed homes.
Claim queued wakes, reconcile current state, record durable disposition and acknowledge only after handling.
Treat unverified native delivery as degraded supervision with a concrete blocker.
Use one bounded foreground wait only for explicit recovery; do not repeat quiet model-authored waits or re-arm commands.
If delivery remains unavailable, retain unfinished wakes and name the next human message or newly verified adapter startup as the resumption trigger.
Use `bin/mx-watch-arm.sh` only when the harness has a tracked background mechanism that survives the tool call and notifies the model on process exit.
A bounded foreground recovery wait over `bin/mx-watch.sh` does not establish turn-ended event delivery.
Never use shell `&` for watcher supervision.
Failure or missing cycle only: inspect the failure, repair its cause and verify delivery before claiming healthy idle.

Record new verification evidence before promoting an unknown harness to a named snippet.
