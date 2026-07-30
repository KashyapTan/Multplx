# Architecture

How broker works, in depth.

The [README](../README.md) carries the high-level diagram and a short synopsis.
This document expands every part of it.
The port's non-auto-loaded broker-contract template and routing index for conditional procedures is [`example_agents.md`](../example_agents.md); this is the human-facing companion.

## Event-driven supervision

A zero-token bash watcher (`bin/mx-watch.sh`) sleeps on the system, classifies detected wakes in bash, and wakes the broker only when something is actionable.
Actionable wakes include maintainer-relevant status signals, no-verb signals whose actor is not provably working, authenticated check output such as PR merge polling, stale panes whose actor is not provably working whether their status log looks terminal or non-terminal, provably-working stale panes that persist past `MX_STALE_ESCALATE_SECS`, declared external waits that remain paused past `MX_PAUSE_RESURFACE_SECS`, and heartbeat backstop hits.
Repeated provably-working stale escalations on the same unchanged pane add an escalation count to the wake reason and, at `MX_WEDGE_DEMAND_INSPECT_COUNT`, a `demand-deep-inspection` marker.
Those actionable wakes are written to a durable local queue (`state/.wake-queue`) before detector state advances, so a missed process exit can be recovered by draining the queue.
When a canonical validated PR poll returns exactly `merged`, the watcher appends that durable notification before publishing a private receipt bound to the poll's registration, bytes, file identities, metadata, provider, URL, and task ID.
The receipt makes retirement safely retryable across restarts: fixed-path recovery revalidates the same evidence, removes the runnable check first, removes its registration and data sidecars, removes the receipt last, and preserves task metadata including `pr=` and `pr_head=`.
A concurrent replacement remains armed, every non-merged or invalid observation remains unchanged, and retirement never performs task or persistent-daemon cleanup.
`bin/mx-pr-lib.sh` owns the receipt format and strict identity mechanics, while `bin/mx-watch.sh` owns queue-before-retirement ordering.
No-verb wakes, such as `working:` notes and bare turn-ended signals, are benign only when `bin/mx-actor-state.sh` reports positive evidence that the actor is still working from native runtime state, an attributed no-mistakes step, or a backend busy signature.
an actor that declares `paused:` for a known external wait is separately absorbed while idle and re-surfaced only on the longer pause cadence, rather than being treated as a possible wedge.
For an ordinary actors that has stopped, the normal-mode watcher first surfaces one stale wake, then applies that same cadence to an unchanged `paused:` or durable `maintainer-held` endpoint only when the backend confidently reports its agent dead.
Live or inconclusive liveness remains fail-open at that initial surface, and the daemon idle-endpoint exemption is unchanged.
Its initial normal-mode status signal still surfaces through the no-verb path, while away mode self-handles that routine signal and owns the later recheck.
Fresh stale panes use the same current-state read before trusting individual observations.
`bin/mx-classify-lib.sh` owns the exact signal-precedence contract; native runtime blockers surface immediately even while an attributed validation run continues, and schema-valid terminal reports outrank regex-only busy text.
No-change heartbeats are also benign.
Absorbed wakes advance their suppression markers, log to `state/.watch-triage.log`, and keep the watcher blocking without a queue record or LLM turn.
After each drain, `mx-wake-drain.sh` runs the same liveness guard as the supervision scripts, so a lapsed watcher chain surfaces even on a turn that only drains and handles queued wakes.
Routine watcher polling, supervision no-ops, elapsed waiting time, and absorbed benign wakes stay silent.
A declared external wait trades that silence for one bounded recheck per pause window, so a forgotten pause cannot remain invisible indefinitely.
Actor status files are append-only wake-event logs, not current-state fields.
Actors write status events through the task-bound `report_status` MCP tool when their harness exposes it or through `bin/mx-report` as the universal fallback.
The wrapper owns the closed actor-writable vocabulary `working|paused|blocked|needs-decision|done|failed|resolved`, rejects multiline events and cross-task writes before opening a status file, and emits the existing plain or keyed line grammar.
Claude and Codex receive session-scoped MCP configuration from `mx-spawn.sh`; Pi uses the wrapper because no project-scoped MCP registration contract is verified for it.
The MCP adapter delegates every accepted call to the wrapper, so validation and append behavior have one owner.
After a successful durable append, `mx-report` may send a payload-free `USR1` nudge to the live watcher only when the PID and PID identity advertised by that home's singleton lock still match.
The signal interrupts the watcher's ordinary terminal poll wait and causes the same scan loop to run early; native Herdr event waits remain bounded and unchanged.
Missing, stale, disabled, or undeliverable nudges are silent, and the durable event plus the normal `MX_POLL` cycle remain authoritative.
This is a latency optimization over the existing reconstructable disk state, not a status-ingest daemon, socket, or second supervision path.
`bin/mx-actor-state.sh <id>` is the cheap current-state read for an actionable heartbeat review: it attributes a no-mistakes run, active or terminal, only when it matches the actor's branch and current code identity, and retains that run-step across a closed pane unless a stronger native runtime verdict is present.
The script header owns the exact run-head ancestry rules.
During no-mistakes' `ci` monitor phase, it also reads the ci step log tail because `axi status` reports both "still waiting on checks" and "checks green, waiting on merge" as `ci,running`.
The most recent recognized ci log marker wins, so checks-green monitoring reports done while a later re-arm, failed-check, or issue marker returns the actors to working.
When no native verdict or matching run exists, a schema-valid status event whose verb maps to a recognized run-state outranks the pane busy-signature; a dead pane without stronger evidence reports unknown instead of trusting a stale log.
Decision-only events such as `resolved` never become current state or leak their prose into the current-state detail.
In that status-log fallback, a declared external wait reports the distinct `paused` state with its reason.
For Herdr, exact `working`, `blocked`, and `done` levels contribute native verdicts, while `idle` is not treated as task-progress evidence because it can occur between tool calls or while a foreground process continues.
Herdr `idle` and unknown levels therefore leave the lower report and rendered busy-signature tiers available.
For whole-system read-only review, `bin/mx-system-snapshot.sh --json` emits schema `mx-system-snapshot.v1` from the backlog, task metadata, current actor state, endpoint probes, PR/report pointers, scout reports, bounded current summaries from registered daemon homes, and daemon return-channel guidance.
`bin/mx-system-view.sh` renders that snapshot as Markdown for humans, while `bin/mx-status-snapshot.sh` provides the bounded catchup projection, so both views consume one structured contract instead of reparsing raw system files.
The script header owns the exact JSON schema.

