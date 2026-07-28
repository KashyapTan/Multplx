# Upstream fork point

Multplx is a port of the upstream **firstmate** project. This file is the durable
fork-point record required by plan 14 (upstream sync) — it must exist before the
vendored `firstmate/` reference copy is deleted at the end of the port.

| Field | Value |
|---|---|
| Upstream repository | https://github.com/kunchenguid/firstmate |
| Fork-point commit | `3f71cddd764a49ab71bcd53a46b84e5e7336557a` |
| Fork-point commit date | 2026-07-25 03:45:10 -0700 |
| Fork-point commit subject | fix(bin): remove vestigial dispatch selector (#1026) |
| Vendored at | `firstmate/` (read-only reference for the duration of the port) |
| Multplx tree bootstrapped | 2026-07-27 (Phase 0, exact copy of the upstream tree at the fork point) |

The Multplx source tree at the repo root was created by extracting the upstream
tree at the fork-point commit (`git archive`), excluding upstream's `CLAUDE.md`
symlink (Multplx keeps its own `CLAUDE.md`). All subsequent divergence is
recorded in this repo's own history, starting with plan 01 (deletions).

## Phase 0 baseline (2026-07-27, macOS; corrected after plan 01)

The initial `--all` baseline run recorded 4 environmental failures, but its
first ~30 scripts ran before per-test monitoring was armed, so several
early-alphabet failures went unrecorded. The post-plan-01 full-suite run
(91 scripts after the plan-01 deletions) produced the complete picture.
**Every failure below reproduces byte-for-byte in the pristine `firstmate/`
checkout** — upstream/macOS-environment issues, not port regressions — except
the one branch-topology case noted last. Gate-skips occur only for backends
and harnesses not installed on this machine (herdr, cmux, live-harness
opt-ins) — expected per `plans/porting.md`.

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

This is the reference every later phase is measured against: a phase is green
when the suite shows **no failures beyond these** and no new unexplained
gate-skips.

### One-time cleanup

The removed grok harness support previously installed a global
`~/.grok/hooks/fm-turn-end.json` hook on operator machines via `fm-spawn.sh`.
That file is inert without this repo's hooks and can be deleted manually.
