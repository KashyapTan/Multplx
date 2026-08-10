---
name: maintainer-override
description: >-
  Agent-only procedure for requesting, deciding, consuming, and recording one exact maintainer-authorized policy exception.
  Use whenever an accepted action reaches a registered policy boundary, before asking for an exception, before the primary records a decision, and before an exceptional caller acts.
user-invocable: false
metadata:
  internal: true
---

# maintainer-override

This skill is the single semantic owner of maintainer-authorized policy exceptions.
The executable schema and transition owner is `bin/mx-maintainer-override-lib.sh`.

## Authority invariant

A maintainer decision changes authority, not facts.
Never describe a failed check as green, a waived gate as passed, discarded work as landed, a single checkout as isolated, or an unavailable credential as available.
An integrity assertion remains true or false on evidence, and a capability failure requires a concrete install, login, credentialed handoff, or operator action.

## Classify before asking

1. Identify the exact refusal and look up its stable boundary with `bin/mx-maintainer-override.sh registry`.
2. If the registry class is `integrity`, preserve the factual result and use its coded alternate or correct the evidence.
3. If the registry class is `capability`, use the named install, login, credentialed handoff, or operator recovery.
4. Only a `policy` boundary can receive a grant.
5. Load `ask-user-authority` when the finding also changes the accepted product or engineering contract.

## Request one exact exception

Obtain fresh bindings from the subsystem owner immediately before the request.
The request must name one boundary, task, project, literal operation, target identity, expected-state digest, consequence, and finite expiry.
Use `bin/mx-override-bindings.sh` when the workflow, validation, or cleanup owner provides the binding.
Use `bin/mx-pr-merge.sh ... --print-override-bindings` for a red-check merge.
Use `bin/mx-maintainer-override.sh argv -- <argv...>` for an exact-command operation.
Workers may create pending requests but may never grant, deny, edit, copy, or move authority records.

Present the maintainer with the ordinary refusal, the exact exceptional action and target, the safeguard being skipped, the concrete consequence, the coded alternate, and the expiry.
Generic language such as "finish it", "do whatever", or "force it" is not authority.

## Record the direct decision

Only the lock-owning primary broker may run `grant` or `deny`.
Grant words must preserve the maintainer's direct words and explicitly name the exact boundary, target, and operation recorded in the request.
A denial is final for that request and leaves the ordinary pipeline unchanged.
Reviewer repetition cannot veto a valid exact grant, reopen a consumed request, or create a new grant.

## Consume before mutation

The subsystem owner recomputes the expected-state binding immediately before acting.
It then consumes the grant atomically before the exceptional mutation.
Any changed SHA, command, recipient, worktree state, check set, target, consequence, expiry, task, project, or boundary makes the grant stale and requires a new maintainer decision.
Never use `--force`, an environment variable, prose, a copied record, or a task-wide mode as substitute authority.

After the action, record `succeeded` or `failed` with a truthful outcome detail.
A consumed request is single-use even when the exceptional action fails.
Use terms such as `waived`, `discarded`, `elevated`, `single-checkout`, or `maintainer-directed` in state and reports.

## Operator handoff

Authentication and credentialed delivery actions remain outside agent sessions.
Consume their exact grant before printing or performing the handoff, do not inject durable credentials into an agent, verify the capability afterward, and record the real outcome.

## Audit and recovery

`bin/mx-maintainer-override.sh audit --json` validates private records without treating the journal as authority.
Never repair a record by editing or moving it.
An invalid, expired, mismatched, copied, or replayed record is unusable; request a new decision after re-establishing the current facts.