## Visual review artifacts

vplan provides the broker's maintainer-facing review surface for plans, structured reports, comparisons, and other responses that benefit from visual hierarchy.
`bin/mx-vplan.sh review` starts one loopback-only Node server, injects the vendored comment SDK into the served copy, and records the exact process identity under `state/.vplan/`.
The maintainer queues element or text-anchored comments and confirms once.
The server atomically merges those comments into an inert `#vplan-comments` JSON block in the artifact, removes its matching run record, and exits.
The artifact is the feedback channel, so vplan adds no polling protocol, persistent service, or parallel completion policy.
[`vplan.md`](vplan.md) owns the lifecycle and persistence contracts, while [`vplan-authoring.md`](vplan-authoring.md) owns artifact design.
Unresolved maintainer decisions return to `decision-hold-lifecycle` before the originating review is treated as complete.

### Registered daemon current state

A registered daemon's validated home is the authority for catchup current state because it owns the child metadata inventory, each child's current-state result, endpoint observations, backlog holds and dependencies, keyed unresolved decisions, and recent Done baseline.
The original cross-home projection instead treated the daemon agent as an ordinary parent task, so an idle daemon's `mx-actor-state` fallback selected the latest append-only parent status event even when structured state in the registered home contradicted it.
The parent-status contract also required explicit keyed resolution for decisions and blockers but not for a material `working` phase, so a start event could remain unsuperseded after the corresponding home backlog had moved the work to Done.
Generated daemon charters reject generic receipt or start acknowledgements, key only supervisor-actionable material phase reports, and close an opened phase with a same-key later state or `resolved` event, while the structured home remains authoritative even if that closure is missing.
Cross-home reads validate the seeded identity and operational-directory boundaries, use per-home time and output bounds, and classify unavailable, malformed, or inconsistent structured state as unknown rather than reviving a parent event as current work.
When only an owned child's current classification is unavailable, the home classification stays unknown while independently trustworthy structured decisions, holds, queued and landed records, endpoint identities, counts, and provenance remain available; every other invalid path stays strict and exposes none of those child-derived surfaces.
A bounded direct-report terminal tail can help diagnose a mismatch by showing that historical parent wording is still visible, but it is untrusted supplemental evidence because scrollback, prompts, copied output, idle shells, and agent prose are not durable state.
The snapshot strips control sequences, retains only capture metadata and literal event-corroboration flags, and never lets terminal evidence override a valid structured classification.
The default path remains local-only; live GitHub enrichment exists only behind the catchup `--include-prs` opt-in.

