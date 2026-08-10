# Configuration

The files and environment variables you set to operate broker.

[Back to the documentation index](README.md).

## Orchestrator behavior (`AGENTS.md`)

The shared orchestrator behavior contract lives in [`AGENTS.md`](../AGENTS.md) and is auto-loaded by supported coding agents.

## Operational home layout and state

This section is the single owner of the top-level operational-home layout; producer script headers and their help own exact child-file fields and mutation contracts.
The tracked code root contains the shared instruction, skill, documentation, workflow, and `bin/` surfaces, while each effective `MX_HOME` contains private operational directories.
`data/` holds durable private system records such as the project and daemon registries, maintainer preferences, optional shared maintainer preferences, learnings, backlog, briefs, and scout reports.
`state/` holds volatile runtime records such as task metadata, append-only status events, endpoint signals, watcher and wake-queue coordination, away-mode state, private daemon config-reread generations with their retry and quarantine state, and parent-owned daemon pending-reply records under `state/pending-replies/` (`bin/mx-pending-reply-lib.sh`).
`config/` holds local gitignored operating choices, and `projects/` holds the local project clones that Multplx reads but changes only through the guarded exceptions defined in `AGENTS.md`.

`bin/mx-spawn.sh` owns the base task-metadata fields it emits, while the runtime-backend section below owns backend-specific fields and selector interpretation.
The producing PR helpers own the fields they append, `bin/mx-classify-lib.sh` owns status-event vocabulary, and `bin/mx-actor-state.sh` owns current-state reconciliation.
Wake, watcher, and away-mode state mechanics remain with their named scripts and reference sections rather than being duplicated into one exhaustive state tree here.

`bin/mx-session-start.sh`'s header is the single owner of session-start ordering, composed commands, digest contents, and the digest's startup mechanism.
`docs/sessionstart-nudge.md` owns the native session-open adapter mechanics that nudge the digest command.
`AGENTS.md` owns the run-once and read-once operator rules, lock-refusal safety, installation consent, and direct-report recovery boundaries because those facts apply at every session start.
Ordinary dead-direct-report recovery is owned by `stuck-actor-recovery`, while persistent-daemon recovery is owned by `daemon-provisioning`.

## Global launcher paths and activation

The global launcher gives one persistent multi-project control plane two explicit path identities.
`MX_ROOT_OVERRIDE` names the plain tracked checkout that supplies `AGENTS.md`, harness configuration, skills, extensions, workflows, documentation, and scripts.
`MX_HOME` names the persistent operational home whose `config/`, `data/`, `projects/`, and `state/` directories retain all private configuration, registries, clones, artifacts, queues, reports, and runtime state.
The launcher canonicalizes and validates both paths on every command and rejects a linked task worktree as the code root.
A managed code root must remain clean, while an adopted checkout remains available for ordinary development edits whether it shares or separates its operational home.

The installer stores one literal absolute path per file at `${XDG_CONFIG_HOME:-$HOME/.config}/multplx/root` and `home`.
Those files are parsed as data and are never sourced or evaluated as shell code.
The stable bootstrap lives at `${XDG_BIN_HOME:-$HOME/.local/bin}/multplx`; managed runtime and home defaults live below `${XDG_DATA_HOME:-$HOME/.local/share}/multplx/`.
The managed installer records `multplx.managed=true` in the runtime clone's local Git configuration, so launcher cleanliness checks apply only to installer-owned runtimes and never conflate an adopted checkout with a separately selected home.
Custom directories are available through the installer's help and are recorded into the bootstrap atomically.

Activation sets `MULTPLX_ACTIVE=1`, `MX_ROOT_OVERRIDE`, and `MX_HOME`, captures the real Claude, Codex, Cursor, and Pi paths before adding shims, and then replaces the launcher with the user's interactive shell.
Bash and Zsh source the user's ordinary interactive rc once before re-prepending the harness shims and composing one static prompt/title marker.
Other tested POSIX shells retain the environment and shims with a one-line banner, while unsupported shells refuse explicitly.
Nested activation refuses rather than stacking prompt, path, or environment layers.
The prompt marker is presentation only: it runs no command, reads no file or state, opens no socket, and does not indicate lock ownership.

Typing `claude`, `codex`, `agent`, `cursor-agent`, or `pi` in the activated shell executes the captured real binary from the code root in a child process.
The activated shell and its caller stay in their original directory.
The harness shim performs only the existing read-only lock-status preflight; `bin/mx-lock.sh` and `bin/mx-session-start.sh` remain the sole lock and startup authorities.
A known different live harness holder refuses before the real binary starts, while stale or uncertain state remains for session start to adjudicate conservatively.

The launcher does not infer a project from the caller's directory and never imports or registers it automatically.
Broker harness, worker harness/model/effort, runtime backend, and request project remain independent selection axes.
Ambient `TMUX`, Herdr, and cmux identifiers pass through unchanged unless the user supplies the launcher's session-scoped backend selector.
[Launcher verification](verification/launcher.md) holds current shell, harness, path, lock, and performance evidence.

## Pi Calm preference (config/calm)

