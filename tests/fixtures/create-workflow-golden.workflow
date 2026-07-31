---
workflow_version: 1
name: support-fix
description: Agree on a reproduction, implement a fix, and run the focused check.
stages:
  - id: reproduce
    title: Agree on the reproduction
    type: interactive
    gate: approve
    output: data/{run}/reproduction.md
  - id: implement
    title: Implement the fix
    type: agent
    executor: actor
    fresh_session: true
    brief_from: [reproduce]
    gate: auto
    contract: local-commits
  - id: verify
    title: Run the focused check
    type: command
    gate: auto
    run: ./tests/focused.test.sh
---

## reproduce

Work with the maintainer to reproduce {input}.
Record the accepted reproduction in {output}.

## implement

Read the inherited reproduction and implement the smallest complete fix.
Commit the fix locally and never push.

## verify

Run the tracked deterministic focused check.
