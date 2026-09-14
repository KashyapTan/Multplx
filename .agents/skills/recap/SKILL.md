---
name: recap
description: Optionally summarize visible session events and unanswered questions when requested.
user-invocable: true
metadata:
  internal: true
---

# Recap

Summarize visible events since the previous real human message, including still-unanswered questions visible earlier in the conversation.
Operational injections are not human messages; use the current typed prefix and narrow legacy recognition owned by [operational input](../../../crates/multplx-domain/src/operational_input.rs).
An unrelated later message does not answer an earlier question.
Do not infer current live state from old events or reconstruct unavailable history after compaction.
If this is the first real human message, [catchup](../catchup/SKILL.md) can retrieve current state.
Otherwise use the visible conversation without a new state scan or mandatory report artifact.
No fixed headings, salutations or empty-state wording are required.
