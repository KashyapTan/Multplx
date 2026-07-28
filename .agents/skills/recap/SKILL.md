---
name: recap
description: Recap visible session events since the prior real maintainer message plus visibly unanswered maintainer decisions when the maintainer explicitly invokes /recap, with a Catchup fallback when /recap is the session's first real maintainer message.
user-invocable: true
metadata:
  internal: true
---

# recap

Give the maintainer a concise session-only recap without gathering fresh state.

1. Inspect only conversation or session history already visible to the current broker.
2. Find the most recent real maintainer-authored message before the current `/recap` invocation.
   A maintainer boundary is an ordinary user-role message unless it matches one of the narrow operational exclusions below.
   Exclude messages that begin with the current U+2063 `MULTPLX_OP:` injection prefix.
   Exclude legacy bare-marker away-mode injections only when U+2063 is immediately followed by `Supervisor escalate (`.
   Exclude the exact legacy unmarked session-start payload ``Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.``
   Custom-role messages such as Pi's `broker-sessionstart-nudge` are not maintainer messages.
   System, developer, tool, watcher, guard, away-mode, and other injected operational messages are not maintainer messages.
   Never infer maintainer authorship merely because a synthetic message appears in the user-role transcript.
   Do not exclude an ordinary maintainer message merely because it begins with U+2063 followed by other text, contains ASCII `MULTPLX_OP:` without a leading U+2063, quotes or embeds a current operational message after ordinary maintainer text, quotes or mentions the legacy session-start payload, or adds any text to that payload.
   Apply the current exclusion only when U+2063 `MULTPLX_OP:` begins at the first character of the whole message: `Maintainer quote: ` followed by that current prefix is a maintainer boundary.
   Apply the legacy startup exclusion as a literal whole-message match: ``Maintainer quote: Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.`` is a maintainer boundary.
3. If no prior real maintainer message exists, load [`../catchup/SKILL.md`](../catchup/SKILL.md) and follow it exactly.
   Catchup alone owns its gathering, artifact, and response contract.
   Do not restate that contract or combine a session recap with Catchup output.
4. If a prior real maintainer message exists, preserve the ordinary recap interval: recap what happened after that message and before the current invocation.
   Include concrete outcomes, landed work, failures, decisions made, new decisions needed, and work still running only when those events appear in that visible interval.
   Use maintainer-facing outcome language and preserve every full PR URL present in that interval.
5. Additionally inspect the entire session history visible to the current broker before the current invocation for every explicit maintainer decision that remains unanswered, including decisions raised before the ordinary recap boundary.
   A later unrelated maintainer message establishes a recap boundary but does not close an earlier decision.
   Treat a decision as closed only when a later visible response substantively resolves it, chooses an option, declines it, grants or denies the requested approval, or otherwise directly addresses that decision.
   Include every visibly supported open decision once, and deduplicate by the decision's substance when the ordinary interval recap already represents it or its wording differs.
6. The normal recap branch is session-history-only.
   Do not call Catchup, shell commands, system snapshots, status readers, GitHub or browser APIs, tools, or file reads or writes.
   Create no report, persist nothing, and do not guess current live state beyond the last visible event.
7. If no ordinary events occurred after the previous maintainer message but an older visibly open decision exists, report that decision instead of claiming nothing happened.
   If neither ordinary events nor visibly open decisions exist, say directly in one sentence that nothing happened after the previous maintainer message.

The current `/recap` message is outside the recap interval.
A previous `/recap` is a real maintainer message and may be the next interval boundary.
If context compaction makes the prior boundary unavailable, state that the exact session boundary is unavailable and summarize only visibly supported events.
Compacted history supports an open decision only when both its request and its still-unanswered status are visible; report uncertainty instead of reconstructing hidden requests or answers.
Do not silently invoke Catchup unless this is genuinely the first real maintainer message.
