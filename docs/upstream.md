---
upstream_repo: https://github.com/kunchenguid/firstmate
fork_point: 3f71cddd764a49ab71bcd53a46b84e5e7336557a
last_reviewed: 3f71cddd764a49ab71bcd53a46b84e5e7336557a
status: active
retired_reason:
---

# Upstream review record

This file is the authoritative fork point, review cursor, relevance map, retirement state, and completed-review log for the Multplx fork.
[Back to the documentation index](README.md).
The Rust lifecycle command behind `bin/mx-upstream-diff.sh` is the sole supported writer of `last_reviewed`.
The fork point never changes.
The review cursor advances only after a completed upstream-sync workflow run receives its final maintainer approval.
Setting `status` to `retired` requires a concise `retired_reason` and makes the diff command exit with status 3 without fetching or writing.
Retirement is reversible, and reactivation resumes from the preserved review cursor.

The fork point is upstream commit `3f71cddd764a49ab71bcd53a46b84e5e7336557a`, dated 2026-07-25 03:45:10 -0700, with subject `fix(bin): remove vestigial dispatch selector (#1026)`.
The Multplx source tree at the repo root was created on 2026-07-27 by extracting that upstream tree with `git archive`.
The extraction excluded upstream's `CLAUDE.md` symlink because Multplx keeps its own `CLAUDE.md`.
All later divergence is recorded in this repository's history, beginning with plan 01.

## Relevance map

The deterministic diff filter matches each touched upstream path against this ordered table.
The first matching glob wins.
Unmatched paths are `flag`, never an implicit skip.
Every completed review classifies newly flagged paths by adding a narrowly justified row.
The `flag` class is also deliberate for mixed files where path-only matching cannot prove that a changed hunk belongs to a removed subsystem.

