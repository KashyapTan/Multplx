# Firstmate External Dependencies

> A catalog of every **external tool, service, and runtime** that the `firstmate/` project depends on — what each is for, whether it's required or optional, and the key files where it's referenced. Built as a reference for swapping/removing tools when porting into Orca.
>
> All paths are relative to `firstmate/`. The canonical detection/install logic lives in `bin/fm-bootstrap.sh` (see the `install_cmd()` case ~lines 485–497 and the `MISSING:` probes).
>
> **Live defaults for this checkout:** `config/backend` = `herdr`, `config/crew-harness` = `claude`.

---

## Quick reference — what to change

| Category | Tools | Orca's likely stance |
|---|---|---|
| **Session backends** | tmux (default/reference), herdr, zellij, cmux, orca | Pluggable. Pick one; drop the rest. |
| **AI harnesses** | claude (default), codex, grok, pi, opencode | Pluggable. Pick your harness(es). |
| **Companion `-axi` npm tools** | gh-axi, tasks-axi, quota-axi, lavish-axi, chrome-devtools-axi | Firstmate-specific. Prime candidates to replace/remove. |
| **Contribution pipeline** | no-mistakes, shellcheck, GitHub Actions | Only needed to contribute to *firstmate itself*. Droppable for a standalone fork. |
| **Worktree provider** | treehouse | Needed by all backends except `orca` (which owns its own worktrees). |
| **Social relay** | myfirstmate.io (X/Twitter + Discord) | Opt-in, inert by default. Easy to remove. |
| **Core unix utils** | jq, git, curl, python3, node, perl, coreutils | Keep — genuine infrastructure. |

---

## 1. Session / Terminal Backends (pluggable)

Firstmate has a **pluggable backend abstraction** for the "session-provider" layer. The dispatcher `bin/fm-backend.sh` resolves the active backend, sources `bin/backends/<name>.sh`, and routes all session ops (capture, send-keys, kill, busy-state, event push) through generic wrappers.

**Selection precedence:** per-task `--backend` flag > `FM_BACKEND` env var > `config/backend` file > auto-detection > default **tmux**.
**Registry:** `FM_BACKEND_KNOWN="tmux herdr zellij orca cmux"` (`bin/fm-backend.sh:69`).

| Tool | Role | Status | Auto-detect | Worktree provider |
|---|---|---|---|---|
| **tmux** | Terminal multiplexer, session provider | **DEFAULT / reference** | yes (`$TMUX`) | treehouse |
| **herdr** | Session provider w/ native busy-state + event-push stream | experimental (active here) | yes (`HERDR_ENV=1`) | treehouse |
| **zellij** | Terminal multiplexer, session provider | experimental | no (explicit only) | treehouse |
| **orca** | Session provider **+ owns its worktrees** | experimental | no (explicit only) | **self** |
| **cmux** | macOS GUI terminal app, session provider | experimental | yes (`CMUX_WORKSPACE_ID`) | treehouse |
| **codex-app** | Codex Desktop threads — **explicitly NOT a backend** | blocked | — | — |

**Files:**
- Dispatcher/registry: `bin/fm-backend.sh`, `bin/fm-backend-hometag-lib.sh`
- Adapters: `bin/backends/tmux.sh`, `bin/backends/herdr.sh` (~131 KB, largest), `bin/backends/zellij.sh`, `bin/backends/orca.sh`, `bin/backends/cmux.sh`
- herdr helpers (python3): `bin/backends/herdr-eventwait.py`, `bin/backends/herdr-workspace-move.py`
- tmux shared lib: `bin/fm-tmux-lib.sh`
- herdr lifecycle: `bin/fm-install-herdr.sh`, `bin/fm-herdr-lab.sh`, `bin/fm-herdr-session-cleanup.sh`, `bin/fm-herdr-ci-cleanup.sh`, `bin/fm-transition-lib.sh`
- Config: `config/backend`, plus optional `config/herdr-presentation-spaces`, `config/cmux-socket-password`
- Docs: `docs/{tmux,herdr,zellij,orca,cmux,codex-app}-backend.md`
- Tests: `tests/fm-backend*.test.sh`, `tests/{zellij,cmux,herdr}-test-safety.sh`
- Skills: `.agents/skills/firstmate-orca/`, `.agents/skills/firstmate-codexapp/`

---

