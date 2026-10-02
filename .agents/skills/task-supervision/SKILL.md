---
name: task-supervision
description: Reconcile task progress, durably handle wakes and preserve blockers when supervising concurrent delegated work or recovering queued events.
metadata:
  internal: true
---

# Supervise current work

Keep the coordinator available for user discussion and new requests while workers execute asynchronously.
Use bounded current-state reads and the emitted harness supervision protocol; an old status line is a notification, not current task truth.
Read `mx task-model inspect TASK_ID` for accepted scope, assignment, attempts and evidence, and `bin/mx-actor-state.sh TASK_ID` for recorded execution state.
`bin/mx-status-snapshot.sh --help` describes the bounded portfolio projection and freshness limits.
Unavailable observations remain unknown rather than becoming healthy or completed state.

Only the verified owner of a home may claim, disposition or acknowledge its wakes.
A lock-refused, read-only or foreign session may inspect state but must not consume another owner's queue.
Use `mx wake --help` for installed mechanics; the essential lifecycle is:

```sh
mx wake claim --limit 10
mx wake disposition EVENT_ID handled --detail 'Recorded the current task outcome'
mx wake ack EVENT_ID
```

Inspect each claimed event's current task before choosing its disposition.
Use `superseded` for obsolete events, `follow-up` with a validated operation receipt for routed work, or `waiting` for a retained condition.
Waiting requires both a named condition and either a trigger or bounded recheck:

```sh
mx wake disposition EVENT_ID waiting --detail 'Awaiting the task decision' --condition 'Current revision receives its answer' --trigger DECISION_ID
mx wake ack EVENT_ID
```

Acknowledging a waiting receipt leaves it unfinished; resume through its trigger and settle it when the condition is resolved.
Printing or claiming an event never acknowledges it.
Use `mx wake list --unfinished` and `mx wake pending` to inspect outstanding obligations; recover ownership only after the prior exact process lifetime is gone.
[Durable coordination](../../../docs/durable-coordination.md) owns dispositions, receipt identity and request handling.

Keep human questions durable and revision-bound through the task reporter's keyed `needs-decision` state.
Inspect evidence, route genuinely missing input and continue unrelated tasks while dependent work remains gated.
Do not take over a blocked worker's deliverable; preserve its work and accepted scope.
Use [subagent-recovery](../subagent-recovery/SKILL.md) for missing endpoints or uncertain ownership and [harness-adapters](../harness-adapters/SKILL.md) for targeted controls.
Each home owns its own queue and children, even when outcomes route through a parent.
Report meaningful outcomes and blockers without narrating unchanged polls.
Explicit away mode uses [afk](../afk/SKILL.md), whose supervisor owns monitoring while active.
