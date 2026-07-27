# Porting firstmate → Computer

This document is the execution guide for porting the upstream `firstmate` project (vendored at `firstmate/`) into **Computer** — a standalone, customized orchestrator per the vision in `CLAUDE.md`. It sequences the eleven plan artifacts in `plans/`, explains how to port safely, and defines the testing discipline for every phase.

**Sources of truth:**
- `UPDATE_PLAN.md` — the architecture-review change spec each plan derives from.
- `firstmate_dependencies.md` — the external-dependency catalog.
- `plans/*.html` — one detailed, self-contained implementation plan per change (open `plans/index.html` for the overview).

**Vocabulary:** target designs use the new role terms — **maintainer** (the human), **broker** (orchestrator agent), **actor** (task agent), **daemon** (persistent domain agent). The old terms (captain/crewmate/first mate/secondmate/fleet) appear only when describing upstream firstmate. All new scripts use the `mx-` prefix (locked in by UPDATE_PLAN §8.6).

---

## 0. Porting model: how the port physically happens

Recommended approach: **evolve a copy, don't edit the reference.**

1. Keep `firstmate/` as the pristine upstream reference (read-only). It stays useful for diffing behavior and re-reading original implementations throughout the port.
2. Create the Computer source tree at the repo root (`bin/`, `tests/`, `docs/`, `skills/`, `config/`) by copying `firstmate/` content in Phase 0, then applying each plan as ordered commits against that copy.
3. Every phase lands as one or more commits with the full behavior suite green before and after (see §Testing discipline). Never batch two plans into one commit — bisectability is the point.
4. When the port is complete and Computer is what you actually run, the `firstmate/` folder is **removed** (decided — see the Decisions record, §5.8). The end state is a single Computer tree at the repo root with no vendored upstream copy.

### Phase 0 — Bootstrap the Computer tree (before Plan 01)

- Copy `firstmate/` → repo root (excluding `.git`): `bin/`, `tests/`, `docs/`, `config/`, `skills/`, `.agents/`, `AGENTS.md`, harness config dirs (`.claude/`, `.codex/`, etc.).
- Run the full suite unmodified to establish the green baseline:
  ```
  bin/fm-test-run.sh --all
  ```
  Record the result (total / failed / gate-skipped counts). Gate-skips for backends you don't have installed (herdr, zellij, cmux) are expected and fine — what matters is that this baseline is reproducible, because it's the reference every later phase is measured against.
- Backend/harness posture (decided, §5.6): Computer keeps backends **tmux, cmux, herdr** and harnesses **claude, codex, pi**. The zellij/orca backends and grok/opencode harness support are deleted in plan 01. During the Phase-0 baseline run, tests for the doomed backends/harnesses may gate-skip or pass — either is fine; they're removed from the inventory in phase 1.

---

## 1. Execution order and rationale

Execute the plans in numeric order. Each plan document contains the full design, file-impact table, step list, and test plan — this section only explains *why* this order.

