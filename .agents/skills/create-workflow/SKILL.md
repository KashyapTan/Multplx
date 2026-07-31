---
name: create-workflow
description: Create a repo-tracked Multplx workflow definition when the maintainer asks to make a workflow, create a workflow for a repeatable process, or automate a multi-stage process through the shared workflow engine.
user-invocable: true
metadata:
  internal: true
---

# Create a workflow

Use this procedure only to author declarative `workflows/*.workflow.md` data.
Never generate a per-workflow script or duplicate the engine state machine.
Read [`docs/workflows.md`](../../../docs/workflows.md) in full before drafting because it is the one schema owner.

## Interview

Ask the maintainer for the ordered stages and the free-form purpose of the workflow.
For each stage, establish:

- whether it is a maintainer conversation, a model stage, or a deterministic command;
- whether a model stage runs through the broker's headless adapter or a spawned actor;
- whether a spawned actor must use a fresh session;
- which prior artifacts it needs;
- which non-empty file, local commit, or command exit status proves completion;
- whether it advances automatically or waits for maintainer approval.

Keep version 1 linear.
If the proposed process needs a branch, parallel group, loop, include, or sub-workflow, put that judgment inside one stage or split the process into separate workflows.
Do not invent schema fields to encode it.

## Draft

Choose a privacy-safe lowercase name and write `workflows/<name>.workflow.md`.
Write only documented frontmatter fields.
Write one `## <stage-id>` body per stage in the maintainer's described working voice.
Use `{input}` for the launch task, `{run}` for the run identity, and `{output}` only when that stage declares an output.
Never interpolate `{input}` into a command.
Keep remote delivery outside agent context and compose the existing deep-review and delivery entrypoints instead of re-expressing them.

## Validate and review

Run:

```sh
bin/mx-workflow.sh validate workflows/<name>.workflow.md
```

Fix every schema error before presenting the draft.
Walk the maintainer through stage order, executor choices, contracts, approval points, and command trust.
The maintainer must review every `run:` field before the definition's first launch.
Offer a no-side-effect rendering with:

```sh
bin/mx-workflow.sh dry-run <name> --input "<representative task>"
```

Do not launch the workflow unless the maintainer asks to run it.
