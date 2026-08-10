---
name: stuck-actor-recovery
description: >-
  Agent-only playbook for stuck or missing ordinary Multplx direct reports.
  Use when the session-start digest reports an actor endpoint dead or its metadata has no window, or after a stale wake, looping pane, repeated confusion, an answered-by-brief question, an unresponsive actor, or a failed steer.
  Reconciles recorded work before escalating from targeted inspection through safe relaunch or failure.
user-invocable: false
metadata:
  internal: true
---

# stuck-actor-recovery

Use this playbook when the session-start digest reports an actor endpoint dead or its metadata has no window, or when an actor is stale, looping, repeatedly confused, asking a question its brief already answers, unresponsive, or when a steer failed to land.

Load `harness-adapters` before sending an interrupt, exit command, resume command, or harness-specific skill invocation.
The target window's harness is recorded as `harness=` in `state/<id>.meta`.

## Session-start reconciliation for a dead actor

This procedure covers ordinary `kind=delivery` and `kind=scout` direct reports.
Load `daemon-provisioning` instead for `kind=daemon` recovery.

As step zero, run `bin/mx-doctor.sh --check stateless-sessions` and preserve its evidence before targeted recovery.
Treat the digest's endpoint result as a presence signal, not proof that the task's work or validation run is gone.
Read the targeted current state with `bin/mx-actor-state.sh <id>` before deciding to relaunch.
A deep-review run matched to the actor's branch and current code remains authoritative when the endpoint is dead: handle a terminal or parked run through the normal lifecycle, and keep monitoring an active run instead of creating a duplicate actor.

When no authoritative run accounts for the task, inspect only its recorded backend and worktree inventory.
Use `treehouse status` for treehouse-backed tmux, herdr, or cmux tasks.
Do not sweep another home's endpoints or infer ownership from a matching window label.

Before relaunch, prove that no live agent still owns the recorded task and that the existing worktree remains available.
Preserve its uncommitted changes and commits, keep the same task identity, and resume or relaunch the recorded harness in that existing worktree with the same brief plus a concise progress note.
Do not use a fresh generic spawn while the recorded worktree is unaccounted for, because allocating another worktree can split one task across two copies.
If the worktree or ownership cannot be reconciled safely, leave all state intact and report the task failed or blocked with the conflicting evidence.

## Live-endpoint escalation

Escalate in order:

1. Peek the pane.
2. If the actor is waiting on a question its brief already answers, answer in one line via `MX_HOME=<this-broker-home> bin/mx-send.sh` from an active broker session unless `MX_HOME` is already set to the active Multplx home.
3. If the actor is confused or looping, interrupt with the adapter's interrupt key, then redirect with one corrective line.
   For example, for a single-Escape adapter: `MX_HOME=<this-broker-home> bin/mx-send.sh <window> --key Escape`.
4. If the actor is genuinely wedged after redirection, exit the agent with the adapter's exit command and relaunch with the same brief plus a `progress so far` note appended to it.
   Genuine wedging means looping, unresponsive, repeating the same obstacle, or truly dead.
   A low context reading is not wedging; modern harnesses auto-compact and keep going.
   The worktree and commits persist, so relaunch is cheap.
5. If a second relaunch fails too, write `failed` to the backlog and tell the maintainer the plain failure, preserved work, and consequence using `AGENTS-PORTING.md` section 9 during the Rust port; do not mention metadata, harness, window, or worktree unless the path itself is needed for action.
