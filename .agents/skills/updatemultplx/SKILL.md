---
name: updatemultplx
description: Self-update a running broker and its daemons to the latest from origin. Use when the maintainer invokes /updatemultplx (e.g. "/updatemultplx", "update broker", "pull the latest broker"). Fast-forwards this Multplx repo's default branch and every daemon home from origin (fast-forward only, never forced, never disruptive), then re-reads AGENTS.md and nudges each updated daemon to do the same, so the whole tree runs the latest bin/ and instructions.
user-invocable: true
metadata:
  internal: true
---

# updatemultplx

Self-update broker in place.
Multplx is its own repo, behind the same deep-review and credentialed-delivery boundary as any project, so new tracked material (`AGENTS.md`, `bin/`, `.agents/skills/`, and public `skills/`) reaches `main` and then sits there until each running broker pulls it.
Only `AGENTS.md`, `bin/`, and `.agents/skills/` are a running broker instruction surface; public `skills/` is installer-facing and is not loaded by broker.
This skill performs that pull for the running main broker and every daemon, without disturbing any in-flight work.

The update is **fast-forward only** - the same sanctioned self-write as the system sync broker already runs.
It never forces, never creates a merge commit, never stashes, and advances a target only on a clean fast-forward; anything dirty, diverged, offline, or on the wrong branch is skipped and reported.
A tracked-files fast-forward leaves the gitignored operational dirs (data/, state/, config/, projects/) untouched, so a daemon's in-flight work is never disrupted.
This touches only the Multplx repo and its own worktrees, never anything under `projects/`.

## What it does

1. **Run the updater:**
   ```sh
   bin/mx-update.sh
   ```
   It fast-forwards this Multplx repo's default branch from origin, then fast-forwards every registered daemon home (each a treehouse worktree of this same repo, leased at a detached HEAD on the default branch) the same way.
   It prints one status line per target (`updated <old>..<new>` / `already current` / `skipped: <reason>`), followed by two action lines that tell you exactly what to do next:
   - `reread-broker: yes|no`
   - `nudge-daemons: mx-<id>...|none`

2. **Re-read AGENTS.md if your own instructions changed.**
   When the updater printed `reread-broker: yes`, the tracked instruction surface (`AGENTS.md`, `bin/`, or `.agents/skills/`) just advanced under you.
   **Read `AGENTS.md` now** to refresh your operating instructions before doing anything else, so you are acting on the new instructions rather than the stale ones you were started with.
   When it printed `reread-broker: no`, nothing changed for you - skip the re-read.

3. **Nudge each updated live daemon.**
   For every target listed on the `nudge-daemons:` line (do nothing when it says `none`), send a one-line re-read nudge so that daemon picks up its new instructions too:
   ```sh
   MX_HOME=<this-broker-home> bin/mx-send.sh <id> 'broker was updated to the latest - please re-read your AGENTS.md to pick up the new instructions.'
   ```
   Include `MX_HOME=<this-broker-home>` unless `MX_HOME` is already set to the active Multplx home.
   This is a gentle steer, not an interruption: the daemon already got a safe tracked-files fast-forward, and the nudge never forces, tears down, or discards its work.
   A daemon that was skipped, already current, or has no live metadata is not on the list and needs no nudge.

4. **Surface plain outcomes to the maintainer.**
   Summarize what landed under `AGENTS.md` section 9 without broker's internal vocabulary: which parts of the system are now on the latest, and which were left as-is and why.
   For example: "Maintainer, broker and both daemons are now on the latest."
   Surface any skipped target whose reason needs the maintainer's attention - for instance a home with its own un-landed changes (diverged) or local edits (dirty), which were left untouched on purpose.

## Safety

- **Fast-forward only.**
  A target that has diverged, is dirty, is offline, or is on a non-default branch is skipped and reported, never forced or stashed.
  Nothing with unlanded work is ever discarded - this is prime directive #3.
- **Only the Multplx repo and its worktrees** are touched, never `projects/`.
  It is the same sanctioned self-write as the system sync.
- **Daemons are never disrupted.**
  A daemon gets a tracked-files fast-forward (safe while it is mid-task, since its work lives in gitignored operational dirs and separate project worktrees) plus a gentle re-read nudge.
  It is never torn down, interrupted, or forced.
