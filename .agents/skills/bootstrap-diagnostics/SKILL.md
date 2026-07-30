---
name: bootstrap-diagnostics
description: >-
  Agent-only handling playbook for session-start bootstrap diagnostics.
  Use whenever the session-start digest's bootstrap section prints an actionable diagnostic line - MISSING, MISSING_MANUAL, BACKEND_INVALID, HEADROOM_INVALID, VPLAN_INVALID, TANGLE, ACTOR_DISPATCH invalid, SYSTEM_SYNC, PR_CHECK_MIGRATION, DAEMON_SYNC, DAEMON_LIVENESS, or NUDGE_DAEMONS - or when a standalone bin/mx-bootstrap.sh run prints one of those lines.
  A silent bootstrap section, or a BOOTSTRAP_INFO fact, means no skill load.
user-invocable: false
metadata:
  internal: true
---

# bootstrap-diagnostics

Handle each printed line as below, before dispatching work that depends on it.
The line formats themselves are owned by `bin/mx-bootstrap.sh`'s header; this playbook owns the response to actionable lines.
The inline rules in `AGENTS.md` section 3 still bind: detect, then consent, then install - never install anything the maintainer has not approved in this session - and no work is dispatched until the tools it needs are present.
When any diagnostic needs maintainer attention, report the plain consequence and requested action using `AGENTS.md` section 9's maintainer-facing translation contract; do not name the diagnostic label unless the maintainer needs to paste it into a command or issue.

- `MISSING: <tool> (install: <command>)` - list the missing tools to the maintainer with a one-line purpose each plus the printed install commands, wait for consent (one approval may cover the list), then run `bin/mx-bootstrap.sh install <approved tools...>`.
  For `treehouse`, this also covers an installed version whose `treehouse get` lacks `--lease`; treat it as an upgrade request.
  The deep-review gate is in-repo and has no external version probe.
- `MISSING_MANUAL: <tool> (instructions: <url>)` - tell the maintainer why the tool is required and give them the printed instructions URL, but do not pass the tool to `bin/mx-bootstrap.sh install`; wait for the maintainer to complete the manual installation, then rerun session start to confirm the dependency is present.
- `BACKEND_INVALID: <name> (known: <names>)` - the resolved runtime backend has no verified dependency or lifecycle contract, so do not dispatch work until the invalid `MX_BACKEND` or `config/backend` value is corrected to one of the listed backends.
- `HEADROOM_INVALID: <reason>` - the owned composite capacity check could not establish a trustworthy local-resource signal, configured API budget, candidate set, or valid JSON result.
  Do not dispatch or bypass the check; correct the named signal or configuration and rerun bootstrap.
- `VPLAN_INVALID: <reason>` - the bundled review CLI, server, template, SDK, or pinned Mermaid asset failed its integrity self-check.
  Do not start a visual review until the tracked module is restored and bootstrap passes.
- `TANGLE: <remediation>` - the primary checkout is stranded on a feature branch instead of its default branch; `AGENTS.md` section 8 explains why this guard exists and what it protects.
  The work is safe on that branch ref; restore the primary to its default branch with the printed `git -C <root> checkout <default>`, then re-validate that branch in a proper worktree.
  This is the only sanctioned broker-initiated git write to the primary, and it is a non-destructive branch switch that strands nothing.
- `ACTOR_DISPATCH: invalid config/actor-dispatch.json - <reason>` - the optional dispatch profile file exists but failed low-cost bootstrap validation; stop profile-based dispatch, report the actionable error, and require correction of the malformed schema, unverified harness name, or invalid harness/effort pair rather than falling back around it or selecting a bad profile.
- `SYSTEM_SYNC: <repo>: skipped: <reason>` - a benign one-off skip (offline, no origin, local-only); bootstrap continued, investigate only if it blocks work.
  A skip can also report the bounded system-refresh timeout (`MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT`, or a system-size-aware default with a 20 second floor); a timeout never blocks startup.
- `SYSTEM_SYNC: <repo>: recovered: <detail>` - the clone had drifted onto a clean detached HEAD holding no unique commits and the sync self-healed it (re-attached the default branch and fast-forwarded); no action needed, it is reported only so the self-heal is visible.
- `SYSTEM_SYNC: <repo>: STUCK: on <state>, N commits behind <base> - needs attention` - the clone is dirty, on a non-default branch, detached with unique commits, or diverged, so the sync left it untouched (never forcing or discarding); it will keep falling behind until you look.
  A loud STUCK, especially a growing N across bootstraps, means that clone needs hands-on attention; dispatch an actor or resolve it before it strands work.
- `PR_CHECK_MIGRATION: canonical polls rebuilt and armed; resume supervision for this home` - the non-executing migration rebuilt canonical task polls from validated metadata, and those polls are already armed.
  Independently verify the private per-task outcome record, then resume the emitted supervision protocol after finishing the session-start wake handling.
- `PR_CHECK_MIGRATION: validated replacement polls armed; resume supervision for this home` - a retry proved canonical publication provenance, metadata identity binding, and single-link integrity for a replacement poll resolving an earlier ambiguous migration outcome.
  Independently verify the private per-task outcome record, then resume the emitted supervision protocol after finishing the session-start wake handling.
- `PR_CHECK_MIGRATION: quarantined polls remain unarmed; review state/.pr-check-migration.log before rearming` - one or more ambiguous or invalid task polls were quarantined without execution and remain unarmed.
  Read the private mode-`0600` per-task outcome record, verify the task's recorded PR independently, and rearm only through `bin/mx-pr-check.sh` with canonical inputs.
- `PR_CHECK_MIGRATION: migration completed safely; resume supervision for this home` - migration crossed the update boundary without rebuilding or quarantining a task poll after pausing the prior watcher.
  Resume the emitted supervision protocol after finishing the session-start wake handling.
- Any other `PR_CHECK_MIGRATION:` refusal means migration did not complete safely, whether because watcher exclusion, a private path, a diagnostic, quarantine validation, or marker publication could not be proved.
  Keep each affected poll unavailable, inspect the named private state path, and do not bypass the migration or execute a quarantined artifact; a completed safe-scan marker allows unrelated authenticated polls to continue while private repair remains pending.
- `DAEMON_SYNC: daemon <id>: skipped: <reason>` - the local-HEAD daemon sync left a live daemon home on its existing checkout because the home was dirty, diverged, unsafe, on the wrong branch, missing the primary target commit, or otherwise not fast-forwardable, or because inherited local-material propagation failed; bootstrap continued, but inspect the reason because the daemon's tracked instructions, inherited settings, or shared maintainer preferences may be stale after a primary update.
- `DAEMON_LIVENESS: daemon <id>: skipped: <reason>|respawn failed after <cause>: <reason>` - the session-start liveness sweep could not guarantee that the registered daemon is running a real agent process.
  Investigate the reason because that daemon is not guaranteed live.
- `NUDGE_DAEMONS: daemon <id>: send failed: <reason>` - the daemon sweep fast-forwarded a running daemon home and its loaded instruction surface (`AGENTS.md`, `bin/`, or `.agents/skills/`) changed, but the deterministic `mx-send.sh mx-<id>` re-read nudge failed.
  Inspect the reason, keep the pending marker under `state/.daemon-nudge-pending/` intact, and rerun session start after the endpoint or metadata issue is fixed so bootstrap can retry the exact same marked send.
  Only when a running watcher needs the cadence transition applied immediately, restart the home-scoped watcher through the emitted harness supervision protocol; bootstrap deliberately never restarts the watcher itself.
