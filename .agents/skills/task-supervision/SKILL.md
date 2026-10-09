---
name: task-supervision
description: Reconcile task progress, durably handle wakes and preserve blockers when supervising concurrent delegated work or recovering queued events.
metadata:
  internal: true
---

# Supervise current work

Supervise the accepted job until the parent validates the full agreed scope, resolves gaps and delivers it; a worker done claim still permits guidance, corrections and revision requests.
After verified delivery, stop automatic contact, polling, nudging and work routing; retain availability for explicit user-directed follow-up as required by [AGENTS.md](../../../AGENTS.md#9-persistent-coordinators-and-parent-reporting).
Preserve task and accepted-revision evidence when scope changes.

Keep the coordinator available for user discussion and new requests while workers execute asynchronously.
Follow the emitted harness protocol to end the turn and wait while supervision is healthy.
Do not run repeating task/Git/capture/checkpoint inspections or timed sleeps as the normal idle workflow.
Inspect bounded current state when the user asks, an actionable event arrives or concrete recovery requires it; an old status line is a notification, not current task truth.
Background programs own continuous monitoring without a model turn.
Read `mx task-model inspect TASK_ID --compact` for current owner/child-state routes, attempt/revision, endpoint/allocation, pending request IDs and expected delivery commit, and `bin/mx-actor-state.sh TASK_ID` for recorded execution state.
Use `--full` when the accepted scope or retained history itself needs inspection; avoid repeating full embedded briefs for ordinary progress checks.
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
