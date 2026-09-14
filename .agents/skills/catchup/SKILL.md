---
name: catchup
description: Summarize current structured state when the user requests a catchup or status report.
user-invocable: true
metadata:
  internal: true
---

# Catchup

Read `bin/mx-status-snapshot.sh` for bounded current state; its help owns live-PR opt-in and output fields.
Fallback readers are `bin/mx-system-snapshot.sh --json` and targeted `bin/mx-actor-state.sh <id>`.
Preserve observation age, omissions, partial homes and unknown states; a historical status line does not establish current truth.
Keep current decisions, queued dependencies, active work, evidence and human-merged outcomes distinct.
Use structured decision records rather than scraping reports for invented open questions.
Summarize what matters to the user in a suitable format with useful artifact and PR links.
Optionally save the report to `data/status-report-<YYYY-MM-DD>.md`; inspect an existing destination before replacing it.
Generating a report does not mutate task state, launch work, tear down a session or merge a PR.