<!-- mx-upstream-map:start -->
| Upstream path glob | Class | Multplx counterpart or reason |
| --- | --- | --- |
| `bin/fm-crew-state.sh` | relevant | `bin/mx-actor-state.sh`, the reconciliation oracle, with later additive signal-precedence behavior. |
| `bin/fm-classify-lib.sh` | relevant | `bin/mx-classify-lib.sh`, including Multplx signal precedence. |
| `bin/fm-watch.sh` | relevant | `bin/mx-watch.sh`, the watcher and liveness loop. |
| `bin/fm-watch-arm.sh` | relevant | `bin/mx-watch-arm.sh`. |
| `bin/fm-watch-checkpoint.sh` | relevant | `bin/mx-watch-checkpoint.sh`. |
| `bin/fm-lock.sh` | relevant | `bin/mx-lock.sh`. |
| `bin/fm-lock-lib.sh` | relevant | `bin/mx-lock-lib.sh`. |
| `bin/fm-session-lock-lib.sh` | relevant | `bin/mx-session-lock-lib.sh`. |
| `bin/fm-turnend-guard.sh` | relevant | `bin/mx-turnend-guard.sh`, the Stop-hook backstop. |
| `bin/fm-wake-lib.sh` | relevant | `bin/mx-wake-lib.sh`, the wake queue and PID-identity primitives. |
| `bin/fm-wake-drain.sh` | relevant | `bin/mx-wake-drain.sh`. |
| `bin/fm-spawn.sh` | relevant | `bin/mx-spawn.sh`, with additive Multplx dispatch and isolation behavior. |
| `bin/fm-brief.sh` | relevant | `bin/mx-brief.sh`, with additive Multplx briefing behavior. |
| `bin/fm-send.sh` | relevant | `bin/mx-send.sh`, with additive Multplx routing behavior. |
| `bin/fm-decision-hold.sh` | relevant | `bin/mx-decision-hold.sh`. |
| `bin/fm-install-treehouse.sh` | relevant | `bin/mx-install-treehouse.sh` and its Rust default implementation, whose external pin remains separately reviewed. |
| `tests/fm-crew-state.test.sh` | relevant | Regression coverage for `bin/mx-actor-state.sh`. |
| `tests/fm-watch*.test.sh` | relevant | Watcher and checkpoint regression coverage. |
| `tests/fm-watcher-lock.test.sh` | relevant | Watcher-lock regression coverage. |
| `tests/fm-wake-queue.test.sh` | relevant | Wake-queue regression coverage. |
| `tests/fm-spawn*.test.sh` | relevant | Spawn and isolation regression coverage. |
| `tests/fm-brief.test.sh` | relevant | Brief regression coverage. |
| `tests/fm-send*.test.sh` | relevant | Send-path regression coverage. |
| `tests/fm-decision-hold*.test.sh` | relevant | Decision-hold regression coverage. |
| `tests/fm-turnend-guard.test.sh` | relevant | Turn-end guard regression coverage. |
| `bin/fm-pr-*.sh` | flag | Mixed GitHub and removed GitLab code cannot be classified safely from the path alone. |
| `tests/fm-pr-*.test.sh` | flag | Mixed provider tests require per-change review. |
| `bin/fm-x-*.sh` | deleted | The public social relay was removed in plan 01. |
| `tests/fm-x-*.test.sh` | deleted | The public social relay tests were removed in plan 01. |
| `bin/backends/zellij.sh` | deleted | The Zellij backend was removed in plan 01. |
| `bin/backends/orca.sh` | deleted | The Orca backend was removed in plan 01. |
| `tests/fm-backend-zellij*.test.sh` | deleted | Zellij backend coverage was removed in plan 01. |
| `tests/fm-backend-orca*.test.sh` | deleted | Orca backend coverage was removed in plan 01. |
| `bin/fm-turnend-guard-grok.sh` | deleted | The Grok harness was removed in plan 01. |
| `.grok/*` | deleted | Grok harness configuration was removed in plan 01. |
| `.opencode/*` | deleted | OpenCode harness configuration was removed in plan 01. |
| `tests/fm-grok*.test.sh` | deleted | Grok harness coverage was removed in plan 01. |
| `tests/fm-opencode*.test.sh` | deleted | OpenCode harness coverage was removed in plan 01. |
| `bin/fm-install-shellcheck.sh` | deleted | The standalone installer was removed when lint moved into deep-review. |
| `bin/fm-lint.sh` | deleted | The standalone lint gate was replaced by deep-review. |
| `bin/fm-tasks-axi-lib.sh` | deleted | The external task tool was replaced by the in-repo backlog library. |
| `bin/fm-nm-*.sh` | deleted | The old validation pipeline was replaced by deep-review. |
| `tests/fm-nm-*.test.sh` | deleted | The old validation contract tests were replaced by deep-review coverage. |
| `tests/fm-no-mistakes*.test.sh` | deleted | The old validation ownership tests were replaced by deep-review coverage. |
| `skills/no-mistakes*` | deleted | The old validation workflow was replaced by deep-review. |
<!-- mx-upstream-map:end -->

## Review procedure

Run `bin/mx-workflow.sh run upstream-sync --input "<cadence and date>"` on the maintainer's chosen monthly or quarterly cadence.
An off-cadence run is appropriate when upstream announces a security or safety fix.
The workflow fetches into its run artifact directory, produces a relevance-filtered report, obtains maintainer-reviewed triage, reimplements approved fixes as ordinary Multplx work, and advances the cursor only after final approval.
It never merges, cherry-picks, applies patches, re-vendors upstream, or gives an upstream change a fast path around deep-review.
Each relevant change receives exactly one final triage class: `port` or `skip`.
Unresolved design questions and unmapped paths remain `flag` until the maintainer converts them to `port` or `skip`.

`bin/mx-upstream-diff.sh --out <dir>` uses `last_reviewed` as the range start and writes its clone, report, and `head-sha` only below `<dir>`.
The private clone keeps its fetch URL but pins its push URL to `/dev/null`.
`bin/mx-upstream-diff.sh --since <sha> --out <dir>` overrides the range start without changing this record.
`bin/mx-upstream-diff.sh --record-reviewed <sha-or-head-sha-file>` fetches upstream again or reuses the adjacent private clone, refuses backward or unrelated movement, and atomically updates `last_reviewed` with its completed-review log entry.
`bin/mx-upstream-diff.sh --status` prints the machine header state and exits with status 3 when sync is retired.

