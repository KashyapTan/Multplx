---
workflow_version: 1
name: new-feature
description: Agree on an approach, write a spec, implement it in a fresh sub-agent, and deliver it.
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
  - id: deliver
    title: Confirm the selected delivery outcome
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

## deliver

Review the implementation evidence and choose the intended local or pull-request outcome for {run}.
Deep-review and vplan run only if they were explicitly requested for this workflow run.
Ordinary implementers may publish a branch or open and update a PR without a review-tool approval.
The stage contract is met only when the selected outcome has written {output}.