| # | Plan | Covers | Why here |
|---|------|--------|----------|
| 01 | [01-deletions.html](01-deletions.html) | §8.1 deletions + pruning | Pure removal (X-mode relay, glab, shellcheck installer/gate, wedge alarm) plus the decided backend/harness pruning (drop zellij, orca, grok, opencode). Do first per UPDATE_PLAN §8.7 — every later phase touches a smaller tree. |
| 02 | [02-rebrand.html](02-rebrand.html) | §7 rebrand | Rename before building anything new, so every script/test/doc created in phases 3–7 is born with `mx-` names and the new vocabulary — nothing gets built twice. |
| 03 | [03-status-reporting.html](03-status-reporting.html) | §3 validated reporting | First functional change. `mx-report` + the MCP `report_status` tool are foundations plans 04 and 05 build on. Additive — reconciliation logic untouched. |
| 04 | [04-watcher-nudge.html](04-watcher-nudge.html) | §4 fast-path nudge | Small extension of `mx-report` (plan 03). Durable write always; signal/FIFO nudge only if a live watcher is listening. |
| 05 | [05-signal-precedence.html](05-signal-precedence.html) | §5 classifier precedence | Needs plan 03's "schema-validated self-report" tier to exist before the precedence rule (native event > validated self-report > regex heuristic) is meaningful. |
| 06 | [06-worktree-provider.html](06-worktree-provider.html) | §1 treehouse | Decided: keep treehouse (Path K). This phase is now just the rebrand rename of the installer/probes; the build-our-own material stays in the plan as reference only. |
| 07 | [07-axi-replacements.html](07-axi-replacements.html) | §8.2 -axi toolbelt | gh-axi→`gh` swap (low effort, do first within the phase), then the in-repo backlog lib and headroom check. The `gh`-based merge path is a prerequisite for plan 09's push service. |
| 08 | [08-vplan.html](08-vplan.html) | §8.5 vplan | Standalone HTML plan-artifact module replacing lavish-axi. Independent of everything else — can run parallel to 07 if desired. |
| 09 | [09-least-privilege-push.html](09-least-privilege-push.html) | §2 least-privilege push | The delivery seam plan 10 hands off to. Needs plan 07's direct-`gh` tooling. Actors lose push credentials; a separate credentialed service owns push/PR-open. |
| 10 | [10-deep-review-gate.html](10-deep-review-gate.html) | §6 + §8.6 deep-review | The flagship gate. Composes worktrees (06), escalation via validated statuses (03), delivery handoff (09), and the harness-adapter seam. |
| 11 | [11-workflow-engine.html](11-workflow-engine.html) | Workflow engine (new feature, maintainer request 2026-07-26) | Last because it composes everything: one shared `mx-workflow.sh` engine runs user-defined multi-stage workflow definitions (`workflows/*.workflow.md`), reusing plan 03's validated statuses, plan 10's headless harness primitive and gate, and plan 09's delivery. Includes the `create-workflow` skill. Its end-to-end proof doubles as the whole port's integration test. |

Parallelization notes: 03/04/05 are a strict chain; 07 and 08 are independent of each other; 08 can be built any time after 02; 11 is strictly last. Everything else respects the numeric order.

---

## 2. How to port properly (working rules)

These apply to every phase:

1. **Read the plan document end-to-end first**, then read the actual firstmate scripts it touches (per `CLAUDE.md`: read more than less). The plans cite real files — verify against the tree before editing, since upstream may have drifted.
2. **One plan, one branch, ordered commits.** Suggested branch naming: `port/01-deletions`, `port/02-rebrand`, … Merge back to `main` only with the suite green.
3. **Never weaken a kept invariant.** The "What NOT to rebuild" list (UPDATE_PLAN bottom of §5, mirrored in `plans/index.html`) is load-bearing: event-log-not-truth, the reconciliation oracle, the Stop-hook backstop, the zero-token classifier, the wake queue, the watcher lock, the liveness beacon. If a plan's implementation appears to require changing one of these, stop and re-read — it doesn't.
4. **Old names die completely.** After plan 02, a grep for `captain|crew|mate|ship|fleet|fm_|fm-` (case-insensitive) across the Computer tree (excluding `firstmate/` and historical docs) must return zero hits — and stays enforced by a naming test from then on.
5. **New scripts are born `mx-`-prefixed** — never create an `fm-` file after phase 2 (UPDATE_PLAN §8.7).
6. **Security-critical rules are ported verbatim, not re-derived.** The two that must not be "improved" during porting: the default-branch-only trust rule for code-executing config fields (plan 10, §6.3.1), and the no-remote-credentials-in-agent-context rule (plan 09).
7. **Deterministic beats model-judged.** Wherever a plan gives a choice (test pass/fail, lint findings, gate decisions), the exit code / deterministic post-processor decides; the model only proposes. This is the no-mistakes design property the whole port preserves.
8. **Log every deviation.** If reality forces a departure from a plan document, note it in a `Deviations` section appended to that plan's HTML (or a sibling `NN-notes.md`) so the plan set stays truthful.

---

## 3. Testing discipline

### 3.1 The runner

`bin/fm-test-run.sh` (→ `bin/mx-test-run.sh` after plan 02) is the single owner of suite selection, families, lanes, and timing. Key invocations used throughout the port:

```
bin/mx-test-run.sh tests/<subject>.test.sh      # one script — primary local loop
bin/mx-test-run.sh --changed                    # conservative changed-file-informed set
bin/mx-test-run.sh --family <name>              # family-scoped
bin/mx-test-run.sh --all                        # full regression — required at every phase boundary
bin/mx-test-run.sh --check-coverage             # prove lanes still equal the full inventory
bin/fm-lint.sh                                  # shell lint (until plan 01/10 relocate lint into the gate's commands.lint)
```

