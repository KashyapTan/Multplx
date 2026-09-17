---
workflow_version: 2
name: new-feature
description: Agree on an approach, write a spec, and implement it in a fresh sub-agent.
stages:
  - id: ideate
    title: Agree on the approach
    type: interactive
    gate: approve
    output: data/{run}/approach.md
  - id: spec
    title: Write the implementation spec
    type: agent
    executor: orchestrator-context
    assignment: researcher
    brief_from: [ideate]
    gate: approve
    output: data/{run}/spec.md
  - id: implement
    title: Implement end to end
    type: agent
    executor: sub-agent-session
    assignment: implementer
    fresh_session: true
    brief_from: [spec]
    gate: auto
    output: data/{run}/implementation.md
    contract: local-commits
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
You may delegate bounded work inside this stage, but later stages remain out of scope.
Ordinary publication is part of the implementation outcome when the accepted brief calls for it and needs no review-tool approval.
Never merge a PR or enable auto-merge.
Write {output} with the exact commit, checks, limitations and delivery state.
