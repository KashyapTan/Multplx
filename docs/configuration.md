# Configuration

The files and environment variables used by the orchestrator and its sub-agents.

[Back to the documentation index](README.md).

## Orchestrator behavior

The shared orchestrator behavior contract lives in the [dormant operating contract](../AGENTS_E.md).

## Operational home layout and state

This section is the single owner of the top-level operational-home layout; Rust command help and the owning crate modules define exact child-file fields and mutation contracts.
The tracked code root contains the shared instruction, skill, documentation, workflow, and `bin/` surfaces, while each effective `MX_HOME` contains private operational directories.
`data/` holds durable private system records such as the project and persistent-sub-agent registries, maintainer preferences, optional shared maintainer preferences, learnings, backlog, briefs, and researcher reports.
`state/` holds volatile runtime records such as task metadata, append-only status events, endpoint signals, watcher and wake-queue coordination, away-mode state, private persistent-sub-agent config-reread generations with their retry and quarantine state, and parent-owned persistent-sub-agent pending-reply records under `state/pending-replies/` (`multplx-domain::lifecycle::pending_reply`).
`config/` holds local gitignored operating choices, and `projects/` holds the legacy managed project clones; the lean local-checkout model is owned by [A9](../porting.md#a9-launch-anywhere-project-discovery-and-one-shared-chat).

`multplx-domain::lifecycle::spawn` owns base task metadata, while the runtime-backend section below owns backend-specific fields and selector interpretation.
[`mx migrate`](state-migration.md) owns the versioned home marker, canonical alias conversion, private backup and rollback evidence.
The [sub-agent model](subagent-model.md) owns the versioned task/attempt/brief and project/checkout contracts, including legacy mappings and the migration boundary.
The producing Rust review helpers own the fields they append, `multplx-core::classification` owns status-event vocabulary, and the Rust sub-agent-state backend owns current-state reconciliation.
Wake, watcher, and away-mode state mechanics remain with the Rust supervision runtime and their reference sections rather than being duplicated into one exhaustive state tree here.

`multplx-cli::session_start` is the single implementation owner of session-start ordering, composed commands, digest contents, and the digest's startup mechanism.
`docs/sessionstart-nudge.md` owns the native session-open adapter mechanics that nudge the digest command.
The dormant contract retains ownership and reconciliation requirements without a mandatory startup interview.
[Sub-agent recovery](../.agents/skills/subagent-recovery/SKILL.md) and [persistent operations](../.agents/skills/persistent-subagents/SKILL.md) point to the relevant CLI owners.

## Global launcher paths and activation

The global launcher keeps three path identities separate.
`MX_ROOT_OVERRIDE` names the installed runtime assets or an adopted source checkout.
`MX_HOME` names the persistent operational home whose `config/`, `data/`, `projects/` and `state/` directories retain configuration, registries, artifacts, queues and task records.
Optional discovery roots name directories to search for local projects; the caller's working directory does not select a new home or become an implicit recursive search root.
The [workspace entry guide](workspace-entry.md) owns local checkout selection, durable intake and terminal navigation.

The installer stores literal absolute paths under `${XDG_CONFIG_HOME:-$HOME/.config}/multplx`.
These records are data, never shell code.
The global command lives at `${XDG_BIN_HOME:-$HOME/.local/bin}/multplx`, and managed runtime assets and the separate home default below `${XDG_DATA_HOME:-$HOME/.local/share}/multplx/`.
Package installation validates version-matched assets and binary checksums without a source checkout or Rust build.
Source installation remains available; linked task worktrees cannot masquerade as the runtime source checkout, and managed source checkouts retain their cleanliness checks.
[Getting started](getting-started.md) and `multplx launcher-install --help` describe installation, transactional upgrade and data-preserving uninstall.

Bare `multplx` opens the terminal workspace.
`multplx chat` enters the remembered supported harness, and `multplx chat codex` explicitly selects Codex CLI.
The existing `multplx claude|codex|cursor|pi` forms remain available.
The launcher captures caller context before the harness enters its trusted runtime directory.
A valid caller repository may be suggested for the next request, but this never changes existing tasks or imports that repository's instructions into unrelated tasks.

`multplx shell` explicitly activates an ordinary child shell.
Activation sets `MULTPLX_ACTIVE=1`, `MX_ROOT_OVERRIDE` and `MX_HOME`, captures real harness executable paths and prepends the shell shims.
Bash and Zsh source the user's ordinary rc once and display a static marker; nested activation refuses.
The marker reads no state and does not claim that the orchestrator is running.
The activated shell stays in the caller's directory, while a harness child starts in the validated runtime context.
Exit restores the parent environment unchanged.

One live owner remains authoritative across launchers and project selection.
A supported recorded endpoint permits connection; otherwise the launcher shows the existing conversation route and durable task submission remains available.
An unavailable attachment never authorizes a second owner.
Recovery preserves runtime state without claiming unsupported transcript restoration.
Harness, worker model/effort, backend and request project remain independent choices.
Ambient tmux, Herdr and cmux identifiers pass through unless explicitly overridden by the launcher's backend option.
[Launcher verification](verification/launcher.md) records deterministic, terminal and real-provider evidence separately.

## Pi Calm preference (config/calm)

The Pi Calm extension stores the maintainer's home-local presentation choice in gitignored `config/calm` under the effective Multplx home, resolved from `MX_HOME`, then `MX_ROOT_OVERRIDE`, then the tracked code root derived from the extension path, or under `MX_CONFIG_OVERRIDE` when that test and specialized-setup override is present.
The only values it writes are `on` and `off`, each followed by one newline; an absent, unreadable, or unrecognized value defaults to off.
The `/calm` command replaces the file atomically before changing live presentation, so a failed write leaves the current choice unchanged rather than claiming persistence.
The extension reloads this preference on every Pi `session_start`, including startup, new, resume, fork, and reload reasons.
This preference is local to each Multplx home and is not part of persistent-sub-agent inherited configuration.

## Backlog backend (config/backlog-backend)

The typed Rust `BacklogStore` is the single owner of the markdown backlog schema, parsing rules, mutation semantics, and retention defaults.
The public functions in `bin/mx-backlog-lib.sh` and the `bin/mx-backlog.sh` command are transport adapters to it by default.
It stores live work in `data/backlog.md`, keeps the newest 10 Done items inline by default, and moves retention overflow to `data/done-archive.md`.
When the default backend is selected, the orchestrator uses the library for routine backlog mutations with no external package or version probe.
Persistent-sub-agent handoffs are separate and unconditional: `mx-backlog-handoff.sh` keeps its system-level validation and routes the item move through the library's atomic `mx_backlog_mv`.
It moves in-scope `## Queued` items only and refuses `## In flight` and historical `## Done` records, which stay with their home for pruning or archiving.
Handoff item bodies must use at least two leading spaces, and the helper refuses a selected item with a single-space or tab-indented continuation rather than risk orphaning it.
The `config/backlog-backend=manual` knob governs the orchestrator's own hand-editing of its backlog, not this validated handoff helper.
Set the local, gitignored `config/backlog-backend` file to `manual` to force manual backlog editing; absent or `owned` selects the in-repo library.
The file format is unchanged in both modes; the library and manual edits produce the same `## In flight`, `## Queued`, and `## Done` sections.
Use `bin/mx-backlog.sh` for routine list, show, add, done, ready, hold, update, block, unblock, move, and validation operations.

## Runtime implementation

Production launcher, local-state, backend, harness, dispatch, lifecycle, supervision, session, authority, workflow, review, delivery, and local-service commands execute through the installed Rust binary.
`MX_RUST_BIN` may name an alternate already-built `mx` binary for packaging and focused verification.
Public script names preserve compatible command paths for existing homes while delegating to the same binary.
Build the runtime with `cargo build --release --workspace --locked` when running directly from a development checkout.

## Runtime backend (config/backend / MX_BACKEND)

For spawn-capable adapters, the runtime session-provider backend controls where task windows/endpoints are created, captured, sent to, watched, and killed.
`tmux` is the verified reference backend (see [`docs/tmux-backend.md`](tmux-backend.md)); `herdr` and `cmux` are experimental spawn backends (see [`docs/herdr-backend.md`](herdr-backend.md) and [`docs/cmux-backend.md`](cmux-backend.md)).
The [built-in Git allocation owner](worktrees.md) supplies exact working paths to tmux, Herdr and cmux.
New spawns choose the backend in this order: an explicit `--backend` flag the orchestrator passes when it spawns a task, then `MX_BACKEND`, then the first non-empty line of local gitignored `config/backend`, then runtime auto-detection from `$TMUX`, `HERDR_ENV=1`, or cmux runtime signals, then default `tmux`.
If more than one runtime marker is present, detection resolves innermost-first: `$TMUX` is checked before `HERDR_ENV=1`, which is checked before cmux's primary `CMUX_WORKSPACE_ID` marker and its documented fallback signals - tmux or herdr started from inside a cmux terminal is the innermost, currently-executing layer, while cmux itself (a terminal application, not a nestable multiplexer) is always checked last.
See [`docs/cmux-backend.md`](cmux-backend.md#runtime-detection) for why cmux can be selected when `CMUX_WORKSPACE_ID` is absent.
Auto-detected herdr or cmux prints a stderr notice naming `config/backend` and `--backend tmux` as opt-outs; auto-detected tmux stays silent to preserve existing default behavior.
Any value other than `tmux`, `herdr`, or `cmux` is rejected until another adapter is implemented and verified.
`mx-spawn.sh` accepts `tmux`, `herdr`, and `cmux` for implementation and research tasks; `backend=cmux` still refuses the legacy `--daemon` alias until persistent-sub-agent launch semantics are designed.
`codex-app` is not an accepted runtime backend yet; [`docs/codex-app-backend.md`](codex-app-backend.md) owns the Codex App boundary.
The session-start persistent-sub-agent liveness sweep uses the recovery-grade `mx_backend_agent_state` classifier where verified.
The comment above that function in `bin/mx-backend.sh` is the single owner of its detailed state contract and recovery authorization.
The compatibility helper `mx_backend_agent_alive` continues to collapse those detailed results to `alive`, `dead`, or `unknown` for older callers.
A herdr spawn additionally version-gates against the installed `herdr` binary's protocol and requires `jq`, refusing loudly on an incompatible or missing installation.
A cmux spawn additionally version-gates against the installed `cmux` binary's version, requires `jq`, and requires the control socket to be reachable and accessible (see [`docs/cmux-backend.md`](cmux-backend.md) "Setup" for the one-time socket-access configuration this needs; Automation mode is the recommended socket control mode, with Password mode supported via `config/cmux-socket-password`), refusing loudly and non-retryably on a `cmuxOnly`/unauthenticated socket.
A backend spawn refusal from a missing dependency, version gate, or unauthenticated socket is terminal for that selected backend; the orchestrator surfaces it as a blocker instead of silently retrying another backend.
Task meta records `backend=` only for a non-default backend; an absent `backend=` means `tmux`, preserving existing default-path meta files.
A herdr task additionally records `herdr_session=`, `herdr_workspace_id=`, `herdr_tab_id=`, and `herdr_pane_id=`.
A cmux task additionally records `cmux_workspace_id=` and `cmux_surface_id=`.
Task selectors for `mx-peek.sh`, `mx-send.sh`, and `mx-actor-state.sh` resolve centrally through `mx_backend_resolve_selector`.
A selector containing `:` is passed through as an explicit backend endpoint escape hatch.
Otherwise an exact task id matching `state/<id>.meta` wins before the legacy `mx-<id>` label fallback, so task ids that themselves start with `mx-` route to their own metadata instead of being stripped.
A metadata-routed selector returns the recorded backend target (`window=`), and matching explicit targets can still recover the recorded backend when metadata contains the same endpoint.
Only metadata-routed task selectors carry persistent-home-marker and Codex-harness context; explicit endpoint escape hatches do not.
These five sentences are the single owner of the task-selector vocabulary; backend guides and other documents point here instead of restating the resolution order.
`mx-teardown.sh <id>` takes a task id directly and uses the same recorded backend target fields after loading `state/<id>.meta`.
By default, Herdr workspaces are derived from `MX_HOME`: the primary home uses `broker`, and a persistent-sub-agent home marked by `.mx-daemon-home` uses `daemon-<daemon-id>`.
The default-container spawn, list-live, and recovery paths read that label from the active home, so a persistent sub-agent's children stay inside that sub-agent home's Herdr space.
The optional local `config/herdr-presentation-spaces` presence flag instead enables Herdr's default-off disposable single-task visual projection; [Optional presentation spaces](herdr-backend.md#optional-presentation-spaces) owns its behavior, safety limits, recovery contract, and narrow locked session-start cleanup of exact restored idle-shell children.
The flag is default-off and inherited into persistent-sub-agent homes under the primary-authoritative contract owned by [inherited configuration](configuration.md#persistent-home-inheritance).
For normal herdr operations, `HERDR_SESSION` selects the named session, but destructive test cleanup must not rely on `HERDR_SESSION` alone.
Use the explicit guarded cleanup path described in [`docs/herdr-backend.md`](herdr-backend.md) instead of `herdr server stop`.
cmux has no session layer at all - one workspace per task, in whatever cmux window is open - and its socket password (when configured) is read from local, gitignored `config/cmux-socket-password` under the effective config directory, never committed.
The caller-facing label remains `mx-<id>`, but the actual cmux workspace title is scoped by the active `MX_HOME` readable label plus a short hash of the resolved `MX_ROOT` path as `mx-<home-label>-<id>`.
Test cleanup must use the guarded path in [`docs/cmux-backend.md`](cmux-backend.md#current-operation-and-safety), never enumerate-and-close every workspace.
The `config/backend` file is not inherited by persistent-sub-agent homes.

## Away-mode supervisor backend (MX_SUPERVISOR_BACKEND / MX_SUPERVISOR_TARGET)

The `/afk` service daemon injects escalation digests into the orchestrator's own pane independently of where new task endpoints are spawned.
It currently supports only `tmux` and `herdr` supervisor panes.
Set `MX_SUPERVISOR_BACKEND=tmux|herdr` and `MX_SUPERVISOR_TARGET=<target>` to override both axes explicitly; for herdr the target is `"<session>:<pane-id>"`.
Without overrides, backend detection uses `$TMUX_PANE` first, then `HERDR_ENV=1` with `HERDR_PANE_ID`, then falls back to `tmux`.
That keeps a tmux pane nested inside herdr on the tmux transport, matching the runtime backend's innermost-first rule.
Target detection uses `MX_SUPERVISOR_TARGET`, then `$TMUX_PANE`, then `"${HERDR_SESSION:-default}:${HERDR_PANE_ID}"` under herdr, then the legacy `broker:0` tmux fallback with a warning.
Selecting any other supervisor backend, including `cmux`, refuses when the service daemon starts instead of trying tmux injection primitives against a non-tmux pane.

## Away-mode wedge alarm channels (config/wedge-alarm)

When away-mode injection wedges past `MX_MAX_DEFER_SECS`, the sub-supervisor raises a loud, rate-limited alarm.
Beyond the durable `state/.subsuper-inject-wedged` marker and the tmux status-line flash, it attempts a configured backend-independent active alert that can reach the maintainer even when every pane and its backend status-line is unreadable.
`config/wedge-alarm` (local, gitignored) lists channel directives, one per non-empty, non-comment line; every listed non-`off` channel fires, best-effort.
`MX_WEDGE_ALARM_CHANNEL` overrides the file with a single directive.
Directives are `off` (a position-independent kill switch that disables every active alert), `auto`/`default`, `herdr` (herdr UI notification), and `command:<cmd>` (run `<cmd>` via `sh -c`, summary on `$1` and stdin).
An absent file means `auto`: no platform has a built-in OS channel, so the durable marker is the only signal until a channel is configured; the alarm fires at most once per max-defer window after a genuine wedge.
A missing or failing channel logs and falls through to the next, never crashing the service daemon.
See [`verification/supervision.md`](verification/supervision.md#wedge-alarm-channels) for active evidence and [`examples/wedge-alarm`](examples/wedge-alarm) for a copyable config.

## Optional deep-review configuration (.deep-review.yaml)

The tracked `.deep-review.yaml` is read only when `bin/mx-deep-review.sh` is explicitly invoked for that project.
Its absence or invalidity does not affect startup, ordinary testing, delegation or delivery.
The review, delivery, and PR-security command family executes through the Rust review-delivery boundary.
Code-executing commands, the command-permission flag, project-settings suppression, and document instructions are loaded from the trusted default-branch copy.
The reviewed branch may supply cosmetic fields, but its commands are inert unless the trusted copy explicitly sets `allow_repo_commands: true`.
The Multplx default keeps repository commands empty, relies on the optional pipeline's focused fallback validation, and keeps evidence in private `state/<id>.gate/` records rather than the branch.
It must not set `commands.test` to a complete `tests/*.test.sh` walk or `bin/mx-test-run.sh --all`.
See [CONTRIBUTING.md](../CONTRIBUTING.md) for the Multplx-specific local test policy and entry points.
Portable shard evidence and coverage rules are in [mx-test-portable-shards.md](mx-test-portable-shards.md); [herdr-backend.md](herdr-backend.md#destructive-lab-safety) owns the real-Herdr lane's isolation boundary, and [runtime-backends.md](verification/runtime-backends.md#herdr) owns active evidence.

## Maintainer Preferences (data/maintainer.md / data/maintainer-shared.md)

Domain-local preferences for one maintainer's system live locally in each home's `data/maintainer.md`; it is gitignored and printed in the session-start context digest after `data/projects.md` and optional `data/daemons.md`.
Before changing it, inspect the current file and rewrite or prune the matching bullet in place; add a new bullet only for a genuinely new durable preference.
Shared maintainer preferences that apply across persistent-sub-agent domains live only in the primary home's optional `data/maintainer-shared.md`.
The persistent-home inheritance section below owns propagation; the parent-authoritative shared preference header and read-only child copies prevent accidental reverse synchronization.

## Operational learnings (data/learnings.md)

System-local operational facts and gotchas live locally in `data/learnings.md`; it is gitignored and printed after the maintainer-preference files in the session-start context digest.
The file is created lazily on first learning and follows the same dated, evidence-backed, curated style as `data/maintainer.md`: inspect the current file first, then rewrite or prune stale entries instead of appending forever.
There is no shared learnings file by maintainer decision.

## Persistent sub-agent routes (data/daemons.md)

The [scoped coordinator command](scoped-coordinators.md) provisions a named domain and private home without manual route setup.
The following registry grammar remains the compatibility surface for existing homes.
Persistent-sub-agent routes live locally in `data/daemons.md`.
The existing parser accepts one route per line:

```text
- <id> - <charter summary> (home: <absolute-home-path>; scope: <responsibility>; projects: <project-a>, <project-b>; added <date>)
```

Keep the route concise; `home:` locates the seeded charter and `projects:` is non-exclusive provisioning data.
`mx-home-seed.sh validate` refuses duplicate ids, duplicate homes, and nested or overlapping homes.
The main orchestrator routes by reading those scopes with judgment; the project list is provisioning data, not exclusive ownership.
Use `mx-home-seed.sh <id> - {<project>...|--no-projects}` to provision a private persistent home using installed runtime assets.
Selected projects become canonical references to existing checkouts; seeding does not clone the runtime or projects.
Use the deliberate `--no-projects` signal for a home with no initial project references.
It cannot be combined with a project list, and omitting both still fails loudly.
A project-less seed requires no existing project clones or `data/projects.md` entries in the home, so it refuses a populated-home conversion without changing that home.
A preexisting project-bearing charter is also refused until it is re-scaffolded with `--no-projects` or removed.
The reservation is held under the persistent-sub-agent id across normal restarts and interrupted seeding.
Teardown retains uncertain Git-backed homes and archives new private homes using their exact recorded lease identity.
Persistent-sub-agent routes also support local-only and remote-free projects without fabricating a publication remote.
The optional deep-review command resolves configuration from the explicitly selected task project and requires no per-clone initialization during seeding.
After creating a persistent sub-agent, move existing main-backlog queued items that you have judged in-scope with `mx-backlog-handoff.sh <daemon-id> <item-key>...`; it is idempotent and refuses In flight, Done, or non-persistent homes.
Set `MX_DAEMON_CHARTER` to seed from inline charter text when no filled charter brief exists; set `MX_DAEMON_SCOPE` when the routing scope should differ from the charter text.
The seeded home's `data/charter.md` owns the standard persistent-sub-agent lifecycle and escalation contract; the route file points to it through the existing `home:` field instead of adding another pointer.
Each seed writes an `.mx-daemon-home` identity marker at the home root.
The tracked root `.gitignore` ignores that marker, so validation can read it without making a freshly seeded home appear dirty to porcelain-based safety checks.
This does not relax protection for any other untracked file.
An existing linked-worktree home that predates this rule advances through its marker-only state during its next bootstrap or spawn local sync, after which Git ignores the marker normally.
A standalone-clone home cannot receive a primary-local commit through that no-fetch sync, so it receives the rule through `/updatemultplx`'s origin refresh instead.

## MX_HOME

`MX_HOME` selects the operational home for one orchestrator instance.
When it is unset, most scripts use the repo root as the home; when it is set, scripts still run from this repo's `bin/`, but `state/`, `data/`, `config/`, and `projects/` come from `$MX_HOME`.
`MX_ROOT_OVERRIDE` overrides the Multplx repo root used by scripts, including the primary checkout watched by the worktree-tangle guard.
When `MX_HOME` is unset, it also behaves as the old whole-root override.
`bin/mx-send.sh` is intentionally stricter than that general fallback: it requires `MX_HOME` to be set before resolving a target, so operator steers cannot silently resolve against the wrong home.
`MX_STATE_OVERRIDE`, `MX_DATA_OVERRIDE`, `MX_PROJECTS_OVERRIDE`, and `MX_CONFIG_OVERRIDE` override individual operational directories for tests and specialized harness setup.
Scoped parent routing accepts a root state override outside its home only while a live watcher lock proves the exact root home and process identity; accepted child facts remain queued when that proof is unavailable.
For the herdr backend, `MX_HOME` also determines the workspace label used by the adapter.
For the cmux backend, `MX_CONFIG_OVERRIDE` overrides where `config/cmux-socket-password` is read from, while `MX_HOME` determines the default config path and readable home prefix embedded in workspace titles.
The full cmux home label also includes a short hash of the resolved `MX_ROOT` path, and there is no per-home container split.

## Harness support

claude, codex, cursor, and pi are empirically verified; new harnesses get verified through a monitored trial task before joining the set.
The trusted project-level [`.codex/config.toml`](../.codex/config.toml) selects `sandbox_mode = "danger-full-access"` for Codex primary sessions because session locking, host-capacity checks, runtime backend control, and sub-agent launch require host operations that the default command sandbox denies.
The project setting does not change `approval_policy`; Codex approval prompts remain under the maintainer's user-level or command-line policy.
The verified adapter knowledge - busy signatures, interrupt and exit commands, skill-invocation syntax, and per-harness quirks - lives in [`.agents/skills/harness-adapters/SKILL.md`](../.agents/skills/harness-adapters/SKILL.md).
Launch mechanics and verified command templates are owned by the Rust lifecycle command; [`bin/mx-spawn.sh`](../bin/mx-spawn.sh) is its transport-only compatibility entrypoint.
Primary-session turn-end guard integrations for verified harnesses are tracked as repo-level hook files and documented in [`docs/turnend-guard.md`](turnend-guard.md).
Primary-session watcher wake protocols are rendered at session start by [`bin/mx-supervision-instructions.sh`](../bin/mx-supervision-instructions.sh) from [`docs/supervision-protocols/`](supervision-protocols/).
Claude's Stop `asyncRewake` hook owns tokenless re-arm cycles, Codex and Cursor use bounded foreground checkpoints, and Pi uses its two tracked primary extensions.
`config/subagent-harness` is a local, gitignored file containing one adapter name for ordinary sub-agent launches; `config/actor-harness` remains its legacy alias.
When it is absent or contains `default`, sub-agents mirror their parent's harness.
`config/persistent-subagent-harness` selects the harness for persistent launches, optionally followed by model and effort tokens; `config/daemon-harness` remains its legacy alias.
The first non-empty, non-comment line is parsed as `<harness> [<model>] [<effort>]`.
A bare `<harness>` preserves the previous behavior: harness only, with no model or effort launch flag.
When the persistent harness token is absent or `default`, launch falls back through `config/subagent-harness` and then the parent's own harness, and no model or effort is read from that file.
`mx harness persistent-subagent-model` and `mx harness persistent-subagent-effort` expose the optional tokens; `daemon-model` and `daemon-effort` remain command aliases.
`mx harness subagent` and `mx harness persistent-subagent` resolve the harness defaults, with `actor` and `daemon` retained as aliases.
Conflicting canonical and legacy files refuse launch instead of silently choosing one.
An explicit harness argument to `mx-spawn.sh` still overrides either config file for that spawn only.
An explicit `--model` or `--effort` overrides the matching persistent default; an explicit verified harness starts with clean model and effort defaults unless those flags are also passed.
When `config/subagent-dispatch.json` or its legacy alias exists, ordinary launches require an explicit resolved harness.
The inherited-local-material contract is owned by [inherited configuration](configuration.md#persistent-home-inheritance); child homes receive the shared sub-agent defaults and profiles.
Those inherited values are defaults and rules only; `mx-spawn` still permits a consciously chosen explicit verified harness outside the config.
The canonical persistent default is inherited so nested delegation can select persistent execution without a separate role class.
The legacy `config/daemon-harness` stays home-local for compatibility.
For Pi persistent-home launches, `mx-spawn.sh` starts Pi with `-e` pointed at the home's `.pi/extensions/mx-primary-pi-watch.ts` and `.pi/extensions/mx-primary-turnend-guard.ts`, supplied through its links to installed runtime assets.
For Cursor launches, `mx-spawn.sh` always passes `--sandbox enabled --trust`; sub-agent turn-end signaling comes from a private per-run plugin and primary behavior comes from tracked `.cursor` rules and hooks.
Cursor model effort is encoded as `<model>[effort=<level>]`; use `agent models` in the authenticated account before choosing a named model.
Cursor deep-review is deliberately unsupported because schema enforcement and project-rule suppression are not verified together.
[Cursor CLI verification](verification/cursor-cli.md) owns the dated version, authentication, sandbox, hook, resume, persistent-sub-agent, and negative-control evidence.

## Sub-agent dispatch profiles (config/subagent-dispatch.json)

`config/subagent-dispatch.json` is an optional local, gitignored file containing natural-language dispatch rules; `config/actor-dispatch.json` remains its compatibility alias.
The lifecycle runtime does not match those rules; the assigning agent chooses a profile and passes concrete `--harness`, `--model`, and `--effort` flags to `mx spawn`.
When the file exists, ordinary spawns require an explicit verified harness through `--harness` or the positional adapter form.
Batch spawns satisfy the same requirement with a shared `--harness`.
Persistent spawns resolve through their separate persistent defaults and optional model and effort tokens.
This section is the single owner of the canonical schema and its per-field semantics; the orchestrator selects concrete dispatch values without a fixed model-selection playbook.

```json
{
  "rules": [
    {
      "when": "<natural-language condition describing a kind of task>",
      "use": [
        { "harness": "<adapter>", "model": "<optional model>", "effort": "<low|medium|high|xhigh|max, optional>" }
      ],
      "why": "<optional rationale that helps the orchestrator choose>"
    }
  ],
  "default": [
    { "harness": "<adapter>", "model": "<optional model>", "effort": "<optional effort>" }
  ]
}
```

Per rule, `when` and `use` are required.
Both `use` and the optional top-level `default` accept either one profile object or a non-empty array of profile objects.
The single-object form stays fully backward-compatible, and every profile needs `harness`.
Profile `model` and `effort` fields and rule `why` are optional.
An omitted model or effort means the selected harness uses its own default for that axis.
Profile arrays express available choices without prescribing a capacity-ranking reasoning procedure.
If no dispatch rule fits, the assigning agent resolves `default` through the same object-or-array path before falling back to `config/subagent-harness`.
If a selected profile carries an effort value the chosen harness does not accept, `mx-spawn.sh` records the requested `effort=` in task meta for traceability but omits the launch flag, and bootstrap reports the invalid harness/effort pair with the selected file's dispatch diagnostic.
See [`docs/examples/actor-dispatch.json`](examples/actor-dispatch.json) for the compatible profile shape to copy into local `config/subagent-dispatch.json`.
When the file exists, bootstrap validates it with the Rust JSON reader.
Valid files stay silent by default; `MX_BOOTSTRAP_VERBOSE_FACTS=1` adds the selected file, rules and optional default profile facts.
Malformed JSON, invalid profiles, unverified harnesses and unsupported efforts are reported as `SUBAGENT_DISPATCH` for the canonical file or `ACTOR_DISPATCH` for the legacy alias.
The existing `MISSING: jq` dependency diagnostic remains part of bootstrap's toolchain reporting.
While the file remains present, ordinary spawns require an explicit resolved harness; malformed configuration must be reported and corrected rather than selected around.
Persistent homes inherit profiles through the same configuration owner.

## Dispatch capacity (config/api-capacity / config/admission-capacity.json / state/.dispatch-queue)

The typed implementation in `crates/multplx-backend/src/headroom.rs` owns capacity calculation, durable admission receipts, and parked requests; `bin/mx-headroom.sh` is the public adapter.
Every nested home resolves its root from validated task ancestry.
The root home's `config/`, `state/.dispatch-queue/`, and `state/.admissions/` therefore provide one budget across competing homes.
Active admission receipts charge sessions and harness resources across descendant homes, including persistent sessions.
Root endpoint metadata contributes legacy live executions only when no exact task, attempt and endpoint receipt already accounts for them.
Reserved and uncertain admissions remain charged, including reservations that become active while a capacity snapshot is being evaluated.
Its JSON combines spare CPU and memory with a conservative configured API concurrency budget.
The default local reservation is one-quarter logical CPU and 256 MiB per additional sub-agent; `MX_HEADROOM_CPU_PER_ACTOR` and `MX_HEADROOM_MEM_PER_ACTOR_BYTES` retain strict test and specialized-setup overrides.
The optional global budget is the nonnegative integer in `config/api-capacity`, with per-harness refinements in `config/api-capacity-<harness>`; absent configuration uses twenty.
This signal is labeled `configured-budget`, not live provider quota.
A provider omitted from the configured snapshot has an opaque native limit: the shared session budget still applies, while Multplx does not invent a zero or claim knowledge of that provider's quota.

`config/admission-capacity.json` optionally configures root-scoped custom resources and queue aging:

```json
{
  "version": 1,
  "aging_seconds": 300,
  "worker_headroom": 1,
  "resources": { "gpu": 2, "project:large-repo": 1 }
}
```

`version` must be `1`, `aging_seconds` and every capacity must be positive, and names may contain ASCII letters, digits, `.`, `_`, `-`, or `:`.
Native `session` and `harness:*` capacities cannot be overridden.
`worker_headroom` defaults to one shared session slot for workers; active workers count toward that reserve when admitting a coordinator.
It is root-scoped, so creating another coordinator home does not multiply the available session budget.
Each spawn requests one session, selected-harness, and bound-project unit.
Repeat `--resource NAME=UNITS` to request additional configured resources, such as `--resource gpu=1`.
Names must be distinct and units must be positive integers; explicit requests cannot use `session`, `harness:*`, or `project:*` names.
The accepted resource request is frozen with the durable queue entry and replayed unchanged after restart.

At limit or behind an unfinished dependency, `mx spawn` writes one mode-`0600` record per stable request ID under the root queue and returns before worktree allocation or endpoint creation.
`--request-id <stable-id>` lets a durable inbox repeat an uncertain submission.
Reuse succeeds only for the same frozen task, parent route, accepted brief, project/checkout, starting revision, attempt, and allocation.
A released request ID cannot launch again; replacement needs a new request and attempt identity.
Use `--replace-attempt CURRENT_ID` to explicitly replace the current attempt; replacement increments its generation and uses an isolated endpoint.
The record preserves owner and parent homes/states, profile choices, priority, dependencies, resources, and enqueue time.
Filenames use request IDs, so equal task basenames in different homes do not collide.
Queue and admission transitions use atomic replacement under a short root lock; provider liveness and endpoint launch run after that lock is released.
Runnable work is ordered by `priority + floor(age_seconds / aging_seconds)`, then enqueue time and task ID.
Aging prevents starvation.
A blocked or oversized request does not prevent a smaller runnable request from fitting, and dependency cycles are rejected before publication.
A drain durably marks `dispatching` and reserves capacity before launch.
After restart it acknowledges only exact canonical metadata with a live endpoint.
Missing, duplicate, wrong-home, stale-attempt, or mismatched metadata stays retained.
Failed endpoints remain uncertain until the lifecycle owner verifies absence; a proven-absent retry reuses its exact allocation.
Successful teardown releases capacity only when task ID, owner state, attempt ID, and endpoint all match.
Use `bin/mx-headroom.sh --queue` to inspect requests, `--queue-cancel <request-id>` to cancel queued work, and `--queue-priority <request-id> <signed-priority>` to reprioritize it.
Dispatching work cannot be cancelled or reprioritized.
`--queue-add <id> <project> [profile flags]` parks explicitly, and `--queue-drain` performs one drain attempt.

## Toolchain

On session start the orchestrator detects what its required toolchain is missing or too old and lists each problem with either an exact install command or manual instructions.
It installs automatically supported tools only after you say go; manual-only tools remain for you to install from the printed instructions.
Required tools come in two parts: a universal toolchain every home needs regardless of backend, and a per-backend delta that follows the runtime backend actually resolved for this home.
The universal toolchain is Git, gh and jq.
Safe worktree cleanup additionally requires a successful occupant observation with `lsof`; unavailable observation retains work.
The viz and vplan services are Rust-native and do not require Node.
The [worktree lifecycle](worktrees.md) is built into the runtime; there is no worktree-provider installer.
This section is the single owner of that universal toolchain list; backend guides' prerequisites point here and add only their backend-specific tools.
The in-repo deep-review scripts supply explicitly requested review evidence, while official gh supports ordinary branch and PR publication with user-configured authentication.
Bootstrap does not require GitHub authentication in the orchestrator session.
Authentication, repeat-safe publication and the human-only merge boundary are documented in [delivery.md](delivery.md).
The in-repo vplan module covers explicitly requested rich-review operations and validates its vendored assets lazily when created, reviewed or explicitly self-checked.
Backlog mutations and dispatch capacity are owned by the repository's typed backlog and headroom modules behind their existing shell entry points.
The per-backend delta is required only for the backend resolved from `MX_BACKEND`, then `config/backend`, then runtime auto-detection, then default `tmux`, so a home is never told to install a tool an inactive backend or feature would need.
That delta is owned in code by `mx_backend_required_tools` in `bin/mx-backend.sh`: the resolved backend's own session-provider CLI (`tmux`, `herdr`, or `cmux`) plus the compatibility `jq` requirement for the JSON-emitting experimental adapters (`herdr`, `cmux`).
Backend tool availability uses the adapter's own executable resolver, so bootstrap and spawn agree on supported non-`PATH` locations such as cmux's bundled CLI.
An unknown resolved backend emits `BACKEND_INVALID` and blocks dispatch instead of silently dropping its dependency delta or falling back to tmux.
A Herdr or cmux home is therefore never told `tmux` is missing; every supported backend uses built-in worktree acquisition.
Bootstrap validates canonical dispatch profiles and the legacy alias in the Rust owner; `jq` remains part of the current general toolchain.
Bootstrap self-checks that `bin/mx-headroom.sh --json` succeeds and emits valid JSON.
An unreadable local capacity signal or malformed configured API budget reports `HEADROOM_INVALID` and blocks dispatch.
Bootstrap does not probe vplan or deep-review assets.
An explicit `bin/mx-vplan.sh --self-check`, `new` or `review` validates the Rust service boundary, seed template, review SDK and pinned Mermaid hash.
Doctor reports an optional-tool failure only when an active vplan run depends on invalid assets.
The "Dispatch capacity" section owns configuration and queue behavior.
Bootstrap also reports a `TANGLE:` line when `MX_ROOT` is on a named non-default branch; follow the printed checkout remediation rather than treating it as an installable tool problem.
In a read-only session that did not get the system lock, the same line is advisory and omits the checkout command.
The locked session-start bootstrap step also runs a best-effort project clone refresh through `mx-system-sync.sh`.
It emits `SYSTEM_SYNC:` for skipped refreshes that may matter, recovered self-heals, and `STUCK:` alarms.
Normal completed runs keep local-only and no-origin skips silent.
If bootstrap kills a timed-out refresh, it replays any completed `mx-system-sync.sh` output before the aggregate timeout skip so no finished result is lost.
A killed refresh (or a teardown process kill) can leave an orphaned `.git/packed-refs.lock` in a clone, which makes the next refresh's fetch fail with Git's `Unable to create '...packed-refs.lock': File exists`.
On that signature only, `mx-system-sync.sh` retries the fetch with a bounded wait for the lock to self-clear, then removes the lock and retries once more only when it can prove the lock stale, exactly like the `mx-teardown.sh` `index.lock` recovery.
It never removes a live lock, leaves any other failure shape untouched, and prints every wait, retry, and removal to stderr plus a one-line `recovered:` summary to stdout on success so that this session-start relay still surfaces the recovery.
The locked session-start bootstrap step also runs the guarded local persistent-sub-agent sync for recorded live homes, then propagates declared inherited local material into each validated live home.
It emits `DAEMON_SYNC:` only when a home was skipped for an actionable sync reason, inheritance failed, or a divergent shared maintainer-preference copy was quarantined.
When a running home advances and its loaded instruction surface (`AGENTS.md`, `bin/`, or `.agents/skills/`) changed, bootstrap sends the re-read nudge itself through the stable `mx-<id>` selector and reports the exact completed send as `BOOTSTRAP_INFO:`.
If that send fails, bootstrap keeps an idempotent retry marker and emits `NUDGE_DAEMONS:` with the failure reason.
The same bootstrap run emits `DAEMON_LIVENESS:` only when a registered persistent sub-agent is skipped or its relaunch fails; already-live and successfully relaunched persistent sub-agents are handled silently.
For a mid-session inherited local-material edit where tracked-file sync is not needed, run `bin/mx-config-push.sh`.
It uses the same live persistent-sub-agent discovery and propagation helper as bootstrap, prints each live home's `actor-dispatch.json`, `actor-harness`, `backlog-backend`, `herdr-presentation-spaces`, and `data/maintainer-shared.md` result as `pushed`, `unchanged`, `skipped`, or `error`, and exits non-zero for real propagation errors or config-reread send failures.
When an allowlisted config item changes for an already-running home, it sends the literal-content reread pointer described in [inherited configuration](configuration.md#persistent-home-inheritance); unchanged allowlisted config sends no pointer unless a previous delivery is pending.
The locked bootstrap inheritance pass uses the same per-home changed-set and reread path for already-running homes; see `daemon-provisioning` for the compatibility contract owner.
That live discovery starts from `state/*.meta` records with `kind=daemon`; `data/daemons.md` only backfills `home=` for older or incomplete meta records.
Skipped items, such as a destination checkout that does not yet gitignore the item, are visible warnings but not hard failures.

## Environment variables

Runtime tuning via environment variables (defaults shown):

```sh
MX_HOME=                 # optional operational home for most scripts, unset means this repo root; mx-send requires it explicitly
MX_ROOT_OVERRIDE=        # override Multplx repo root, tangle-guard target, and cmux home-title hash; also legacy whole-root override when MX_HOME is unset
MX_STATE_OVERRIDE=       # alternate state dir, mainly for tests
MX_DATA_OVERRIDE=        # alternate data dir, mainly for tests
MX_PROJECTS_OVERRIDE=    # alternate projects dir, mainly for tests
MX_CONFIG_OVERRIDE=      # alternate config dir, mainly for tests
MX_PROC_ROOT_OVERRIDE=   # alternate /proc root for the Linux process-identity read in mx-wake-lib.sh, mainly for tests
MX_BACKEND=             # optional runtime backend override for new spawns; tmux/herdr/cmux support implementation/research spawns, codex-app is not accepted
HERDR_SESSION=default  # herdr-only: named session for normal backend ops; not enough for destructive cleanup (docs/herdr-backend.md)
MX_BACKEND_HERDR_COMPOSER_LINES=20  # herdr-only: tail lines scanned by composer-state guard/fallback paths; idle-baseline submit confirmation uses agent-state
MX_BACKEND_HERDR_IDLE_RE='^Type a message\.\.\.$'  # herdr-only: empty-composer placeholder regex after shared ghost extraction plus border and prompt stripping
MX_BACKEND_HERDR_BARE_PROMPT_RE='^[❯›]'  # herdr-only: verified agent glyphs recognized as an UNBORDERED (bare) composer row, e.g. Claude's ❯ or Codex's ›; shell glyphs remain unknown rather than empty, and de-emphasised ghost/placeholder text reads empty through shared mx_composer_strip_ghost (docs/herdr-backend.md "Composer and injection safety")
MX_BACKEND_HERDR_PI_COMPOSER_MAX_LINES=8  # herdr-only: maximum rows admitted between Pi's native-identity-corroborated separator pair; taller or ambiguous candidates stay unknown (docs/herdr-backend.md "Composer and injection safety")
MX_BACKEND_HERDR_SUBMIT_POLLS=6  # herdr-only: agent-state samples spread across each Enter attempt's budget when confirming a submit (docs/herdr-backend.md "Current transport behavior")
MX_BACKEND_HERDR_SUBMIT_MIN_SLEEP=0.6  # herdr-only: minimum per-Enter confirmation budget before polling agent-state after an idle baseline
MX_BACKEND_CMUX_COMPOSER_LINES=20  # cmux-only: tail lines scanned to locate the composer row for submit verification
MX_BACKEND_CMUX_IDLE_RE='^Type a message\.\.\.$'  # cmux-only: empty-composer placeholder regex after border/prompt stripping
CMUX_SOCKET_PASSWORD=   # cmux-only: socket password fallback when config/cmux-socket-password is absent (docs/cmux-backend.md)
MX_SESSION_START_STATUS_TAIL=5   # state/*.status lines printed per task in the session-start digest
MX_BOOTSTRAP_DETECT_ONLY=0   # internal/read-only session-start mode: skip bootstrap's mutating sweeps and print advisory TANGLE wording
MX_GUARD_READ_ONLY=0    # internal/read-only guard mode: keep alarms but suppress drain, supervision repair, and checkout repair commands
MX_GUARD_CONTINUE_LINE='This is a supervision warning only; the guarded operation WILL still run.'   # banner continuation line; mx-send.sh overrides it to name the requested message specifically
MX_POLL=15              # seconds between watcher poll cycles
MX_HEARTBEAT=600        # base seconds between heartbeat scans; no-change heartbeats are absorbed while idle
MX_HEARTBEAT_MAX=7200   # heartbeat backoff cap
MX_CHECK_INTERVAL=300   # seconds between slow checks (authenticated merge polls or custom checks)
MX_CHECK_TIMEOUT=30     # seconds allowed per slow check script
MX_CODEX_WATCH_CHECKPOINT=180   # seconds per foreground watcher checkpoint in Codex primary supervision
MX_ACTOR_STATE_BIN=bin/mx-actor-state.sh   # test override for the current-state reader used by working/paused watcher triage
MX_LOCK_STALE_AFTER=2   # seconds before dead-pid lock records can be reclaimed; mid-acquire locks keep at least 2s grace
MX_GUARD_GRACE=300      # seconds before guard warnings, arm health checks, and the primary turn-end guard treat a watcher beacon as stale
MX_CLAUDE_AUTOARM_SYNC_WAIT_MS=800   # milliseconds the --claude turn-end guard waits for the Stop auto-arm's claim, health, or fresh rewake epoch before re-blocking
MX_CLAUDE_AUTOARM_EPOCH_FRESH=15   # seconds a recorded auto-arm rewake outcome counts as this event epoch's owned recovery
MX_CLAUDE_TURNEND_BLOCK_BUDGET=3   # consecutive --claude guard re-blocks before a degraded allow; safely below Claude Code's 8-block override
MX_ARM_CONFIRM_TIMEOUT=10   # seconds mx-watch-arm waits to confirm a fresh watcher before reporting FAILED
MX_ARM_ATTACH_POLL=0.5  # seconds between checks while mx-watch-arm is attached to an existing healthy watcher cycle
MX_PI_ARM_READY_TIMEOUT_MS=12000   # milliseconds the Pi watcher extension waits for a successor arm to report started or attached
MX_WATCH_ARM_RETIRE_TIMEOUT_MS=1000   # milliseconds Pi waits for an unready successor arm to exit before abandoning retries
MX_WATCH_REARM_RETRY_BASE_MS=250   # Pi adapter base delay for continuity restoration retries
MX_WATCH_REARM_RETRY_MAX_MS=4000   # Pi adapter cap for exponential continuity retry delay
MX_WATCH_REARM_RETRY_LIMIT=5   # Pi adapter launch-failure retries before surfacing restoration failure
MX_WATCH_CYCLE_LOG_MAX_BYTES=262144   # size cap for the arm-owned watcher lifecycle ledger
MX_WATCH_CYCLE_LOG_KEEP_LINES=1000   # newest complete lifecycle rows considered when the ledger is capped
MX_WATCHER_STALE_GRACE=300   # defaults to MX_GUARD_GRACE; seconds a live watcher lock may have a stale beacon before re-arm errors
MX_SIGNAL_GRACE=30      # seconds to coalesce nearby status and turn-end signals into one wake
MX_MAINTAINER_RE='done:|needs-decision:|blocked:|failed:|PR ready|checks green|ready in branch|merged'   # maintainer-relevant status regex; nonterminal progress verbs remain excluded even when their prose matches
MX_CLASSIFY_PAUSED_VERB=paused     # read-side compatibility override for legacy status logs; validated sub-agent writes use the closed `paused` state
MX_TASK_ID=                        # spawn-managed task binding consumed by mx-report and mx-report-mcp; do not set globally
MX_REPORT_STATE_OVERRIDE=          # spawn-managed parent status directory, distinct from a persistent sub-agent's own operational MX_HOME/state
MX_AGENT_GH_TOKEN=                 # retired launch override; configure ordinary GH_TOKEN/gh/SSH authentication instead
MX_DELIVERY_GH_TOKEN=              # optional explicit publication token for mx-deliver.sh
MX_DELIVERY_GH_CONFIG_DIR=         # optional absolute isolated gh config for mx-deliver.sh; mutually exclusive with MX_DELIVERY_GH_TOKEN
MX_NUDGE=1                         # set to 0 to disable mx-report's best-effort watcher nudge without changing durable writes
MX_NUDGE_DEBUG=0                   # set to 1 to print otherwise-silent watcher-nudge diagnostics from mx-report
MX_STALE_ESCALATE_SECS=240         # idle seconds before a provably-working stale pane escalates; stale panes whose sub-agent is not provably working surface immediately unless they declare the pause verb
MX_PAUSE_RESURFACE_SECS=3600       # seconds before an idle declared external wait re-surfaces for a recheck in the watcher or away-mode service daemon
MX_WEDGE_DEMAND_INSPECT_COUNT=3    # consecutive provably-working stale escalations on the same unchanged pane before demand-deep-inspection is added
MX_WATCH_TRIAGE_LOG_MAX_BYTES=262144   # size cap for the watcher's absorbed-wake debug log
MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT=     # optional seconds allowed for bootstrap's best-effort clone refresh; unset/blank defaults to max(20, 5 + 3 * origin-backed-project-count)
MX_SYSTEM_PRUNE=1        # set to 0 to skip pruning local branches whose upstream is gone
MX_STALE_WORKTREE_LOCK_AGE_SECS=30       # min mtime age before mx-teardown.sh treats a leftover worktree git index.lock as provably stale
MX_WORKTREE_LOCK_RETRIES=3        # rechecks before proving a Git index.lock stale
MX_WORKTREE_LOCK_RETRY_WAIT_SECS=1 # seconds mx-teardown.sh waits before each retry after that signature
MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS=   # legacy alias for MX_WORKTREE_LOCK_RETRY_WAIT_SECS when the new variable is unset
MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRIES=3        # fetch retries after mx-system-sync.sh hits the orphaned .git/packed-refs.lock signature
MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRY_WAIT_SECS=1 # seconds mx-system-sync.sh waits before each of those retries
MX_SYSTEM_SYNC_PACKED_REFS_LOCK_AGE_SECS=30       # min mtime age before mx-system-sync.sh treats a leftover packed-refs.lock as provably stale
MX_BUSY_REGEX='esc (to )?interrupt|Working\.\.\.|Ctrl\+c:cancel'   # busy-pane signatures, shared by watcher, mx-actor-state pane fallback, and tmux helper
MX_COMPOSER_IDLE_RE=    # optional empty-composer regex, applied after ghost and border stripping
MX_COMPOSER_GHOST_LUMA_MAX=128   # system-wide: max perceived luminance (0.299R+0.587G+0.114B, 0-255) for a TRUECOLOR foreground to count as de-emphasised ghost/placeholder text and be stripped; dim/faint (SGR 2) is stripped regardless. Assumes a dark terminal theme (bin/mx-composer-lib.sh's mx_composer_strip_ghost, shared by the tmux and herdr composer readers)
MX_SEND_RETRIES=3       # mx-send Enter-retry attempts after typing the line once
MX_SEND_SLEEP=0.4       # seconds between mx-send submit checks
MX_SEND_SETTLE=1        # seconds mx-send waits after a successful text submit; 0 disables
MX_PENDING_REPLY_GRACE_SECS=120   # seconds after marked-request delivery before a completed turn without a correlated parent report is eligible for its one recovery repost
# service daemon (bin/mx-supervise-daemon.sh); presence-gated via /afk
MX_SUPERVISOR_BACKEND=             # optional supervisor pane backend override; tmux/herdr only, otherwise detects $TMUX_PANE then HERDR_ENV/HERDR_PANE_ID before tmux fallback
MX_SUPERVISOR_TARGET=              # optional supervisor pane target override; tmux target or herdr <session>:<pane-id>, otherwise auto-detected
MX_INJECT_SKIP=heartbeat           # |-prefixes force-self-handled bypassing classification; empty disables
MX_ESCALATE_BATCH_SECS=90          # buffer window for batched escalation digests; 0 = flush immediately
MX_MAX_DEFER_SECS=300              # max buffered escalation age before retry plus wedge alarm; 0 disables
MX_WEDGE_ALARM_CHANNEL=            # override config/wedge-alarm with one active-alert directive for the wedge alarm; off|auto|herdr|command:<cmd>; absent = auto (no built-in channel; the durable marker is the only signal)
MX_WEDGE_ALARM_EXEC=              # notifier seam: route every channel (herdr, command:) through this command as `<cmd> <channel> <summary>`; "discard" fires nothing; unset in production; the service daemon defaults it to "discard" when sourced so no test posts a real notification
MX_WEDGE_ALARM_TIMEOUT_SECS=10    # maximum seconds for each herdr, override, or command: notifier before its watchdog terminates it and continues to the next channel; invalid or zero values use 10
MX_INJECT_FAIL_SLEEP=30            # seconds to back off when the supervisor pane is unavailable
MX_INJECT_CONFIRM_RETRIES=3        # service-daemon Enter-retry attempts after typing a digest once
MX_INJECT_CONFIRM_SLEEP=0.5        # seconds between service-daemon submit checks
MX_HEARTBEAT_SCAN_SECS=300         # cadence of the catch-all status scan for missed maintainer verbs
MX_HOUSEKEEPING_TICK=15            # seconds between batch-flush, stale/pause-recheck, and scan passes
MX_CRASH_THRESHOLD=10              # watcher crashes allowed inside MX_CRASH_WINDOW before service-daemon backoff
MX_CRASH_WINDOW=60                 # seconds in the crash-loop detection window
MX_CRASH_BACKOFF=60                # seconds to wait after crossing the crash threshold
MX_CRASH_NORMAL_SLEEP=5            # seconds to wait after an isolated watcher crash
MX_LOG_MAX_BYTES=1048576           # service-daemon log size that triggers trimming
MX_LOG_KEEP_LINES=2000             # service-daemon log lines kept when trimming
```

`mx-teardown.sh` rechecks a present Git index lock up to `MX_WORKTREE_LOCK_RETRIES` times before applying stale-lock proof.
`MX_WORKTREE_LOCK_RETRIES` accepts a nonnegative integer, and an unset, blank, or invalid value uses the default of 3.
`MX_WORKTREE_LOCK_RETRY_WAIT_SECS` accepts nonnegative whole or fractional seconds between attempts.
When it is unset or blank, `MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS` remains a compatible fallback, and a blank fallback uses the 1-second default.
An invalid nonblank wait falls back to 1 second rather than interrupting teardown.
Teardown never removes a lock during the retry window, and after that window it attempts stale-lock cleanup only for a still-present lock that passes the configured age and live-holder checks.

`mx-system-sync.sh` applies the same shape to an orphaned `.git/packed-refs.lock`: it retries only Git's `Unable to create '...packed-refs.lock': File exists` fetch failure up to `MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRIES` times (nonnegative integer; unset, blank, or invalid uses the default of 3), waiting `MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRY_WAIT_SECS` seconds (nonnegative whole or fractional; invalid falls back to 1 second) before each.
Only after those retries exhaust does it remove the lock, and only when it is provably stale - still present, mtime age at least `MX_SYSTEM_SYNC_PACKED_REFS_LOCK_AGE_SECS` (default 30), and no `lsof` holder of the lock file or of the clone worktree itself (a live `git` keeps that as its cwd even in the window after it closes the lock and before it exits).
A live lock, a missing `lsof`, any failed check, or any other fetch failure keeps today's behavior.
Every wait, retry, and removal is printed to stderr, and a successful recovery also prints one `recovered:` summary line to stdout so a session-start refresh - which discards system-sync stderr and relays only stdout - still surfaces it.
The shared staleness proof lives in `multplx-core`; the Rust teardown and system-sync lifecycle paths use that one fail-closed proof.

## Persistent-home inheritance

The [inheritance module](../crates/multplx-domain/src/inheritance.rs) owns the allowlist, byte validation, per-home lock and generation publication.
The current allowlist contains `config/subagent-dispatch.json`, `config/subagent-harness`, `config/persistent-subagent-harness`, the legacy `config/actor-dispatch.json` and `config/actor-harness` aliases, `config/backlog-backend`, `config/herdr-presentation-spaces` and `data/maintainer-shared.md`.
`config/daemon-harness` and `data/learnings.md` remain home-local.
An inherited literal `default` harness resolves against the child's own harness, not the parent's effective choice.
Shared preference copies are read-only in children; divergent bytes are quarantined before replacement and are never copied back to the parent.
Missing source material converges to absence through the same owner; unsafe links and nonordinary files are rejected.
Config changes publish bounded private generations containing the exact destination bytes or `ABSENT`, and send only a routed pointer.
Pending generations survive failed publication or send; unchanged configuration causes no new message unless delivery is pending.
Relaunch supersedes old pending rereads because the new execution reads configuration afresh; failed cleanup is quarantined.
A delivered pointer is transport evidence, not proof of model acknowledgement.
The guarded local tracked-file fast-forward is separate and does not fetch or modify private task state.
Use `bin/mx-config-push.sh --help` for mid-session convergence and the existing update command for origin refresh.
The optional [persistent operations reference](../.agents/skills/persistent-subagents/SKILL.md) provides command discovery.