## 2. AI Agent CLIs / Harnesses (pluggable)

Firstmate runs *on top of* an interactive AI coding CLI (the "harness") and spawns worker agents on a harness. Harness resolution: `bin/fm-harness.sh`; per-harness launch commands: `bin/fm-spawn.sh`; agent-facing contract: `.agents/skills/harness-adapters/SKILL.md`.

Firstmate wires **four guard layers** (turn-end, PreToolUse arm-check, session-start nudge, cd-check) into each harness's native hook system.

| Harness | What it is | Config dir | Model flag | Effort flag | Autonomy flag | Env marker |
|---|---|---|---|---|---|---|
| **claude** | Anthropic Claude Code CLI (**default**) | `.claude/` | `--model` | `--effort <low..max>` | `--dangerously-skip-permissions` | `CLAUDECODE=1` |
| **codex** | OpenAI Codex CLI | `.codex/` | `--model` | `-c model_reasoning_effort=` (no max) | `--dangerously-bypass-approvals-and-sandbox` | ancestry only |
| **grok** | xAI Grok Build TUI (Claude-compatible) | `.grok/` | `--model` | `--reasoning-effort <low..high>` | `--always-approve` | `GROK_AGENT=1` |
| **pi** | Pi Coding Agent (`@earendil-works/pi-coding-agent`) | `.pi/` | `--model` | `--thinking <low..max>` | none (no perm system) | `PI_CODING_AGENT=true` |
| **opencode** | OpenCode CLI/TUI | `.opencode/` | `--model <prov/model>` | none (interactive) | `OPENCODE_CONFIG_CONTENT` allow-all | ancestry only |

**Shared guard scripts (all harnesses):**
- `bin/fm-harness.sh`, `bin/fm-spawn.sh`
- `bin/fm-turnend-guard.sh`, `bin/fm-arm-pretool-check.sh`, `bin/fm-cd-pretool-check.sh`, `bin/fm-sessionstart-nudge.sh`, `bin/fm-subagent-pretool-check.sh`
- `bin/fm-turnend-guard-grok.sh`, `bin/fm-claude-stop-autoarm.sh`
- Docs: `docs/{turnend-guard,arm-pretool-check,sessionstart-nudge,cd-guard,subagent-guard}.md`

**Per-harness config/hook files:**
- **claude:** `.claude/settings.json` (SessionStart/PreToolUse/Stop hooks), `.claude/skills` → symlink to `../.agents/skills`
- **codex:** `.codex/hooks.json` (defensive `bash -lc` wrappers)
- **grok:** `.grok/hooks/*.json` (`fm-primary-*.json`), `bin/fm-turnend-guard-grok.sh`, global hook `~/.grok/hooks/fm-turn-end.json`
- **pi:** `.pi/extensions/*.ts` (`fm-primary-turnend-guard.ts`, `fm-primary-pi-watch.ts`, `fm-calm.ts`), `.pi/extensions/lib/`
- **opencode:** `.opencode/plugins/*.js` (`fm-primary-*.js`), `.opencode/plugins/lib/`, `.opencode/plugins/package.json`
- Config: `config/crew-harness`, `config/secondmate-harness`, `config/calm`

---

## 3. Companion `-axi` Tools (npm, firstmate-specific)

Installed via `npm install -g <name>` (some run `<name> setup hooks`). Detected/offered in `bin/fm-bootstrap.sh:489–490`. **These are the most firstmate-specific dependencies — prime candidates to replace or remove in Orca.**

| Tool | Purpose | Status | Files |
|---|---|---|---|
| **gh-axi** | Canonical GitHub interface; used to merge PRs | required | `bin/fm-pr-merge.sh:84`, `bin/fm-bootstrap.sh` |
| **tasks-axi** | Backlog backend (markdown, `.tasks.toml`); version-gated 0.1.1+ | required | `bin/fm-tasks-axi-lib.sh`, `bin/fm-backlog-handoff.sh`, `.tasks.toml`, CI (`npm install -g tasks-axi`) |
| **quota-axi** | Quota/headroom for dispatch decisions | required | `bin/fm-bootstrap.sh`, `AGENTS.md` §4 |
| **lavish-axi** | Structured decisions/reports | required | `bin/fm-bootstrap.sh` |
| **chrome-devtools-axi** | Browser automation for tasks | optional (per task) | `bin/fm-bootstrap.sh` |