The Pi Calm extension stores the maintainer's home-local presentation choice in gitignored `config/calm` under the effective Multplx home, resolved from `MX_HOME`, then `MX_ROOT_OVERRIDE`, then the tracked code root derived from the extension path, or under `MX_CONFIG_OVERRIDE` when that test and specialized-setup override is present.
The only values it writes are `on` and `off`, each followed by one newline; an absent, unreadable, or unrecognized value defaults to off.
The `/calm` command replaces the file atomically before changing live presentation, so a failed write leaves the current choice unchanged rather than claiming persistence.
The extension reloads this preference on every Pi `session_start`, including startup, new, resume, fork, and reload reasons.
This preference is local to each Multplx home and is not part of daemon inherited configuration.

## Backlog backend (config/backlog-backend)

The in-repo `bin/mx-backlog-lib.sh` is the single owner of the markdown backlog schema, parsing rules, mutation semantics, and retention defaults.
It stores live work in `data/backlog.md`, keeps the newest 10 Done items inline by default, and moves retention overflow to `data/done-archive.md`.
When the default backend is selected, broker uses the library for routine backlog mutations with no external package or version probe.
Daemon handoffs are separate and unconditional: `mx-backlog-handoff.sh` keeps its system-level validation and routes the item move through the library's atomic `mx_backlog_mv`.
It moves in-scope `## Queued` items only and refuses `## In flight` and historical `## Done` records, which stay with their home for pruning or archiving.
Handoff item bodies must use at least two leading spaces, and the helper refuses a selected item with a single-space or tab-indented continuation rather than risk orphaning it.
The `config/backlog-backend=manual` knob governs broker's own hand-editing of its backlog, not this validated handoff helper.
Set the local, gitignored `config/backlog-backend` file to `manual` to force manual backlog editing; absent or `owned` selects the in-repo library.
The file format is unchanged in both modes; the library and manual edits produce the same `## In flight`, `## Queued`, and `## Done` sections.
Use `bin/mx-backlog.sh` for routine list, show, add, done, ready, hold, update, block, unblock, move, and validation operations.

## Runtime backend (config/backend / MX_BACKEND)

