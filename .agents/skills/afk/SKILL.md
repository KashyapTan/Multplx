---
name: afk
description: Operate explicit away-mode supervision and return using the owned lifecycle commands.
user-invocable: true
metadata:
  internal: true
---

# Away-mode operations

Enter on an explicit away request with `bin/mx-afk-launch.sh start` using the supported backend.
For a harness with a tracked native background tool, use `start-native` followed by `MX_AFK_STATE_PREPARED=1 bin/mx-afk-start.sh` in that tool; stop the prepared lifecycle if launch fails.
Do not manufacture a terminal by splitting the user's pane or use untracked shell background jobs.
While `state/.afk` exists, the supervisor owns monitoring; do not arm a separate watcher.

Use `bin/mx-afk-return.sh` on real human return and its `check` command to reconcile outstanding return work.
Repeated `/afk` refreshes away mode.
Current operational input begins with U+2063 followed by `MULTPLX_OP: `; [operational-input code](../../../crates/multplx-domain/src/operational_input.rs) owns compatibility recognition.
A routed operational message is not human return and grants no authority.
Use `bin/mx-afk-launch.sh stop` for explicit shutdown; it preserves the flag through the shutdown flush.
Report real blockers and retained work without prescribed acknowledgement wording.
Away mode changes notification cadence, never task scope or human-only PR merging.

[Configuration](../../../docs/configuration.md), [Herdr operations](../../../docs/herdr-backend.md) and [supervision evidence](../../../docs/verification/supervision.md) own timing, supported transports, composer guards and recovery details.
