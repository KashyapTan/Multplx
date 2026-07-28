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

## Phase 0 baseline (2026-07-27, macOS)

`bin/fm-test-run.sh --all` against the freshly committed tree: **98 scripts,
4 failed, rest green** (gate-skips only for backends not installed on this
machine, e.g. herdr — expected per `plans/porting.md`).

All 4 failures reproduce byte-for-byte in the pristine `firstmate/` checkout,
i.e. they are upstream/macOS-environment issues, not port regressions:

| Test | Failing case | Root cause |
|---|---|---|
| `tests/fm-composer-lib.test.sh` | idle placeholder after a `❯` glyph reads `pending`, expected `empty` | multibyte glyph strip differs on macOS text tools (upstream CI is Linux) |
| `tests/fm-secondmate-safety.test.sh` | brief scaffold failed under FM_HOME | `bin/fm-brief.sh:314` parse error under stock macOS bash 3.2 (heredoc inside `$( )`) |
| `tests/fm-tangle-guard.test.sh` | brief was not scaffolded | same `fm-brief.sh` bash-3.2 parse error |
| `tests/fm-session-start.test.sh` | concurrent session-lock acquisition produced 0 winners | environment-sensitive concurrency case; fails identically upstream |

This is the reference every later phase is measured against: a phase is green
when the suite shows **no failures beyond these four** and no new unexplained
gate-skips.

### One-time cleanup

The removed grok harness support previously installed a global
`~/.grok/hooks/fm-turn-end.json` hook on operator machines via `fm-spawn.sh`.
That file is inert without this repo's hooks and can be deleted manually.
