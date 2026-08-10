# Maintainer-scoped exceptions

This document is the human-facing owner for exact, single-use maintainer exceptions.
The semantic procedure lives in [the maintainer-override skill](../.agents/skills/maintainer-override/SKILL.md), while `bin/mx-maintainer-override-lib.sh` owns the executable schema, registry, validation, locking, and transitions.

## Invariant

A maintainer decision changes authority for one action and never changes facts.
A failed gate may become maintainer-waived but not passed, red checks remain red after a maintainer-directed merge, discarded work is not landed, and a single checkout is not isolated.
Integrity and capability failures are therefore inventoried separately from consumable policy exceptions.

There is no generic force flag, environment bypass, standing grant, inherited grant, wildcard boundary, or post-hoc approval.
Every request binds a stable boundary id, task, project, literal operation or canonical argv, target identity, expected-state SHA-256, consequence, request time, and expiry.

## State machine

Private mode-`0700` state lives below `state/maintainer-overrides/` in `pending`, `granted`, `denied`, `consumed`, and `stale` directories.
Records are regular mode-`0600` single-link JSON files and are never sourced as shell.
One transition lock serializes every move, and a request identity must exist in exactly one lifecycle directory.

Workers may create a pending request, but only the lock-owning primary broker may grant or deny it.
Grant wording must name the exact boundary, operation, and target.
The subsystem owner recomputes fresh state and atomically moves a matching grant to `consumed` before the exceptional mutation.
A mismatch or expiry moves the record to `stale`, a denial preserves the ordinary path, and a consumed outcome is immutable after `succeeded` or `failed` is recorded.

Use `bin/mx-maintainer-override.sh registry --json` for the current machine-readable inventory, `audit --json` for validation, and `inspect <request>` for one record.
Never repair, copy, move, or hand-edit an authority record.

## Registered policy alternates

| Boundary | Exact alternate |
|---|---|
| `workflow.skip-stage` | Skip only the bound run stage and preserve every other snapshotted stage. |
| `workflow.reorder-stage` | Move only the bound stage before the bound target in the private run order. |
| `validation.waive-gate` | Create a distinct exact-SHA waived delivery handoff without changing the failed gate. |
| `delivery.merge-red` | Bind the PR URL, head SHA, and failed-check set, then use the credentialed admin merge path. |
| `cleanup.discard-unlanded` | Inventory and discard only the bound task resources through teardown. |
| `project.direct-write` | Run canonical argv from the named Git root and record before and after state for ordinary validation. |
| `isolation.single-checkout` | Record lost isolation, reserve the exact checkout, and release it only during teardown. |
| `session.terminate-owner` | Send `TERM` to the verified bound harness, prove exit, and acquire the ordinary lock. |
| `security.one-action-elevation` | Run only the bound argv once while all other guards remain enabled. |
| `delivery.credentialed-action` | Produce an already-consumed exact operator or credentialed-service handoff. |
| `dependency.install` | Run the exact installer argv and re-check the named command capability. |
| `authentication.login` | Produce an already-consumed exact interactive-login handoff and re-check authentication afterward. |

The command runner first prints fresh bindings with `bin/mx-override-run.sh --print-bindings ... -- <argv>` and later accepts the granted request id with the same arguments.
Authentication and credentialed delivery stay outside agent sessions, so `bin/mx-maintainer-override.sh handoff <request>` prints only an atomically consumed request and leaves its outcome `not-run` until the operator reports the real result.

## Factual boundaries

The registry classifies validation state, object identity, session-lock state, and worktree isolation as integrity results.
Their concrete alternatives are waiver, a new object-bound request, terminate-and-reacquire, or explicit single-checkout mode; none makes the original assertion true.

Missing tools, authentication, credentials, and host permissions are capability results.
Their concrete next actions are an exact verified install, interactive login, credentialed handoff, or operator execution followed by a fresh capability check.

## Subsystem commands

`bin/mx-override-bindings.sh` prints fresh bindings for cleanup, validation, workflow changes, single-checkout launch, and owner termination.
`bin/mx-pr-merge.sh --print-override-bindings` owns red-merge bindings.
`bin/mx-validation-waive.sh`, `bin/mx-workflow.sh`, `bin/mx-teardown.sh`, `bin/mx-spawn.sh`, `bin/mx-lock.sh`, `bin/mx-pr-merge.sh`, and `bin/mx-override-run.sh` each consume immediately before their own exceptional action.

The ordinary command remains the default when no exact grant exists.
Supplying a wrong, stale, denied, expired, consumed, duplicated, or unrelated request cannot widen the command and requires a new maintainer decision for current facts.