At session start, `bin/mx-session-start.sh` emits exactly one primary-harness supervision block rendered by `bin/mx-supervision-instructions.sh` from `docs/supervision-protocols/`.
That block owns the live wait shape for the running primary harness: Claude's Stop `asyncRewake` hook owns tokenless re-arm cycles, Codex uses bounded foreground checkpoints, and Pi uses its two tracked primary extensions.
`bin/mx-watch-arm.sh` remains the verified arm wrapper for protocols that call it; it forks the watcher as a tracked child, verifies it is genuinely alive with a fresh liveness beacon, and prints an honest `started`, `attached`, or nonzero `FAILED` status.
On `attached` it stays live across identity-matched successors, and an unexplained clean child close either attaches to a verified healthy successor or becomes the typed nonzero `watcher: FAILED - cycle ended without an actionable reason` result.
The arm layer records one bounded lifecycle row per observed cycle in `state/.watch-cycle-exits.log`; `state/.watch-triage.log` remains exclusively the absorbed-wake debug log.
Pi verifies session-lock ownership and launches one singleton successor from its child-close handler before delivering an actionable wake prompt, with bounded exponential retry for failed restoration.
Claude's `bin/mx-claude-stop-autoarm.sh` hook fires on every Stop and, when the home is eligible and still needs supervision, claims one home-scoped cycle, foregrounds the arm wrapper, and translates an actionable close or typed failure into one exit-2 rewake.
[`watcher-continuity.md`](watcher-continuity.md) owns Claude's residual active-turn coverage and watcher-status command-gating boundary.
The existing turn-end guard remains the final backstop for all three harness protocols, cooperating with the auto-arm claim in its `--claude` mode.
Its `--restart` mode signals only the watcher recorded in the current home's `state/.watch.lock`, so restarting one home cannot kill sibling daemon watchers.
A pull-based guard (`bin/mx-guard.sh`) warns through supervision tool output if the primary checkout is tangled, or if tasks are in flight and that watcher stops running or queued wakes are waiting to be drained.
The drain script calls that guard after emptying the queue, which avoids repeating the queued-wakes warning for records it just consumed while still warning on stale watcher liveness.
It leads with a prominent bordered tangle banner, while `bin/mx-guard.sh` owns the stale-watcher banner/reminder policy so repeated guarded commands stay noisy without reprinting the full watcher-down banner in the same episode.
On every verified primary harness, tracked hook integration gives the primary session a push-based backstop: when work is in flight and no identity-matched watcher lock with a fresh beacon is live, direct Stop hooks block and passive turn-end hooks force one bounded follow-up.
The guard covers the main primary and genuinely marked daemon homes, exempts child actor/scout worktrees, is loop-safe per harness, and is documented in [turnend-guard.md](turnend-guard.md).