---

## 4. Git Forges & CI

| Tool | Purpose | Status | Files |
|---|---|---|---|
| **gh** (GitHub CLI) | Read PR head commit/status; auth check | required | `bin/fm-pr-check.sh:75`, `bin/fm-pr-poll.sh`, `bin/fm-bootstrap.sh:847` (`gh auth status` → `NEEDS_GH_AUTH`), `bin/fm-brief.sh`, `bin/fm-teardown.sh`, `bin/fm-bearings-snapshot.sh` |
| **glab** (GitLab CLI) | GitLab MR polling/head commit (merge NOT implemented) | optional (GitLab watches only) | `bin/fm-pr-check.sh:54`, `bin/fm-pr-poll.sh:8`, `bin/fm-pr-lib.sh`, `docs/gitlab-merge-watch.md` |
| **git** | Worktrees, branches, diffs, PR flows (~201 uses) | required | `bin/fm-pr-lib.sh`, `bin/fm-review-diff.sh`, `bin/fm-spawn.sh`, `bin/fm-teardown.sh` |
| **GitHub Actions** | Project's own CI (lint, tests, herdr lane, macOS bash lane) | required to contribute | `.github/workflows/ci.yml`, `.github/workflows/no-mistakes-required.yml` |

---

## 5. Contribution Pipeline (only for contributing to firstmate itself)

| Tool | Purpose | Status | Files |
|---|---|---|---|
| **no-mistakes** | Local git proxy that runs review/test/lint/CI before PR; a GitHub Actions check rejects PRs not raised through it. Min v1.31.2+ | required to contribute | `CONTRIBUTING.md`, `.no-mistakes.yaml`, `bin/fm-crew-state.sh`, `bin/fm-home-seed.sh`, `bin/fm-bootstrap.sh` (`no_mistakes_compatible`), `.github/workflows/no-mistakes-required.yml`. Install: `curl -fsSL https://raw.githubusercontent.com/kunchenguid/no-mistakes/main/docs/install.sh \| sh` |
| **shellcheck** | Lint gate for shell scripts (~166 uses); pinned release via installer | required to lint/contribute | `bin/fm-lint.sh`, `bin/fm-install-shellcheck.sh`, `tests/fm-lint.test.sh` |

> **Note:** For a standalone Orca fork you likely won't push back to `kunchenguid/firstmate`, so the entire no-mistakes + shellcheck-CI pipeline can be dropped or replaced with your own.

---

## 6. Worktree Provider

| Tool | Purpose | Status | Files |
|---|---|---|---|
| **treehouse** | Shared worktree provider (distinct from session provider) for every backend **except orca**. Pinned v2.0.1, SHA-256 verified | required (non-orca backends) | `bin/fm-install-treehouse.sh`, `bin/fm-bootstrap.sh` (`command -v treehouse`, lease check), `bin/fm-teardown.sh`, `bin/fm-home-seed.sh`. Manual: `curl -fsSL https://kunchenguid.github.io/treehouse/install.sh \| sh` |

---

## 7. Social Relay — X Mode (opt-in, inert by default)

**"X" = X.com / Twitter** (plus **Discord**). A hosted relay (`myfirstmate.io`) receives public mentions and relays firstmate's replies. **Ships inert**; activates only when `FMX_PAIRING_TOKEN` is placed in the home's gitignored `.env`.

| Tool/Service | Purpose | Status | Files |
|---|---|---|---|
| **myfirstmate.io relay** | HTTP API (over curl) for X/Twitter + Discord mentions & replies. Default `https://myfirstmate.io`, override `FMX_RELAY_URL` | optional (opt-in) | `bin/fm-x-lib.sh` (~1400-line client), `bin/fm-x-poll.sh`, `bin/fm-x-reply.sh`, `bin/fm-x-dismiss.sh`, `bin/fm-x-followup.sh`, `bin/fm-x-link.sh`. Bearer-token auth. Docs: `AGENTS.md` §14, `docs/configuration.md` |

---

## 8. Notifications

| Tool | Purpose | Status | Files |
|---|---|---|---|
| **wedge-alarm channels** | Away-mode "wedge alarm" — fires a configured notifier when escalations get wedged | optional (away-mode) | `bin/fm-supervise-daemon.sh` (wedge_alarm_notify). Config: `config/wedge-alarm`; docs `docs/configuration.md` "Away-mode wedge alarm channels". Channels: `herdr`, or custom `command:<cmd>`. No third-party messaging (Slack/email/SMS) used |