For spawn-capable adapters, the runtime session-provider backend controls where task windows/endpoints are created, captured, sent to, watched, and killed.
`tmux` is the verified reference backend (see [`docs/tmux-backend.md`](tmux-backend.md)); `herdr` and `cmux` are experimental spawn backends (see [`docs/herdr-backend.md`](herdr-backend.md) and [`docs/cmux-backend.md`](cmux-backend.md)).
Treehouse remains the worktree provider for tmux, herdr, and cmux, since herdr and cmux are session providers only.
New spawns choose the backend in this order: an explicit `--backend` flag broker passes when it spawns a task, then `MX_BACKEND`, then the first non-empty line of local gitignored `config/backend`, then runtime auto-detection from `$TMUX`, `HERDR_ENV=1`, or cmux runtime signals, then default `tmux`.
If more than one runtime marker is present, detection resolves innermost-first: `$TMUX` is checked before `HERDR_ENV=1`, which is checked before cmux's primary `CMUX_WORKSPACE_ID` marker and its documented fallback signals - tmux or herdr started from inside a cmux terminal is the innermost, currently-executing layer, while cmux itself (a terminal application, not a nestable multiplexer) is always checked last.
See [`docs/cmux-backend.md`](cmux-backend.md#runtime-detection) for why cmux can be selected when `CMUX_WORKSPACE_ID` is absent.
Auto-detected herdr or cmux prints a stderr notice naming `config/backend` and `--backend tmux` as opt-outs; auto-detected tmux stays silent to preserve existing default behavior.
Any value other than `tmux`, `herdr`, or `cmux` is rejected until another adapter is implemented and verified.
`mx-spawn.sh` accepts `tmux`, `herdr`, and `cmux` for delivery and scout tasks; `backend=cmux` still refuses `--daemon` until daemon launch semantics are designed.
`codex-app` is not an accepted runtime backend yet; [`docs/codex-app-backend.md`](codex-app-backend.md) owns the Codex App boundary.
The session-start daemon liveness sweep uses the recovery-grade `mx_backend_agent_state` classifier where verified.
The comment above that function in `bin/mx-backend.sh` is the single owner of its detailed state contract and recovery authorization.
The compatibility helper `mx_backend_agent_alive` continues to collapse those detailed results to `alive`, `dead`, or `unknown` for older callers.
A herdr spawn additionally version-gates against the installed `herdr` binary's protocol and requires `jq`, refusing loudly on an incompatible or missing installation.
A cmux spawn additionally version-gates against the installed `cmux` binary's version, requires `jq`, and requires the control socket to be reachable and accessible (see [`docs/cmux-backend.md`](cmux-backend.md) "Setup" for the one-time socket-access configuration this needs; Automation mode is the recommended socket control mode, with Password mode supported via `config/cmux-socket-password`), refusing loudly and non-retryably on a `cmuxOnly`/unauthenticated socket.
A backend spawn refusal from a missing dependency, version gate, or unauthenticated socket is terminal for that selected backend; broker surfaces it as a blocker instead of silently retrying another backend.
Task meta records `backend=` only for a non-default backend; an absent `backend=` means `tmux`, preserving existing default-path meta files.
A herdr task additionally records `herdr_session=`, `herdr_workspace_id=`, `herdr_tab_id=`, and `herdr_pane_id=`.
A cmux task additionally records `cmux_workspace_id=` and `cmux_surface_id=`.
Task selectors for `mx-peek.sh`, `mx-send.sh`, and `mx-actor-state.sh` resolve centrally through `mx_backend_resolve_selector`.
A selector containing `:` is passed through as an explicit backend endpoint escape hatch.
Otherwise an exact task id matching `state/<id>.meta` wins before the legacy `mx-<id>` label fallback, so task ids that themselves start with `mx-` route to their own metadata instead of being stripped.
A metadata-routed selector returns the recorded backend target (`window=`), and matching explicit targets can still recover the recorded backend when metadata contains the same endpoint.
Only metadata-routed task selectors carry daemon-marker and Codex-harness context; explicit endpoint escape hatches do not.
These five sentences are the single owner of the task-selector vocabulary; backend guides and other documents point here instead of restating the resolution order.
`mx-teardown.sh <id>` takes a task id directly and uses the same recorded backend target fields after loading `state/<id>.meta`.
By default, Herdr workspaces are derived from `MX_HOME`: the primary home uses `broker`, and a daemon home marked by `.mx-daemon-home` uses `daemon-<daemon-id>`.
The default-container spawn, list-live, and recovery paths read that label from the active home, so a daemon's own actors stay inside that daemon home's herdr space.
The optional local `config/herdr-presentation-spaces` presence flag instead enables Herdr's default-off disposable single-task visual projection; [Optional presentation spaces](herdr-backend.md#optional-presentation-spaces) owns its behavior, safety limits, recovery contract, and narrow locked session-start cleanup of exact restored idle-shell children.
The flag is default-off and inherited into daemon homes under the primary-authoritative contract owned by [`daemon-provisioning`](../.agents/skills/daemon-provisioning/SKILL.md).
For normal herdr operations, `HERDR_SESSION` selects the named session, but destructive test cleanup must not rely on `HERDR_SESSION` alone.
Use the explicit guarded cleanup path described in [`docs/herdr-backend.md`](herdr-backend.md) instead of `herdr server stop`.
cmux has no session layer at all - one workspace per task, in whatever cmux window is open - and its socket password (when configured) is read from local, gitignored `config/cmux-socket-password` under the effective config directory, never committed.
The caller-facing label remains `mx-<id>`, but the actual cmux workspace title is scoped by the active `MX_HOME` readable label plus a short hash of the resolved `MX_ROOT` path as `mx-<home-label>-<id>`.
Test cleanup must use the guarded path in [`docs/cmux-backend.md`](cmux-backend.md#current-operation-and-safety), never enumerate-and-close every workspace.
The `config/backend` file is not inherited by daemon homes.

## Away-mode supervisor backend (MX_SUPERVISOR_BACKEND / MX_SUPERVISOR_TARGET)

The `/afk` sub-supervisor injects escalation digests into broker's own pane independently of where new task endpoints are spawned.
It currently supports only `tmux` and `herdr` supervisor panes.
Set `MX_SUPERVISOR_BACKEND=tmux|herdr` and `MX_SUPERVISOR_TARGET=<target>` to override both axes explicitly; for herdr the target is `"<session>:<pane-id>"`.
Without overrides, backend detection uses `$TMUX_PANE` first, then `HERDR_ENV=1` with `HERDR_PANE_ID`, then falls back to `tmux`.
That keeps a tmux pane nested inside herdr on the tmux transport, matching the runtime backend's innermost-first rule.
Target detection uses `MX_SUPERVISOR_TARGET`, then `$TMUX_PANE`, then `"${HERDR_SESSION:-default}:${HERDR_PANE_ID}"` under herdr, then the legacy `broker:0` tmux fallback with a warning.
Selecting any other supervisor backend, including `cmux`, refuses at daemon startup instead of trying tmux injection primitives against a non-tmux pane.

## Away-mode wedge alarm channels (config/wedge-alarm)

When away-mode injection wedges past `MX_MAX_DEFER_SECS`, the sub-supervisor raises a loud, rate-limited alarm.
Beyond the durable `state/.subsuper-inject-wedged` marker and the tmux status-line flash, it attempts a configured backend-independent active alert that can reach the maintainer even when every pane and its backend status-line is unreadable.
`config/wedge-alarm` (local, gitignored) lists channel directives, one per non-empty, non-comment line; every listed non-`off` channel fires, best-effort.
`MX_WEDGE_ALARM_CHANNEL` overrides the file with a single directive.
Directives are `off` (a position-independent kill switch that disables every active alert), `auto`/`default`, `herdr` (herdr UI notification), and `command:<cmd>` (run `<cmd>` via `sh -c`, summary on `$1` and stdin).
An absent file means `auto`: no platform has a built-in OS channel, so the durable marker is the only signal until a channel is configured; the alarm fires at most once per max-defer window after a genuine wedge.
A missing or failing channel logs and falls through to the next, never crashing the daemon.
See [`verification/supervision.md`](verification/supervision.md#wedge-alarm-channels) for active evidence and [`examples/wedge-alarm`](examples/wedge-alarm) for a copyable config.

## Gate defaults (.deep-review.yaml)

The tracked `.deep-review.yaml` is the project policy read by `bin/mx-deep-review.sh`.
Code-executing commands, the command-permission flag, project-settings suppression, and document instructions are loaded from the trusted default-branch copy.
The reviewed branch may supply cosmetic fields, but its commands are inert unless the trusted copy explicitly sets `allow_repo_commands: true`.
The Multplx default keeps repository commands empty, relies on the gate's focused fallback validation, and keeps evidence in private `state/<id>.gate/` records rather than the branch.
It must not set `commands.test` to a complete `tests/*.test.sh` walk or `bin/mx-test-run.sh --all`.
See [CONTRIBUTING.md](../CONTRIBUTING.md) for the Multplx-specific local test policy and entry points.
Portable shard evidence and coverage rules are in [mx-test-portable-shards.md](mx-test-portable-shards.md); [herdr-backend.md](herdr-backend.md#destructive-lab-safety) owns the real-Herdr lane's isolation boundary, and [runtime-backends.md](verification/runtime-backends.md#herdr) owns active evidence.

## Maintainer Preferences (data/maintainer.md / data/maintainer-shared.md)

Domain-local preferences for one maintainer's system live locally in each home's `data/maintainer.md`; it is gitignored and printed in the session-start context digest after `data/projects.md` and optional `data/daemons.md`.
Before changing it, inspect the current file and rewrite or prune the matching bullet in place; add a new bullet only for a genuinely new durable preference.
Shared maintainer preferences that apply across daemon domains live only in the primary home's optional `data/maintainer-shared.md`.
`daemon-provisioning` owns its propagation contract, including the required header, read-only daemon copies, quarantine diagnostics, and the rollout rule that existing homes trim `data/maintainer.md` by hand after first propagation rather than deleting private content automatically.

## Operational learnings (data/learnings.md)

System-local operational facts and gotchas live locally in `data/learnings.md`; it is gitignored and printed after the maintainer-preference files in the session-start context digest.
The file is created lazily on first learning and follows the same dated, evidence-backed, curated style as `data/maintainer.md`: inspect the current file first, then rewrite or prune stale entries instead of appending forever.
There is no shared learnings file by maintainer decision.

## Daemon routes (data/daemons.md)

Persistent daemon routes live locally in `data/daemons.md`.
The concise single-line route contract is owned by the [`daemon-provisioning` skill](../.agents/skills/daemon-provisioning/SKILL.md#routing-table), including the parser-compatible fields, one-sentence summary requirement, `home:` pointer to the seeded charter, and limit on extra registry prose.
`mx-home-seed.sh validate` refuses duplicate ids, duplicate homes, and nested or overlapping homes.
The main broker routes by reading those scopes with judgment; the project list is provisioning data, not exclusive ownership.
Use `mx-home-seed.sh <id> - {<project>...|--no-projects}` to lease a fresh broker worktree for the daemon home.
Use the deliberate `--no-projects` signal only for a Multplx-repo domain that needs no separate project clones.
It cannot be combined with a project list, and omitting both still fails loudly.
A project-less seed requires no existing project clones or `data/projects.md` entries in the home, so it refuses a populated-home conversion without changing that home.
A preexisting project-bearing charter is also refused until it is re-scaffolded with `--no-projects` or removed.
The lease is held under the daemon id until explicit retirement or seed rollback returns it, so normal restarts do not free or recycle the home.
Teardown of a leased home fails closed if `treehouse return` cannot release the lease; plain-clone homes with no treehouse pool slot are removed directly.
Daemon routes cover `deep-review` and `direct-PR` projects; `local-only` projects remain main-broker work.
The deep-review gate is an in-repo script and requires no per-clone initialization during seeding.
After creating a daemon, move existing main-backlog queued items that you have judged in-scope with `mx-backlog-handoff.sh <daemon-id> <item-key>...`; it is idempotent and refuses In flight, Done, or non-daemon homes.
Set `MX_DAEMON_CHARTER` to seed from inline charter text when no filled charter brief exists; set `MX_DAEMON_SCOPE` when the routing scope should differ from the charter text.
The seeded home's `data/charter.md` owns the standard daemon lifecycle and escalation contract; the route file points to it through the existing `home:` field instead of adding another pointer.
Each seed writes an `.mx-daemon-home` identity marker at the home root.
The tracked root `.gitignore` ignores that marker, so validation can read it without making a freshly seeded home appear dirty to porcelain-based safety checks.
This does not relax protection for any other untracked file.
An existing linked-worktree home that predates this rule advances through its marker-only state during its next bootstrap or spawn local sync, after which Git ignores the marker normally.
A standalone-clone home cannot receive a primary-local commit through that no-fetch sync, so it receives the rule through `/updatemultplx`'s origin refresh instead.

## MX_HOME

`MX_HOME` selects the operational home for one broker instance.
When it is unset, most scripts use the repo root as the home; when it is set, scripts still run from this repo's `bin/`, but `state/`, `data/`, `config/`, and `projects/` come from `$MX_HOME`.
`MX_ROOT_OVERRIDE` overrides the Multplx repo root used by scripts, including the primary checkout watched by the worktree-tangle guard.
When `MX_HOME` is unset, it also behaves as the old whole-root override.
`bin/mx-send.sh` is intentionally stricter than that general fallback: it requires `MX_HOME` to be set before resolving a target, so operator steers cannot silently resolve against the wrong home.
`MX_STATE_OVERRIDE`, `MX_DATA_OVERRIDE`, `MX_PROJECTS_OVERRIDE`, and `MX_CONFIG_OVERRIDE` override individual operational directories for tests and specialized harness setup.
For the herdr backend, `MX_HOME` also determines the workspace label used by the adapter.
For the cmux backend, `MX_CONFIG_OVERRIDE` overrides where `config/cmux-socket-password` is read from, while `MX_HOME` determines the default config path and readable home prefix embedded in workspace titles.
The full cmux home label also includes a short hash of the resolved `MX_ROOT` path, and there is no per-home container split.

## Harness support

claude, codex, cursor, and pi are empirically verified; new harnesses get verified through a monitored trial task before joining the set.
The trusted project-level [`.codex/config.toml`](../.codex/config.toml) selects `sandbox_mode = "danger-full-access"` for Codex primary sessions because session locking, host-capacity checks, runtime backend control, and actor launch require host operations that the default command sandbox denies.
The project setting does not change `approval_policy`; Codex approval prompts remain under the maintainer's user-level or command-line policy.
The verified adapter knowledge - busy signatures, interrupt and exit commands, skill-invocation syntax, and per-harness quirks - lives in [`.agents/skills/harness-adapters/SKILL.md`](../.agents/skills/harness-adapters/SKILL.md).
Launch mechanics, including the verified command templates, live in [`bin/mx-spawn.sh`](../bin/mx-spawn.sh).
Primary-session turn-end guard integrations for verified harnesses are tracked as repo-level hook files and documented in [`docs/turnend-guard.md`](turnend-guard.md).
Primary-session watcher wake protocols are rendered at session start by [`bin/mx-supervision-instructions.sh`](../bin/mx-supervision-instructions.sh) from [`docs/supervision-protocols/`](supervision-protocols/).
Claude's Stop `asyncRewake` hook owns tokenless re-arm cycles, Codex and Cursor use bounded foreground checkpoints, and Pi uses its two tracked primary extensions.
`config/actor-harness` is a local, gitignored file containing one adapter name for actor and scout launches.
When it is absent or contains `default`, actors mirror the broker's own harness.
`config/daemon-harness` is a separate local, gitignored file containing the adapter the primary uses to launch daemon agents, optionally followed by model and effort tokens on the same line.
The first non-empty, non-comment line is parsed as `<harness> [<model>] [<effort>]`.
A bare `<harness>` preserves the previous behavior: harness only, with no model or effort launch flag.
When the harness token is absent or `default`, daemon launch falls back through `config/actor-harness` and then the primary's own harness, and no model or effort is read from that file.
`mx-harness.sh daemon-model` and `mx-harness.sh daemon-effort` expose only the optional tokens from `config/daemon-harness`; `config/actor-harness` remains a bare adapter-name file.
An explicit harness argument to `mx-spawn.sh` still overrides either config file for that spawn only.
An explicit `--model` or `--effort` overrides the matching token from `config/daemon-harness`; an explicit harness or raw launch command starts with clean model and effort defaults unless those flags are also passed.
When `config/actor-dispatch.json` exists, actor and scout spawns require an explicit resolved harness instead of automatically falling back to `config/actor-harness`.
The inherited-local-material contract is owned by [`daemon-provisioning`](../.agents/skills/daemon-provisioning/SKILL.md); its harness-relevant consequence is that a daemon's own actors use the primary's dispatch profiles and static harness value.
Those inherited values are defaults and rules only; `mx-spawn` still permits a consciously chosen explicit runtime outside the config.
`config/daemon-harness` is not inherited because daemons do not launch daemons.
For Pi daemon launches, `mx-spawn.sh` starts Pi with `-e` pointed at the daemon home's own tracked `.pi/extensions/mx-primary-pi-watch.ts` and `.pi/extensions/mx-primary-turnend-guard.ts`, both already present from the daemon home's git worktree.
For Cursor launches, `mx-spawn.sh` always passes `--sandbox enabled --trust`; actor turn-end signaling comes from a private per-run plugin and primary behavior comes from tracked `.cursor` rules and hooks.
Cursor model effort is encoded as `<model>[effort=<level>]`; use `agent models` in the authenticated account before choosing a named model.
Cursor deep-review is deliberately unsupported because schema enforcement and project-rule suppression are not verified together.
[Cursor CLI verification](verification/cursor-cli.md) owns the dated version, authentication, sandbox, hook, resume, daemon, and negative-control evidence.

## Actors dispatch profiles (config/actor-dispatch.json)

`config/actor-dispatch.json` is an optional local, gitignored file containing natural-language rules that broker reads before dispatching an actor or scout.
The shell scripts do not match those rules; broker chooses the best matching rule with judgment, resolves its profile object or array under the operating contract in `AGENTS.md` section 4, and passes only concrete `--harness`, `--model`, and `--effort` flags to `mx-spawn.sh`.
When the file exists, `mx-spawn.sh` enforces that contract by refusing actor and scout spawns that lack an explicit harness (`--harness`, a positional adapter, or a raw launch command).
Batch spawns satisfy the same requirement with a shared `--harness`.
Daemon spawns are exempt and still resolve through `config/daemon-harness` and its optional model and effort tokens.
This section is the single owner of the canonical schema and its per-field semantics; `AGENTS.md` section 4 owns the dispatch and array-selection procedure.

```json
{
  "rules": [
    {
      "when": "<natural-language condition describing a kind of task>",
      "use": [
        { "harness": "<adapter>", "model": "<optional model>", "effort": "<low|medium|high|xhigh|max, optional>" }
      ],
      "why": "<optional rationale that helps broker choose>"
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
Every profile array is an implicit capacity-aware choice.
If no dispatch rule fits, broker resolves `default` through the same object-or-array path before falling back to `config/actor-harness`.
If a selected profile carries an effort value the chosen harness does not accept, `mx-spawn.sh` records the requested `effort=` in task meta for traceability but omits the launch flag, and bootstrap reports the invalid harness/effort pair as a `ACTOR_DISPATCH` diagnostic when it is visible in the file.
See [`docs/examples/actor-dispatch.json`](examples/actor-dispatch.json) for a starting point to copy into local `config/actor-dispatch.json`.
When the file exists, bootstrap validates it with `jq`.
Valid files stay silent by default; with `MX_BOOTSTRAP_VERBOSE_FACTS=1`, bootstrap emits `BOOTSTRAP_INFO: actor dispatch active config/actor-dispatch.json`, one `BOOTSTRAP_INFO:` fact per rule, and one fact for the optional default profile set.
Malformed JSON, an empty or malformed rule/default array, an unverified harness, or an effort value unsupported by that harness is reported as `ACTOR_DISPATCH: invalid config/actor-dispatch.json - ...`; missing `jq` is reported through the normal `MISSING: jq` install-consent flow.
While the file remains present, no actor or scout spawn may proceed without an explicit resolved harness; malformed configuration must be reported and corrected rather than selected around.
Daemon homes inherit this file from the primary, so a daemon's own actors apply the same dispatch profile behavior.

## Dispatch capacity (config/api-capacity / state/.dispatch-queue)

`bin/mx-headroom.sh` is the single owner of dispatch-capacity calculation and parked-request record fields.
Its JSON combines spare CPU and available memory with a conservative configured API concurrency budget, and the tighter component controls `available` and `at_limit`.
The default local reservation is one-quarter logical CPU and 256 MiB per additional actor, while `MX_HEADROOM_CPU_PER_ACTOR` and `MX_HEADROOM_MEM_PER_ACTOR_BYTES` retain explicit overrides.
The optional global API budget is the nonnegative integer in `config/api-capacity`, with per-harness refinements in `config/api-capacity-<harness>`; absent configuration uses a capacity of twenty.
This API signal is deliberately labeled `configured-budget`, not live provider quota.
An unreadable local signal, malformed budget, or unaccounted configured candidate is an error rather than permission to guess.
At limit, `bin/mx-spawn.sh` writes one private record per task under `state/.dispatch-queue/` and returns a queued outcome without allocating a worktree or endpoint.
The watcher checks fresh headroom on each poll and launches at most the oldest one, preserving FIFO and leaving every record untouched while capacity remains unavailable.
Use `bin/mx-headroom.sh --queue` to inspect parked requests and `bin/mx-headroom.sh --queue-cancel <id>` to cancel one exact task.

## Toolchain

On session start the broker detects what its required toolchain is missing or too old and lists each problem with either an exact install command or manual instructions.
It installs automatically supported tools only after you say go; manual-only tools remain for you to install from the printed instructions.
Required tools come in two parts: a universal toolchain every home needs regardless of backend, and a per-backend delta that follows the runtime backend actually resolved for this home.
The universal toolchain is node, git, gh, jq, and Treehouse with durable `get --lease` support.
[`upstream.md`](upstream.md#pinned-external-dependencies) owns Treehouse's exact version pin and points to the verified installer.
This section is the single owner of that universal toolchain list; backend guides' prerequisites point here and add only their backend-specific tools.
The in-repo deep-review scripts supply local validation, while official gh covers read-only agent operations plus credentialed non-agent delivery.
Bootstrap does not require GitHub authentication in the broker session.
The credential boundary and delivery-context setup are documented in [delivery.md](delivery.md).
The in-repo vplan module covers rich-review operations and is self-checked with its vendored assets rather than probed as an external tool.
Backlog mutations and dispatch capacity are owned by the repository's `bin/mx-backlog-lib.sh` and `bin/mx-headroom.sh`.
The per-backend delta is required only for the backend resolved from `MX_BACKEND`, then `config/backend`, then runtime auto-detection, then default `tmux`, so a home is never told to install a tool an inactive backend or feature would need.
That delta is owned in code by `mx_backend_required_tools` in `bin/mx-backend.sh`: the resolved backend's own session-provider CLI (`tmux`, `herdr`, or `cmux`) plus `jq` for the JSON-emitting experimental adapters (`herdr`, `cmux`) whose spawn and liveness paths parse the backend's JSON output.
Backend tool availability uses the adapter's own executable resolver, so bootstrap and spawn agree on supported non-`PATH` locations such as cmux's bundled CLI.
An unknown resolved backend emits `BACKEND_INVALID` and blocks dispatch instead of silently dropping its dependency delta or falling back to tmux.
A herdr or cmux home is therefore never told `tmux` is missing, while Treehouse's command and durable-lease checks still run unconditionally because every supported backend delegates worktree acquisition to it.
When `config/actor-dispatch.json` exists, bootstrap also requires `jq` for dispatch profile validation.
Bootstrap self-checks that `bin/mx-headroom.sh --json` succeeds and emits valid JSON.
An unreadable local capacity signal or malformed configured API budget reports `HEADROOM_INVALID` and blocks dispatch.
Bootstrap also self-checks `bin/mx-vplan.sh`, its server syntax, seed template, review SDK, and pinned Mermaid hash.
An incomplete or corrupt bundled review module reports `VPLAN_INVALID`.
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
The locked session-start bootstrap step also runs the guarded local daemon sync for recorded live daemon homes, then propagates declared inherited local material into each validated live home.
It emits `DAEMON_SYNC:` only when a home was skipped for an actionable sync reason, inheritance failed, or a divergent shared maintainer-preference copy was quarantined.
When a running home advances and its loaded instruction surface (`AGENTS.md`, `bin/`, or `.agents/skills/`) changed, bootstrap sends the re-read nudge itself through the stable `mx-<id>` selector and reports the exact completed send as `BOOTSTRAP_INFO:`.
If that send fails, bootstrap keeps an idempotent retry marker and emits `NUDGE_DAEMONS:` with the failure reason.
The same bootstrap run emits `DAEMON_LIVENESS:` only when a registered daemon is skipped or its relaunch fails; already-live and successfully relaunched daemons are handled silently.
For a mid-session inherited local-material edit where tracked-file sync is not needed, run `bin/mx-config-push.sh`.
It uses the same live daemon discovery and propagation helper as bootstrap, prints each live home's `actor-dispatch.json`, `actor-harness`, `backlog-backend`, `herdr-presentation-spaces`, and `data/maintainer-shared.md` result as `pushed`, `unchanged`, `skipped`, or `error`, and exits non-zero for real propagation errors or config-reread send failures.
When an allowlisted config item changes for an already-running home, it sends the literal-content reread pointer described in [`daemon-provisioning`](../.agents/skills/daemon-provisioning/SKILL.md); unchanged allowlisted config sends no pointer unless a previous delivery is pending.
The locked bootstrap inheritance pass uses the same per-home changed-set and reread path for already-running homes; see `daemon-provisioning` for the single contract owner.
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
MX_BACKEND=             # optional runtime backend override for new spawns; tmux/herdr/cmux support delivery/scout spawns, codex-app is not accepted
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
MX_CLASSIFY_PAUSED_VERB=paused     # read-side compatibility override for legacy status logs; validated actor writes use the closed `paused` state
MX_TASK_ID=                        # spawn-managed task binding consumed by mx-report and mx-report-mcp.mjs; do not set globally
MX_REPORT_STATE_OVERRIDE=          # spawn-managed parent status directory, distinct from a daemon's own operational MX_HOME/state
MX_AGENT_GH_TOKEN=                 # optional remotely-enforced read-only token mapped into spawned agent GH_TOKEN; unset means no agent token
MX_DELIVERY_GH_TOKEN=              # optional service token accepted only by mx-deliver.sh outside agent sessions
MX_DELIVERY_GH_CONFIG_DIR=         # optional absolute isolated gh config for mx-deliver.sh; mutually exclusive with MX_DELIVERY_GH_TOKEN
MX_NUDGE=1                         # set to 0 to disable mx-report's best-effort watcher nudge without changing durable writes
MX_NUDGE_DEBUG=0                   # set to 1 to print otherwise-silent watcher-nudge diagnostics from mx-report
MX_STALE_ESCALATE_SECS=240         # idle seconds before a provably-working stale pane escalates; stale panes whose actor is not provably working surface immediately unless they declare the pause verb
MX_PAUSE_RESURFACE_SECS=3600       # seconds before an idle declared external wait re-surfaces for a recheck in the watcher or away-mode daemon
MX_WEDGE_DEMAND_INSPECT_COUNT=3    # consecutive provably-working stale escalations on the same unchanged pane before demand-deep-inspection is added
MX_WATCH_TRIAGE_LOG_MAX_BYTES=262144   # size cap for the watcher's absorbed-wake debug log
MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT=     # optional seconds allowed for bootstrap's best-effort clone refresh; unset/blank defaults to max(20, 5 + 3 * origin-backed-project-count)
MX_SYSTEM_PRUNE=1        # set to 0 to skip pruning local branches whose upstream is gone
MX_STALE_WORKTREE_LOCK_AGE_SECS=30       # min mtime age before mx-teardown.sh treats a leftover worktree git index.lock as provably stale
MX_TREEHOUSE_RETURN_LOCK_RETRIES=3        # retries after a treehouse return fails on the transient git index.lock signature
MX_TREEHOUSE_RETURN_LOCK_RETRY_WAIT_SECS=1 # seconds mx-teardown.sh waits before each retry after that signature
MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS=   # legacy alias for MX_TREEHOUSE_RETURN_LOCK_RETRY_WAIT_SECS when the new variable is unset
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
# sub-supervisor (bin/mx-supervise-daemon.sh); presence-gated via /afk
MX_SUPERVISOR_BACKEND=             # optional supervisor pane backend override; tmux/herdr only, otherwise detects $TMUX_PANE then HERDR_ENV/HERDR_PANE_ID before tmux fallback
MX_SUPERVISOR_TARGET=              # optional supervisor pane target override; tmux target or herdr <session>:<pane-id>, otherwise auto-detected
MX_INJECT_SKIP=heartbeat           # |-prefixes force-self-handled bypassing classification; empty disables
MX_ESCALATE_BATCH_SECS=90          # buffer window for batched escalation digests; 0 = flush immediately
MX_MAX_DEFER_SECS=300              # max buffered escalation age before retry plus wedge alarm; 0 disables
MX_WEDGE_ALARM_CHANNEL=            # override config/wedge-alarm with one active-alert directive for the wedge alarm; off|auto|herdr|command:<cmd>; absent = auto (no built-in channel; the durable marker is the only signal)
MX_WEDGE_ALARM_EXEC=              # notifier seam: route every channel (herdr, command:) through this command as `<cmd> <channel> <summary>`; "discard" fires nothing; unset in production; the daemon defaults it to "discard" when sourced so no test posts a real notification
MX_WEDGE_ALARM_TIMEOUT_SECS=10    # maximum seconds for each herdr, override, or command: notifier before its watchdog terminates it and continues to the next channel; invalid or zero values use 10
MX_INJECT_FAIL_SLEEP=30            # seconds to back off when the supervisor pane is unavailable
MX_INJECT_CONFIRM_RETRIES=3        # daemon Enter-retry attempts after typing a digest once
MX_INJECT_CONFIRM_SLEEP=0.5        # seconds between daemon submit checks
MX_HEARTBEAT_SCAN_SECS=300         # cadence of the catch-all status scan for missed maintainer verbs
MX_HOUSEKEEPING_TICK=15            # seconds between batch-flush, stale/pause-recheck, and scan passes
MX_CRASH_THRESHOLD=10              # watcher crashes allowed inside MX_CRASH_WINDOW before daemon backoff
MX_CRASH_WINDOW=60                 # seconds in the crash-loop detection window
MX_CRASH_BACKOFF=60                # seconds to wait after crossing the crash threshold
MX_CRASH_NORMAL_SLEEP=5            # seconds to wait after an isolated watcher crash
MX_LOG_MAX_BYTES=1048576           # daemon log size that triggers trimming
MX_LOG_KEEP_LINES=2000             # daemon log lines kept when trimming
```

`mx-teardown.sh` retries only Git's `Unable to create '...index.lock': File exists` return failure up to `MX_TREEHOUSE_RETURN_LOCK_RETRIES` times.
`MX_TREEHOUSE_RETURN_LOCK_RETRIES` accepts a nonnegative integer, and an unset, blank, or invalid value uses the default of 3.
`MX_TREEHOUSE_RETURN_LOCK_RETRY_WAIT_SECS` accepts nonnegative whole or fractional seconds between attempts.
When it is unset or blank, `MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS` remains a compatible fallback, and a blank fallback uses the 1-second default.
An invalid nonblank wait falls back to 1 second rather than interrupting teardown.
Teardown never removes a lock during the retry window, and after that window it attempts stale-lock cleanup only for a still-present lock that passes the configured age and live-holder checks.

`mx-system-sync.sh` applies the same shape to an orphaned `.git/packed-refs.lock`: it retries only Git's `Unable to create '...packed-refs.lock': File exists` fetch failure up to `MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRIES` times (nonnegative integer; unset, blank, or invalid uses the default of 3), waiting `MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRY_WAIT_SECS` seconds (nonnegative whole or fractional; invalid falls back to 1 second) before each.
Only after those retries exhaust does it remove the lock, and only when it is provably stale - still present, mtime age at least `MX_SYSTEM_SYNC_PACKED_REFS_LOCK_AGE_SECS` (default 30), and no `lsof` holder of the lock file or of the clone worktree itself (a live `git` keeps that as its cwd even in the window after it closes the lock and before it exits).
A live lock, a missing `lsof`, any failed check, or any other fetch failure keeps today's behavior.
Every wait, retry, and removal is printed to stderr, and a successful recovery also prints one `recovered:` summary line to stdout so a session-start refresh - which discards system-sync stderr and relays only stdout - still surfaces it.
The shared staleness proof lives in `bin/mx-lock-lib.sh`, which both `mx-teardown.sh` and `mx-system-sync.sh` use.
