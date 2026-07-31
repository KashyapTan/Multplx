---
workflow_version: 1
name: upstream-sync
description: Review upstream changes, reimplement approved fixes, and record the completed review.
stages:
  - id: fetch
    title: Fetch upstream and build the filtered report
    type: command
    gate: auto
    output: data/{run}/upstream/report-input.md
    run: bash "$MX_WORKFLOW_HOME/bin/mx-upstream-diff.sh" --out "$MX_WORKFLOW_HOME/data/{run}/upstream"
  - id: triage
    title: Classify every upstream change
    type: agent
    executor: broker
    brief_from: [fetch]
    gate: auto
    output: data/{run}/triage.md
  - id: review
    title: Review and rule on the proposed triage
    type: interactive
    gate: approve
    output: data/{run}/approved-ports.md
  - id: port
    title: Reimplement the approved fixes
    type: agent
    executor: actor
    fresh_session: true
    brief_from: [fetch, review]
    gate: auto
    output: data/{run}/port-result.md
  - id: record
    title: Approve advancing the upstream review cursor
    type: interactive
    gate: approve
    output: data/{run}/record-approval.md
  - id: advance
    title: Advance the reviewed upstream commit
    type: command
    gate: auto
    run: bash "$MX_WORKFLOW_HOME/bin/mx-upstream-diff.sh" --record-reviewed "$MX_WORKFLOW_HOME/data/{run}/upstream/head-sha"
---

## fetch

Fetch upstream into the run artifact directory and build the deterministic review input.
The command exit code, declared report, and adjacent `head-sha` are the ground truth.
Do not fetch into the Multplx source tree.

## triage

Read the inherited upstream report completely.
Write one entry for every reported commit to {output}.
Each entry must include the full upstream commit, one final proposed class, a one-line reason, every touched path, and any question that needs maintainer judgment.

Use this closed rubric:

- `port` means a bug fix or safety tightening in a mechanism Multplx kept.
- `skip` means a feature, a change confined to a removed subsystem, or a fix to behavior Multplx intentionally replaced.
- `flag` means the desired outcome is unclear, the change touches a mechanism Multplx materially extended, or a touched path lacks a relevance-map entry.

Do not write code, modify the relevance map, or advance the review cursor.
Treat every map default as `flag`, never as an implicit skip.
For a proposed port, identify the Multplx counterpart and the upstream regression coverage that must be reimplemented.
For a proposed skip, make the reason specific enough for the maintainer to audit later.

## review

Review `data/{run}/triage.md` with the maintainer against `data/{run}/upstream/report-input.md`.
Walk through ports, skips, and flags, including every mechanical skip line.
Resolve each flag to `port` or `skip`.
For every newly mapped path, record the maintainer-approved relevance-map row that must be applied before the review cursor advances.
Write {output} as the final approved list.
An empty port list is valid, but the file must still record all skips, all mapping decisions, the reviewed upstream HEAD, and the maintainer's approval.

## port

Read both inherited artifacts and the full upstream report before changing code.
If the approved port list is empty, make no source commit and write {output} with the reviewed HEAD, the statement `approved ports: none`, and the tests inspected.
Otherwise reimplement each approved fix against its Multplx counterpart in Multplx vocabulary.
Never merge, cherry-pick, apply an upstream patch, or restore a removed subsystem.
Reimplement the upstream regression test for every fix.
Keep one local commit per approved upstream fix and cite the upstream commit in that commit message.
Apply approved relevance-map updates in the same ordinary reviewed change.
Run the focused tests and the normal deep-review gate for the exact local commits.
Write {output} with every upstream commit, corresponding local commit, test result, deep-review result, and delivery state.
Do not claim a fix landed merely because it exists in the actor worktree.

## record

Review `data/{run}/port-result.md`, `data/{run}/approved-ports.md`, and `data/{run}/upstream/head-sha`.
Approve cursor advancement only after every approved fix and relevance-map update is present in the maintained Multplx branch through its ordinary delivery path.
Confirm that the triage covers every commit in the deterministic report and that no flag remains unresolved.
Write {output} with the exact upstream HEAD and a concise completed-review log entry only after those conditions hold.
Resolving this approval authorizes the following deterministic command to update `last_reviewed`.

## advance

Advance `docs/upstream.md` only to the exact reviewed `head-sha`.
The diff command re-fetches or reuses its private clone, proves forward ancestry, and refuses unrelated, backward, or retired updates.
