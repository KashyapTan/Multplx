# Actionable wake supervision implementation plan

The accepted outcome is normal end-turn-and-wait supervision across Codex CLI, Claude, Pi and Cursor, with automatic parent notifications only for meaningful changes.
Background programs may continue bounded monitoring; the main agent inspects on user input, actionable events or concrete recovery needs.

## Ownership and implementation

The shared implementer owns report acceptance, durable notification classification, parent routing and common watcher triage.
The harness implementer owns session protocols, Codex idle delivery, Pi extension behavior, provider guidance and empirical adapter evidence.
The shared implementer integrates both commits and owns release validation, publication and CI through green.
The independent reviewer checks the complete contract and failure paths.

A small pure classifier in the existing core classification module defines routine progress versus actionable notification.
An optional backward-compatible envelope field freezes the accepted automatic-notification decision before durable report publication.
Absent classification remains actionable for historical envelopes and unknown messages.
Message envelopes, status history, accepted evidence and parent receipts remain durable even when no wake is appended.
No suppressed report creates a wake disposition or acknowledgement obligation; its pending message transport acknowledgement remains distinct from a handled wake.
Direct report publication, interrupted-publication repair, nested parent delivery and status-file ingestion honor the same frozen decision.

## Lifecycle acceptance matrix

| Input | Automatic notification | Durable result |
| --- | --- | --- |
| First accepted start for current attempt/revision | Yes, once per message identity | Status, envelope, evidence and route |
| Repeated unrequested working progress or healthy heartbeat | No | Complete status and evidence history |
| Requested acknowledgement or informational answer while working | Yes | Exact correlation and disposition, preserving completion/waits |
| Blocker, decision, failure, terminal result or resolution | Yes | Current identity and durable route |
| Changed attempt/revision, resumed work, artifact or delivery milestone | Yes | Revision-bound evidence |
| Unknown input or actual monitoring failure | Yes | Conservative diagnostic |
| Identical publication retry/restart | Existing decision and notification identity | No duplicate or reclassified event |
| Superseded report | Historical only | Retained original identity; no new interpretation |

## Verification and delivery

Focused tests cover report bursts, one-time start, correlated answers, terminal events, interrupted publication, nested delivery and status reinjection.
Watcher and bridge fixtures run multiple background cycles without healthy-idle main notifications.
Provider tests distinguish deterministic fixtures from authenticated live runs and identify missing providers explicitly.
Integration runs Rust formatting, clippy, workspace tests and a release build before the full behavior suite, documentation audience/syntax/inventory checks, shadow diagnostics and the unchanged coverage guard.
Raw logs and JSON artifacts remain outside Git under canonical /private/tmp paths.
The task branch is published as one PR; CI failures are fixed without weakening thresholds, adding retries or skipping required checks.
Only the human merges.
