---
workflow_version: 1
name: new-feature
description: Agree on an approach, write a spec, implement it in a fresh actor, validate it, and deliver it.
stages:
  - id: ideate
    title: Agree on the approach
    type: interactive
    gate: approve
    output: data/{run}/approach.md
  - id: spec
    title: Write the implementation spec
    type: agent
    executor: broker
    brief_from: [ideate]
    gate: approve
    output: data/{run}/spec.md
  - id: implement
    title: Implement end to end
    type: agent
    executor: actor
    fresh_session: true
    brief_from: [spec]
    gate: auto
    contract: local-commits
  - id: review
    title: Run deep-review
    type: command
    gate: auto
    run: bash "$MX_WORKFLOW_HOME/bin/mx-deep-review.sh" {run} --intent-file "$MX_WORKFLOW_HOME/data/{run}/spec.md"
  - id: deliver
    title: Approve and perform credentialed delivery
    type: interactive
    gate: approve
    output: state/{run}.delivered
---

## ideate

Work with the maintainer to converge on an implementation approach for:

{input}

Ask clarifying questions, propose concrete alternatives, and push back where the tradeoffs warrant it.
Do not begin the specification until the approach is agreed.
Record the agreed approach in {output}.

## spec

Write a complete end-to-end implementation specification from the inherited approach.
Cover behavior, affected boundaries, failure handling, tests, documentation, and a definition of done.
Do not begin implementation.
Write the specification to {output}.

## implement

Read the inherited specification fully before changing code.
Implement it end to end in the isolated worktree and commit every intended change locally.
Treat a required departure from the specification as a maintainer decision instead of silently improvising.

## review

Run the existing deep-review gate with the approved specification as authoritative intent.
The gate owns its own finding, decision, retry, and validated-handoff lifecycle.

## deliver

Review the exact validated handoff for {run}.
If delivery is approved, record that approval through the existing delivery contract and run `bin/mx-deliver.sh {run}` from a maintainer shell or the credentialed scheduler.
Do not ask the broker or an actor to perform the remote write.
The stage contract is met only when the service has written {output}.