## Completed review log

<!-- mx-upstream-log:start -->
_No completed upstream review has been recorded._
<!-- mx-upstream-log:end -->

## Pinned external dependencies

| Dependency | Upstream | Pin | Verification owner |
|---|---|---|---|
| Treehouse worktree provider | https://github.com/kunchenguid/treehouse | `v2.0.1` | `crates/multplx-backend/src/treehouse_tools.rs` owns the Rust-default per-platform SHA-256 table, bounded download, exact post-install version check, and `get --lease` gate; `bin/mx-install-treehouse.sh` retains the public command and legacy rollback. |

Treehouse release review is part of the upstream watch.
A pin bump is an ordinary reviewed change that updates both the version and every platform checksum in the Rust implementation and retained legacy rollback.

## Phase 0 baseline (2026-07-27, macOS; corrected after plan 01)

The initial `--all` baseline run recorded 4 environmental failures, but its
first ~30 scripts ran before per-test monitoring was armed, so several
early-alphabet failures went unrecorded. The post-plan-01 full-suite run
(91 scripts after the plan-01 deletions) produced the complete picture.
**Every failure below reproduces byte-for-byte in the pristine `firstmate/`
checkout** — upstream/macOS-environment issues, not port regressions — except
the one branch-topology case noted last. Gate-skips occur only for backends
and harnesses not installed on this machine (herdr, cmux, live-harness
opt-ins) - expected for the historical baseline recorded in the completed [implementation ledger](../plans/index.html).

| Test | Failing case | Root cause |
|---|---|---|
| `tests/fm-composer-lib.test.sh` | idle placeholder after a `❯` glyph reads `pending` | multibyte glyph strip differs on macOS text tools (upstream CI is Linux) |
| `tests/fm-composer-ghost.test.sh` | glyph/placeholder cases flake under `--jobs` (pass serially) | same macOS glyph-classification family; parallel-mode flake |
| `tests/fm-backend-cmux.test.sh` | ghost placeholder `Type a message...` reads `pending` | same glyph-classification family |
| `tests/fm-brief.test.sh` | `bash -n bin/fm-brief.sh` parse error at line 314 | stock macOS bash 3.2 (heredoc inside `$( )`) |
| `tests/fm-secondmate-safety.test.sh` | brief scaffold failed under FM_HOME | same `fm-brief.sh` bash-3.2 parse error |
| `tests/fm-tangle-guard.test.sh` | brief was not scaffolded | same `fm-brief.sh` bash-3.2 parse error |
| `tests/fm-ask-user-authority.test.sh` | generated brief lets the worker own an ask-user decision | brief generation degraded by the same bash-3.2 issue |
| `tests/fm-session-start.test.sh` | concurrent session-lock acquisition produced 0 winners | bash-3.2 lacks BASHPID; fails identically upstream |
| `tests/fm-afk-launch.test.sh` | interrupted lifecycle resumed or retained its lock | signal/lock timing case; fails identically upstream |
| `tests/fm-backend.test.sh` | old-vs-new conformance (`fm-send --key` log differs) | **branch topology, not environment**: the test rebuilds "old" scripts from `merge-base(HEAD, main)`, which is a docs-only commit until the port branch merges; heals on merge |

Phase 2 intentionally resolves the Bash 3.2 brief parser failures and the
session-lock test's `BASHPID` dependency because no later plan owns them.
It also fixes the runner's first-completed-worker refill regression and makes
that assertion scheduling-safe.
Those resolved entries remain in the historical table above but are no longer
accepted failures for phase-2 validation.

This is the reference every later phase is measured against: a phase is green
when the suite shows **no failures beyond these** and no new unexplained
gate-skips.

### One-time cleanup

The removed grok harness support previously installed a global
`~/.grok/hooks/fm-turn-end.json` hook on operator machines via `fm-spawn.sh`.
That file is inert without this repo's hooks and can be deleted manually.