### 3.2 The invariant at every phase boundary

Before merging any plan's branch:

1. `--all` run is green (same or better than baseline; gate-skips only for uninstalled backends, and never *new* unexplained skips).
2. `--check-coverage` passes — if you added/renamed/deleted test scripts, the runner's family labels, changed-file map, and lane composition (all owned by the runner script) were updated to match.
3. The plan's own new tests exist and pass.
4. The naming test (post-plan-02) passes.

### 3.3 Existing tests: keep / change / delete — per phase

Each plan document carries the authoritative per-file list; summary of the pattern:

- **Plan 01 (deletions):** tests covering deleted subsystems are **deleted with the code** (`fm-x-mode.test.sh`; the glab branches of PR-check tests; the wedge-alarm daemon branch; `fm-lint.test.sh` reduces or moves with the lint relocation; the zellij/orca backend tests, `zellij-test-safety.sh`, and the grok/opencode harness tests per the pruning decision). No test that covers *kept* behavior may be deleted to make the phase pass.
- **Plan 02 (rebrand):** the entire suite is **renamed, not rewritten** — `tests/fm-*.test.sh` → `tests/mx-*.test.sh`, helper vars like `FM_TEST_LIB_SOURCED` renamed, runner family/lane maps updated in the same commit. Behavior assertions must not change in this phase; that's what makes the rename verifiable (green before == green after). Add `tests/mx-naming.test.sh` (greps for leftover old vocabulary/prefixes) as the one *new* test.
- **Plans 03–05 (reporting/nudge/precedence):** existing crew-state/classify/watch tests **must keep passing unmodified** — these changes are additive by design. New tests: `mx-report.test.sh`, `mx-nudge.test.sh`, `mx-signal-precedence.test.sh` (details in each plan). If an existing classifier test fails, the precedence rule was implemented as a replacement instead of an addition — that's a bug in the port, not the test.
- **Plan 06 (worktree):** Path K decided — existing spawn/teardown/worktree tests pass as-is (modulo the plan-02 rename). The Path-B `mx-worktree.test.sh` suite is not built.
- **Plan 07 (-axi):** tests that mocked `gh-axi`/`tasks-axi` binaries (via the `tests/lib.sh` fakebin pattern) are **changed** to mock `gh` / exercise the in-repo backlog lib directly. Bootstrap tests change to assert the -axi probes are *gone*. New: `mx-backlog-lib.test.sh`, `mx-headroom.test.sh`.
- **Plan 08 (vplan):** purely additive. New: `mx-vplan.test.sh` (golden-file render, input validation, output self-containment).
- **Plan 09 (push):** PR-watch tests keep passing; merge tests adapt to the service split. New: `mx-push-service.test.sh` including the two security negatives (no credentials in actor env; refuse to push if the branch head moved past the validated SHA).
- **Plan 10 (deep-review):** upstream no-mistakes-contract tests (`fm-nm-test-contract`, `no-mistakes-required-workflow`, `fm-no-mistakes-ownership`, gate-refuse) are **retired and replaced** by the deep-review suite: `mx-deep-review-lib.test.sh` + `mx-deep-review.test.sh` with mocked headless-harness fixtures. The default-branch config-trust test is mandatory before the gate is ever pointed at a real branch.
- **Plan 11 (workflow engine):** purely additive — reconciliation, decision-hold, spawn, and wake tests must pass unmodified. New: `mx-workflow-lib.test.sh` (schema validation: closed enums, auto-gate-requires-contract, version gating), `mx-workflow.test.sh` (stage-order enforcement, gate/contract semantics, restart-reconstruction, definition-snapshot immutability, and the security test: commands execute only from the launch-time snapshot, never from mid-run artifacts), plus a golden-interview test keeping the `create-workflow` skill's output valid against the schema.

### 3.4 Writing new tests

Follow the house pattern: plain bash, source `tests/lib.sh` (ok/not-ok reporters, temp root, fakebin PATH shims, deterministic git fixtures), colocate as `tests/mx-<subject>.test.sh`, register the file in the runner's family map in the same commit. Mock external processes with fakebin shims — the deep-review tests mock the *harness* (a fake headless agent that returns canned JSON) exactly the way upstream tests mock tmux/treehouse/no-mistakes.

