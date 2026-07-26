# Firstmate Fork: Planned Improvements

Working notes from an architecture review of `kunchenguid/firstmate`. Goal: remove hard dependencies on the upstream author's own tool ecosystem where practical, and tighten status-reporting determinism without discarding what firstmate already does well.

---

## 1. Dependency removal

### treehouse → build our own
Treehouse is a small tool at its core: a pooled set of disposable `git worktree`s, a locked on-disk state file tracking which are claimed, and process/lease-based "in use" detection so a worktree isn't recycled out from under a live task.
- **Easy to reimplement** if we don't need pool reuse: plain `git worktree add`/`remove` per task, no pool at all.
- **Worth real design time** only if we want treehouse's actual value-add: reusing a worktree so `node_modules`/build caches stay warm across tasks. That reuse-vs-safe-reclaim logic (durable leases for secondmates with no live process vs. safe-to-recycle orphans) is the one part not to shortcut.

### no-mistakes → inspect before touching
This is a full Go project (git-proxy remote, pipeline engine, CI monitor, daemon, DB, TUI), not a script. Don't rewrite it.
- **Decision to make**: keep it as an optional dependency (firstmate already degrades gracefully without `tasks-axi`, do the same here), or fork just the pipeline stage config (swap which review/test/lint tools it invokes) while keeping its gate/worktree/CI-watch mechanics intact.

---

## 2. Least-privilege push

Crewmates should never hold remote write credentials, this isn't a workaround for an org policy, it's the correct shape regardless:
- Crewmate commits **locally only** (no remote permission needed, and it's our verifiable "artifact exists" signal for a done task).
- A **separate, non-agent process** with its own service credentials owns the actual `git push` / PR-open step, triggered only after validation passes and/or the captain approves.

---

## 3. Status reporting: enforce the schema at write-time, not after

**Current state (already in firstmate):** `state/<id>.status` is an append-only *event log*, never trusted as current truth. `fm-crew-state.sh` is the reconciliation oracle that re-derives real state by checking it against the actual running validation step and terminal busy-signature. `fm-classify-lib.sh` classifies each event against a documented vocabulary (`done:`, `blocked:`, `needs-decision:`, `failed:`, etc.).

**The actual gap:** that vocabulary is only documented in `brief.md` prose. Nothing validates a status line *before* it's written; a malformed line just gets silently bucketed as an unclassified heartbeat downstream, no escalation, no error back to the model.

**Fix — add validation upstream of the write:**
- **`bin/fm-report` (new script)**: a thin wrapper crewmates call instead of hand-writing to the status file. Validates `--state` against the closed enum (`working|blocked|needs-decision|done|failed`) client-side; on invalid input, writes nothing, exits non-zero, prints the valid options to stderr, so the model sees the failure in its own tool output on the same turn and can retry.
- **MCP `report_status` tool** for harnesses that support MCP: a real JSON-schema-validated tool, the harness itself rejects a malformed call before it ever executes, which is stronger enforcement than a shell wrapper's own checking.
  - Registered in the harness's own MCP config (e.g. `.mcp.json`), so it starts and stops with that harness's session automatically. It is not part of `fm-watch.sh` or the always-on watcher process, entirely separate lifecycle.
  - Per-harness wiring (MCP tool vs. `fm-report` fallback) belongs in `.agents/skills/harness-adapters`, the existing seam for per-harness quirks.
- **`data/<id>/brief.md` (via `fm-brief.sh`)**: update the instruction from "write status lines in this format" to "call `fm-report` / the `report_status` tool instead of writing directly."
- Both paths still write to the same `state/<id>.status` file. This is additive to the existing event-log model, not a replacement, `fm-crew-state.sh` and the reconciliation logic don't change, they just receive better-validated input.

---

## 4. Optional low-latency nudge (not an HTTP daemon)

Rejected idea: a standalone HTTP status-ingest listener. It would reduce latency (avg. ~7.5s against a 15s poll interval) but reintroduces a long-running stateful process, which cuts against firstmate's actual design virtue: everything is reconstructable from disk, kill any process and the next one reconciles with zero daemon state to restore.

**Better fit**: have `fm-report` optionally send a signal (or write to a named pipe) to the watcher's PID as a pure fast-path nudge. Durable write to `state/<id>.status` happens every time regardless; the nudge just wakes `fm-watch.sh` immediately instead of it waiting for its next 15s poll (`FM_POLL`). If nobody's listening, it's a silent no-op and falls back to normal polling. No new persistent daemon, no new restart-proofing burden.

---

## 5. Explicit signal-precedence rule in the classifier

**The problem:** different backends produce different-quality signals about the same crewmate:
- Herdr's native `done`/`blocked` events (already wired in for that backend) ≈ near-ground-truth.
- A schema-validated self-report (from #3) = good, but still self-reported.
- Regex busy-signature matching on pane text (tmux/zellij/orca/cmux, which have no native busy primitive) = weakest, most prone to false positives.

When two of these disagree about one task at the same moment, there's currently no single documented rule for which one wins, it's handled implicitly and inconsistently per backend.

**Fix:** add one explicit precedence order to `fm-classify-lib.sh`: harness-native event > schema-validated self-report > text/regex heuristic. Applied uniformly regardless of which backend a given task happens to be running on.

---

## What NOT to rebuild (already solid in firstmate, verified against the architecture doc)

- Status-log-is-an-event-not-truth + `fm-crew-state.sh` as the reconciliation oracle.
- The Stop-hook turn-end backstop (actively blocks a blind turn-end while work is in flight and no watcher is live, stronger than a passive turn-end detector).
- Herdr's native busy/idle/done/blocked events as a real event source instead of polling.
- The zero-token bash classifier, durable wake-queue, singleton watcher lock, and liveness beacon.