A presence-gated sub-supervisor (`bin/mx-supervise-daemon.sh`) extends this for walk-away supervision: the `/afk` skill starts it through the tracked foreground helper `bin/mx-afk-start.sh`, after which the watcher reverts to daemon-managed one-shot mode and the daemon self-handles routine wakes in bash.
The watcher and daemon share `bin/mx-classify-lib.sh` for maintainer-relevant status verbs, declared-external-wait vocabulary, and status-scan primitives.
Terminal verbs remain maintainer-relevant, while a nonterminal progress verb cannot become terminal merely because its prose contains a legacy free-text token such as `merged`; bare legacy free-text lines remain compatible.
The always-on watcher also uses that library's absorb classification on no-verb signals and first-sighting stale panes before status-log terminality is trusted, while the daemon maintains distinct wedge and declared-pause recheck cadences.
In away mode, seen-status dedupe does not clear possible-wedge aging for nonterminal progress, so housekeeping still re-escalates an unchanged idle pane at the configured bound.
The daemon escalates maintainer-relevant events, plus a bounded recheck for a declared pause that remains idle, as one batched, single-line digest using the canonical `away-supervisor` kind from `bin/mx-operational-input.sh` so broker can distinguish it structurally from real messages.
Its supervisor injection path supports tmux and herdr panes, with `MX_SUPERVISOR_BACKEND` and `MX_SUPERVISOR_TARGET` resolved independently from the task-spawn backend.
Pane existence, busy checks, composer checks, capture, and verified submit route through `bin/mx-backend.sh`: tmux keeps the same submit core used by the tmux send backend, while herdr uses native busy state, native agent-state submit confirmation on idle baselines, and its ANSI-aware structural composer classifier for pending-input guards and submit fallback.
The tmux submit core (shared `mx_tmux_submit_enter_core`) treats a busy pane + retries-exhausted + composer-still-pending as a queued Enter (some harness TUIs accept Enter mid-turn and queue it for after the turn without clearing the composer), reported as `empty` so the daemon and `mx-send` do not re-send; an idle pane keeps the `pending` verdict as a genuine swallow. The same busy-queue case is a known gap on the herdr adapter and is recorded in `docs/herdr-backend.md` rather than patched here.
Composer-content classification has one shared owner, `bin/mx-composer-lib.sh`, used by tmux, herdr, and cmux after each adapter performs its own capture and composer-row recognition.
The daemon injects only into an affirmatively `empty` composer, so both `pending` and `unknown` defer and a bare dead-shell prompt cannot receive an escalation; the current boundary is in [Composer and injection safety](herdr-backend.md#composer-and-injection-safety).
Unsupported supervisor backends refuse at daemon startup.
Stalled escalation delivery writes `state/.subsuper-inject-wedged` and attempts a configured backend-independent active alert after `MX_MAX_DEFER_SECS` instead of silently deferring forever.
On an unmarked return, `bin/mx-afk-return.sh` owns ordered shutdown, durable catch-up evidence, and the fail-closed gate that keeps ordinary work behind every live broker-actionable blocker.
`mx-send.sh` selects a pre-Enter popup-settle for slash commands and for codex `$...` skill invocations using metadata-routed target `harness=` values, then adds its own `MX_SEND_SETTLE` pause after successful text sends so immediate peeks catch the receiving turn starting; the sub-supervisor uses only the shared submit core and does not pay that post-submit pause.

## Runtime session backends

The runtime backend is the session-provider layer below broker's scripts.
It owns task endpoint creation, bounded capture, text/key sends, current-path reads for spawn-time worktree discovery when the backend does not create the worktree itself, live-window fallback lookup, agent-process liveness probes where verified, and endpoint teardown.
`bin/mx-backend.sh` centralizes backend selection, `state/<id>.meta` helpers, selector resolution, and operation dispatch; `bin/backends/tmux.sh` is the verified reference adapter ([`docs/tmux-backend.md`](tmux-backend.md)), and `bin/backends/herdr.sh` (P2) and `bin/backends/cmux.sh` (P5) are experimental task-spawn adapters.
New spawns select a backend from `--backend`, then `MX_BACKEND`, then local `config/backend`, then runtime auto-detection from `$TMUX`, `HERDR_ENV=1`, or cmux runtime signals, then default `tmux`.
Runtime auto-detection is innermost-first: `$TMUX` wins over `HERDR_ENV=1`, which wins over cmux's primary `CMUX_WORKSPACE_ID` marker and documented fallback signals; auto-detected herdr or cmux prints a one-time opt-out notice, and auto-detected tmux stays silent.
Unknown backend names fail loudly.
For compatibility, default tmux tasks do not write `backend=tmux`; every reader treats a missing `backend=` field as `tmux`.
`mx-watch.sh` polls each window's backend for a busy state: tmux and cmux have no native primitive and always report unknown, preserving the original pane-tail-regex detection unchanged; herdr's `agent.get` semantic state (working/idle/done/blocked) is consulted first for stale detection, with unknown native states falling back to the same regex.
That poll loop is the default event source for backends with no native push events, so this stays an extraction of the abstraction rather than a watcher rewrite.
For capable Herdr sessions, the same watcher replaces its terminal sleep with a bounded native event wait that immediately surfaces `blocked`; [Push events and polling fallback](herdr-backend.md#push-events-and-polling-fallback) owns the current mechanism and capability gates, while [runtime backend verification](verification/runtime-backends.md#native-blocked-event) owns the active evidence.
The deeper session-start agent-process liveness probe is separate from that busy-state poll: tmux and Herdr have verified classifiers for daemon recovery, and cmux does not support daemon spawns.
Herdr is experimental and can be selected explicitly or by runtime auto-detection: Treehouse remains its worktree provider, [`herdr-backend.md`](herdr-backend.md) owns current setup and safety limits, and [`verification/runtime-backends.md`](verification/runtime-backends.md#herdr) owns active empirical evidence.
Herdr's durable default container shape is workspace-per-home plus tab-per-task: the primary home uses workspace label `broker`, daemon homes use `daemon-<daemon-id>`, and recovery/list-live scopes to the current `MX_HOME`'s workspace.
Its optional default-off presentation projection may place one clean new task in a disposable workspace without changing endpoint authority or lifecycle ownership; [Optional presentation spaces](herdr-backend.md#optional-presentation-spaces) owns that conditional design and its narrow home-local restored-shell cleanup at locked session start.
cmux is experimental, GUI-first, macOS-only, and can be selected explicitly or by runtime auto-detection from its primary `CMUX_WORKSPACE_ID` marker plus documented fallback signals: Treehouse remains its worktree provider, [`cmux-backend.md`](cmux-backend.md) owns current setup and limits, and [`verification/runtime-backends.md`](verification/runtime-backends.md#cmux) owns active source and live evidence.
cmux's container shape is one workspace per task with one surface, no per-home container split; workspace titles are scoped by the active home label plus a short hash of the resolved `MX_ROOT` path, and `--daemon` spawns are refused.
Codex App support is recorded in `docs/codex-app-backend.md`; it is not selectable as a runtime backend.

## Worktrees, not branches in your checkout

Actors never intentionally touch your project clone; [treehouse](https://github.com/kunchenguid/treehouse) is the backend-independent worktree provider that pools clean worktrees for tmux, herdr, and cmux task sessions.
The exact external pin and its checksum owner are recorded in [`upstream.md`](upstream.md#pinned-external-dependencies).
For delivery and scout work, `mx-spawn.sh` refuses to launch unless the resolved task path is a real git worktree root that is distinct from the project primary checkout.

The Multplx repo has one extra exposure because it can dispatch actors to work on itself.
Its operating checkout (`MX_ROOT`) and the disposable actor worktrees are all linked git worktrees of the same repository, so the valid discriminator is branch state, not whether the checkout is linked.
The primary checkout is healthy on its default branch, and linked worktrees or daemon homes are healthy at detached HEAD.
Only a named non-default branch checked out in `MX_ROOT` is a worktree tangle.

`mx-tangle-lib.sh` resolves the default branch from `origin/HEAD`, then local `main` or `master`, and classifies that named non-default primary branch as the tangle.
`mx-guard.sh` prints the repair command on the next mutable system action, while `bin/mx-session-start.sh` reports the same condition through bootstrap as a `TANGLE:` line at session start.
If another live session holds the system lock, both surfaces keep the alarm but switch to read-only wording with no repair command.
Delivery briefs also tell the actor to verify `pwd -P` and `git rev-parse --show-toplevel` before creating `mx/<id>`, then stop with a blocked status if it landed in the primary checkout.

## No-mistakes gate authority boundary

Multplx's own no-mistakes gate runs agents inside a checkout that also contains the system-maintainer identity in `example_agents.md`, so gate execution needs an authority boundary separate from ordinary actor worktree isolation.
The tracked `.no-mistakes.yaml` sets `disable_project_settings: true`; no-mistakes honors that setting only from the trusted default-branch copy, so a pushed branch cannot enable its own project instructions during validation.
Independently, `mx-spawn.sh`, `mx-send.sh`, and `mx-teardown.sh` source `bin/mx-gate-refuse-lib.sh` and exit with status 3 before system mutation when the gate environment marker is present or the current checkout matches the default no-mistakes gate-repository topology.
A normal primary checkout or actor worktree has neither signal and remains unaffected.
The helper's header owns the exact signal detection, relocated-home limitation, test-harness bypass, and relationship to no-mistakes' HEAD-continuity guard.

## Two task shapes

DELIVERY TASK change projects and delivery by project mode (`no-mistakes`, `direct-PR`, or `local-only`); scout tasks leave standalone investigation reports at `data/<id>/report.md` and never push.
The intake and authority-contract template in `example_agents.md` owns when separate scout research is warranted.

## Dispatch profiles

Actor and scout dispatch can stay on the static actor harness resolved by `config/actor-harness`, or it can use local dispatch profiles in `config/actor-dispatch.json`.
The dispatch file is intentionally judgment-based: broker reads the natural-language rules at intake, chooses the best matching rule, resolves profile arrays itself from current quota output under `example_agents.md` section 4, and passes only concrete `--harness`, `--model`, and `--effort` axes to `mx-spawn.sh`.
The shell scripts validate the JSON shape and verified harness/effort combinations, but they do not parse task intent, match natural-language rules, or own array selection.
The session-start bootstrap step keeps valid dispatch configuration silent unless verbose facts are enabled and surfaces a concise invalid-config line when validation fails.
When the file exists, `mx-spawn.sh` refuses actor and scout launches without an explicit harness, so `config/actor-harness` is only automatic when no dispatch profile file is active.
Daemon launches are exempt because they resolve the daemon harness and any optional daemon model or effort tokens instead.
Unsupported effort values are still recorded in task meta when passed to `mx-spawn.sh`, but the launch template omits any effort flag that the selected harness does not accept.
That keeps spawn launch compatible across claude, codex, and pi while preserving the requested profile for later audit.

## Optional daemons

`data/daemons.md` records persistent daemons with natural-language scopes, project clone lists, and home paths.
`mx-home-seed.sh` provisions the isolated home, clones the listed PR-based projects into it, initializes newly cloned `no-mistakes` projects, copies the charter to `data/charter.md`, and `mx-spawn.sh --daemon` launches it through the same session-provider and status-file path as any routed agent.
For a domain whose subject is the Multplx repo itself, a deliberate `--no-projects` seed creates a project-less home whose actors take pooled worktrees of that repo instead of separate clones.
The signal cannot be mixed with project names or omitted accidentally, and a populated home cannot be converted in place; the full seed contract is in [configuration.md](configuration.md#daemon-routes-datadaemonsmd).
On the herdr backend, a daemon launch lands in that daemon home's labeled workspace, and actors spawned from that home land in the same workspace.
When seeded with `-`, the home is a durable treehouse lease under the daemon id, so it survives with no live process and is not recycled by later `treehouse get` or pruning.
Retirement or seed rollback returns the leased home; normal restart/recovery keeps it leased.
If returning the lease fails during teardown, broker leaves the route and home intact instead of hiding a still-held lease.
Seeding is transactional: if validation, cloning, initialization, or registry update fails, generated briefs, new homes, new project clones, and registry edits are rolled back.
`local-only` projects stay with the main broker because they merge into the main local checkout instead of a remote-backed PR path.
The same project may appear in multiple daemon homes when their scopes differ, such as issue triage versus feature development.
Daemons are idle by default: after startup recovery reconciles only work already in their own home, an empty queue waits silently for routed tasks, and they never self-initiate surveys or audits.
When called with `MX_HOME=<this-broker-home>` or when `MX_HOME` is already set to the active Multplx home, metadata-routed `mx-send.sh` requests to a live `kind=daemon` use the live-charter-compatible `from-broker` carrier owned by `bin/mx-operational-input.sh`, so the daemon returns terse answers through status lines and detailed answers through docs plus status pointers instead of replying only in its own chat.
The parent guards every marked request against a missing correlated report without reading the daemon conversation; `bin/mx-pending-reply-lib.sh` owns the correlation, recovery, escalation, and retention contract.
Explicit backend-target sends and direct human typing stay unmarked, so maintainer intervention in a daemon pane remains conversational.
After seeding a daemon, `mx-backlog-handoff.sh` validates the system-specific handoff, then atomically routes already-judged in-scope queued item moves through the owned backlog library so the domain queue starts in the right place.
Idle daemon panes are healthy; teardown is explicit and refuses while the daemon home has in-flight work unless the maintainer has approved discard with `--force`.

Daemon homes converge conservatively to the primary's version and declared inherited local material at launch and during locked session start.
The [`daemon-provisioning` skill](../.agents/skills/daemon-provisioning/SKILL.md) owns the full guarded sync, propagation, nudge, and mid-session local-material push contract.

Daemon agents can run on a different verified harness than actors.
`config/daemon-harness` controls the primary's daemon launch harness and may also carry optional model and effort tokens as `<harness> [<model>] [<effort>]` on the first non-empty, non-comment line.
A bare harness line remains harness-only, so existing `config/daemon-harness` files keep their previous behavior.
When the harness token is unset or `default`, launch falls back to `config/actor-harness`, then to the primary's own harness, and the model and effort tokens are ignored.
Those optional tokens are re-read on every daemon spawn or respawn and are overridden by explicit per-spawn `--model` or `--effort` flags.
An explicit per-spawn harness or raw launch command does not inherit model or effort tokens from `config/daemon-harness`.
`config/actor-harness` remains the actor harness and is inherited into daemon homes.
`config/actor-dispatch.json` is inherited too; daemons use the same natural-language dispatch profiles when spawning their own actors.
The [`daemon-provisioning` skill](../.agents/skills/daemon-provisioning/SKILL.md) owns the complete inherited-local-material allowlist and propagation contract.

The `data/daemons.md` line contract is owned by the [`daemon-provisioning` skill](../.agents/skills/daemon-provisioning/SKILL.md#routing-table), and the daemon environment variables are documented in [configuration.md](configuration.md).

## Project modes are explicit

`data/projects.md` records each project's delivery mode and optional `+yolo` autonomy flag.
PR-based modes stop agent work at a clean local commit; the full-validation mode records an approved SHA through its gate, while `direct-PR` omits the full review pipeline but retains the same non-agent remote-delivery boundary.
`local-only` projects stay local until broker performs an approved fast-forward merge.
When a selected delivery path calls for a diff, `bin/mx-review-diff.sh` refreshes the authoritative base and, when task meta records `pr=`, always fetches and compares against `refs/pull/<n>/head` by default (recorded `pr_head=` is only an offline fallback) before falling back to the local branch with a warning.
For target project repos delivered through their own no-mistakes pipeline, commits under `.no-mistakes/evidence/` are the pipeline's PR-viewable validation evidence and are expected to stay in the actors branch until the evidence-hosting design changes.
The Multplx repo itself is the exception: its `.no-mistakes/` directory is local state, stays gitignored, and is rejected by CI if tracked.
Remote delivery is owned by the non-agent `bin/mx-deliver.sh` context described in [delivery.md](delivery.md).
It consumes a private restart-safe handoff, re-verifies its gate and approved SHA, pushes that exact object, opens the PR, and records the URL through `bin/mx-pr-check.sh`.
PR-based task merges run from the same non-agent credential context through `bin/mx-pr-merge.sh`, which records `pr=` and any available `pr_head=` through `bin/mx-pr-check.sh` before calling official `gh pr merge`.
The helper requires a full `https://github.com/<owner>/<repo>/pull/<n>` URL, invokes `gh pr merge <n> --repo <owner>/<repo>`, defaults to `--squash`, preserves explicit merge-method flags, and rejects malformed URLs or repo override flags before recording merge state; any URL on a host other than github.com is refused as a validation error.
Teardown is fail-closed for delivery worktrees: dirty worktrees refuse, committed work must be landed, and any ready-to-push handoff must be delivered or explicitly discarded before the worktree is returned.
[`bin/mx-teardown.sh`](../bin/mx-teardown.sh)'s header owns the landed-work proofs, PR-discovery fallback, and stale-lock recovery procedure.

## Project memory belongs to projects

Durable project-intrinsic agent knowledge lives in each project's committed `AGENTS.md`, with `CLAUDE.md` as a symlink.
Delivery briefs prompt actors to create or update those files through the normal delivery path; `data/projects.md` stays a thin private registry.
Each project `AGENTS.md` carries a short `## Maintaining this file` self-governance section; `bin/mx-ensure-agents-md.sh` owns the canonical wording and injects it idempotently when creating the skeleton, promoting an existing `CLAUDE.md`, or reconciling an existing `AGENTS.md` that still lacks it.
It refuses a case-variant real memory file such as a lowercase `agents.md`, whose `CLAUDE.md` symlink would carry an uppercase literal target that dangles on a case-sensitive filesystem, and surfaces the mismatch for manual reconciliation.
The full ownership rule - what is project-intrinsic versus system-private, and how broker keeps the two apart without writing into project clones - is owned by [`example_agents.md`](../example_agents.md) (project and knowledge management).

## Operational memory routing

`/stow` sweeps the current session for durable knowledge that only exists in conversation and routes each finding to the most specific disk home.
Home-domain maintainer preferences go to `data/maintainer.md`, cross-domain shared maintainer preferences go to the primary home's `data/maintainer-shared.md`, system-local operational facts and gotchas go to home-local `data/learnings.md`, project-intrinsic knowledge goes through normal actor delivery into that project's committed `AGENTS.md`, and task-scoped notes or undone next steps go to the backlog.
Memory writes use inspect-then-update: read the current destination first, then rewrite or prune matching bullets or notes in place instead of appending by default.
Task-scoped notes use `bin/mx-backlog.sh show <id>` followed by `bin/mx-backlog.sh update <id> --body-file <path>`, adding `--archive-body` when the prior body should remain recoverable.
Generalizable broker knowledge goes to shared tracked docs through the normal PR pipeline; the broker-internal `/stow` deliberately never stores findings in either skill directory.

## Local clones stay fresh

The locked session-start bootstrap step, PR-based teardown, and merged-PR wake handling refresh remote-backed project clones when the clone is safe to move.
Wake-time refreshes can target a single clone by project name, so the primary home also catches up when a daemon reports a merge from its own home.
Clean default-branch clones fast-forward to `origin/<default>`, and a clean detached HEAD that holds no unique commits is re-attached to the default branch before the same fast-forward path runs.
Dirty clones, non-default branches, detached HEADs with unique commits, diverged defaults, and default branches checked out in another worktree are reported as `STUCK:` with their behind count and left untouched.
Fetches blocked by an orphaned `.git/packed-refs.lock` use bounded retries and remove the lock only when the shared staleness proof can prove it abandoned; [configuration.md](configuration.md#toolchain) owns the recovery details and tuning knobs.
Local-only projects, clones without an origin remote, and fetch failures remain benign skips.
The refresh also prunes local branches whose remote is gone and that no worktree still needs.

## Self-updates stay safe

`/updatemultplx` fast-forwards the running Multplx repo and registered daemon homes from `origin`, then re-reads updated instructions and nudges updated daemons without touching project clones.
The update is fast-forward only: dirty, diverged, offline, and off-default targets are reported and left untouched.
The origin-based updater and the local daemon sync share the same guarded fast-forward helper; only the origin mode fetches.
The mechanics are owned by the `/updatemultplx` skill and broker's operating-manual template in [`example_agents.md`](../example_agents.md) (self-update).

## Restart-proof

System state lives in each task's session-provider backend (tmux by hard default, herdr or cmux when selected or auto-detected), no-mistakes run records, status event logs, local markdown under `data/` including `data/maintainer.md`, `data/maintainer-shared.md`, and `data/learnings.md`, and persistent daemon homes.
For herdr, respawning after a server-restored layout closes and replaces confirmed no-agent or dead task-tab husks instead of requiring manual tab cleanup.
At session start, confirmed-dead daemon agent endpoints are closed and relaunched through the same daemon spawn path, while ambiguous liveness reads are left untouched to avoid duplicate supervisors.
Use `/stow` before an intentional reset when the conversation may hold durable knowledge that has not yet been written to disk; after that, the next broker session can reconcile and carry on.

## Development notes

The current watcher reliability work combines always-on bash triage with a durable queue for actionable wakes, a race-proof singleton lock, duplicate self-eviction, drain-time liveness assertion, and a self-verifying tracked-child arm wrapper.
The presence-gated sub-supervisor (`bin/mx-supervise-daemon.sh`) provides walk-away supervision via the `/afk` skill while reusing the same shared wake classifier as the always-on watcher.