### 3.5 When behavior intentionally changes

If a plan changes functionality an existing test asserts (e.g. brief text now instructs agents to call `mx-report`; merge goes through `gh` instead of `gh-axi`), **change the test in the same commit as the behavior**, and say so in the commit message (`test-change: <file> — <why>`). A behavior suite that's ever knowingly red, or a test silently deleted to get to green, breaks the port's audit trail.

---

## 4. Definition of done (for the whole port)

- All eleven plans landed; `plans/` updated with any deviations.
- `bin/mx-test-run.sh --all` green; `--check-coverage` passes; naming test enforces zero old-vocabulary references (the `firstmate/` allowlist exception disappears with the folder itself).
- No `-axi` binary, no `no-mistakes`, no `glab`, no relay code, no zellij/orca/grok/opencode support anywhere in the Computer tree; `treehouse` kept (pinned, verified) as the worktree provider.
- An actor session demonstrably cannot push (no credentials), and the push service demonstrably refuses unvalidated/stale branches.
- deep-review runs end-to-end on a real sample change with an explicit `--intent`: findings → fix round → ask-user escalation → maintainer decision → validated local branch → credentialed push → remote CI watched. Config read from `.deep-review.yaml` on the default branch.
- The `new-feature` workflow runs end-to-end through `mx-workflow.sh` (ideate → approved spec → fresh-session implement → deep-review with the spec as intent → approved delivery) — this doubles as the whole port's integration proof.
- `AGENTS.md` reads correctly in the new voice (plan 02's checklist), and `README`/docs describe Computer, not firstmate.
- `firstmate/` is deleted from the repo; the Computer tree at the root is the only copy.

---

## 5. Decisions record (maintainer, 2026-07-26)

All open questions from the initial planning pass are resolved. The plan documents carry these as "Decided" callouts.

1. **Treehouse (Plan 06): KEEP.** The §1-vs-§8 contradiction in UPDATE_PLAN resolves in favor of keeping treehouse as-is (pinned v2.0.1, SHA-verified installer). Building our own worktree provider is rejected for the port; the pooled `mx-worktree` design stays in plan 06 as reference only.
2. **Script prefix (Plan 02): keep `mx-`.** Maintainer was open to `cr-` as well; `mx-` stays — already locked into UPDATE_PLAN §8.6, zero churn across the plan set, and `cr-` reads as "code review".
3. **Headroom model (Plan 07): local machine resources + API rate headroom, combined.** A max-concurrent-actors-only cap is rejected as the primary model (a concurrency cap may still be derived from the resource signal). The concrete API-headroom source is the one remaining design detail inside plan 07.
4. **Gate config (Plan 10): `.deep-review.yaml`.** Renamed, not drop-in; no `.no-mistakes.yaml` migration shim needed in a fresh port.
5. **Intent (Plan 10): explicit `--intent` required** for every gated task. The intent-extraction-from-transcript path is not ported; its prompt remains in plan 10 as upstream reference only.
6. **Backends/harnesses (Plan 01 scope, Phase 0 posture): prune.** Backends kept: **tmux, cmux, herdr**. Backends deleted: zellij, orca. Harnesses kept: **claude, codex, pi**. Harnesses deleted: grok, opencode. The deletions fold into plan 01; plans 02 and 05 shrink accordingly.
7. **Status enum (Plan 03): use firstmate's full original vocabulary** — `working|paused|blocked|needs-decision|done|failed|resolved` (verified in `bin/fm-classify-lib.sh`; UPDATE_PLAN §3's five-state enum was incomplete). `mx-report` and the MCP `report_status` tool validate against this full set.
8. **End state: `firstmate/` is removed** once the port is fully done. All renamed/rebranded files live in the root Computer directory — the vendored reference exists only for the duration of the port.
9. **Workflow engine (Plan 11, decided 2026-07-26): one shared engine + declarative definitions, not per-workflow generated scripts.** `bin/mx-workflow.sh` interprets schema-validated `workflows/*.workflow.md` files (YAML frontmatter skeleton, markdown stage bodies); the model only authors definitions (data), never enforcement code. Every stage's gate is configurable (`approve` or `auto`, where auto still requires the output contract to be met); runs take one free-form task input instead of fixed parameters; definitions are repo-tracked and freely editable by any user; the `create-workflow` skill lets the broker author new definitions through a guided interview.