---

## 9. Runtimes & Package Managers

| Tool | Purpose | Status | Files |
|---|---|---|---|
| **node** | Runs the `.mjs` command-policy scripts; runtime for all `-axi` CLIs | required | `bin/fm-arm-command-policy.mjs`, `bin/fm-cd-command-policy.mjs` (`#!/usr/bin/env node`), gated in `bin/fm-arm-pretool-check.sh` / `bin/fm-cd-pretool-check.sh` |
| **npm** | Global installer for the `-axi` toolbelt | required | `bin/fm-bootstrap.sh:489–490`, CI `npm install -g tasks-axi` |
| **python3** | Runs the herdr helper scripts; required by real-Herdr CI lane | required (herdr/CI) | `bin/backends/herdr-eventwait.py`, `bin/backends/herdr-workspace-move.py`, `ci.yml:168` |
| **brew** (Homebrew) | Recommended macOS install path (install-hint strings only; firstmate does not run brew) | hint only | `bin/fm-bootstrap.sh:485–486` (tmux/node/git/gh/curl/jq/orca/zellij, `--cask cmux`) |

> `pip` / `cargo` / `go` / `apt` are **not** used by scripts (apt appears in docs only).

---

## 10. Standard Unix / System Utilities

Ordered by dependency weight (counts = real invocations, comment prose excluded).

| Tool | ~Uses | Purpose | Representative files |
|---|---|---|---|
| **jq** | ~305 | JSON parsing everywhere — heaviest hard dep | nearly every script; `bin/fm-fleet-snapshot.sh`, `bin/fm-x-lib.sh`, `bin/fm-crew-state.sh` |
| **git** | ~201 | Worktrees, branches, diffs | `bin/fm-pr-lib.sh`, `bin/fm-spawn.sh` |
| **shellcheck** | ~166 | Shell lint gate | `bin/fm-lint.sh` |
| **stat** | ~75 | File metadata / mtime | `bin/fm-watch.sh`, `bin/fm-fleet-snapshot.sh` |
| **sha256sum / shasum** | ~22/21 | Content hashing (Linux / macOS fallback) | `bin/fm-install-*.sh`, `bin/fm-config-inherit-lib.sh` |
| **gh** | ~20 | GitHub PR/CI ops | `bin/fm-pr-check.sh` |
| **python3** | ~16 | herdr helper scripts | `bin/backends/herdr-*.py` |
| **curl** | ~13 | Installer downloads + X relay HTTP | `bin/fm-install-*.sh`, `bin/fm-x-*.sh` |
| **perl** | ~12 | Text munging + file locking (`-MFcntl=:DEFAULT`) | `bin/fm-lock-lib.sh` |
| **node** | ~11 | Command-policy `.mjs` files | `bin/fm-*-command-policy.mjs` |
| **lsof** | ~11 | Process/port/file-handle inspection | `bin/fm-watch.sh` |
| **glab** | ~7 | GitLab CLI | `bin/fm-pr-check.sh` |
| **timeout / gtimeout** | ~5 each | Bounded command execution (GNU / macOS) | `bin/fm-fleet-snapshot.sh`, `bin/fm-watch.sh` |
| **base64** | ~5 | Encoding | lib scripts |
| **md5 / md5sum** | ~4 | Hashing fallback (`md5 -q` macOS) | `bin/fm-config-inherit-lib.sh` |
| **pgrep / sysctl / openssl / file / lsappinfo** | ~1–2 | Process lookup, sys params, crypto, file-type, macOS app info | `bin/fm-supervise-daemon.sh` |
| **tar** | — | Archive extraction in installers | `bin/fm-install-*.sh` |

Basic coreutils/builtins (`mv cp rm cat chmod cd basename sleep bash zsh`) appear in command-policy allowlists — standard, not notable third-party deps.

---

## Appendix — False positives (verified NOT actually used)

- `sqlite3`, `ruby`, `tsc` — appear only in comments/docs prose, no invocation.
- High `which` / `hash` grep counts — mostly English prose in comments ("which is", "hash sig"), not command usage.
- `codex-app` — documented but explicitly excluded as a backend (no shell-callable transport).
