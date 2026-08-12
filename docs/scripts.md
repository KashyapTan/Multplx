# The bin/ toolbelt

The broker drives these; interactive entrypoints work by hand too, while `*-lib.sh` files are sourced helpers.
Portion 09 session, bootstrap, supervision, snapshot, view, and timeline entrypoints enter the Rust command boundary by default, with `MX_SESSION_IMPLEMENTATION=legacy` reserved for explicit rollback.
Portion 10 decision, override, binding, exact-command, and workflow entrypoints enter the Rust authority boundary by default, with `MX_AUTHORITY_IMPLEMENTATION=legacy` reserved for explicit pre-mutation rollback.
Portion 11 review, delivery, merge, promotion, and PR-security entrypoints enter the Rust review-delivery boundary by default, with `MX_REVIEW_DELIVERY_IMPLEMENTATION=legacy` reserved for explicit pre-mutation rollback.
Portion 12 viz and vplan entrypoints enter the Rust local-services boundary by default, with `MX_LOCAL_SERVICES_IMPLEMENTATION=legacy` reserved for explicit pre-mutation rollback.
Each row is one purpose clause only: the script's own header comment is the authoritative description of its behavior, flags, and contracts, so read the header before first use.
If you have changed away from the Multplx home in an interactive shell, invoke these scripts by absolute path through the repo's `bin/` directory; the scripts self-locate internally after they start.
The shared deep-review gate refusal for system lifecycle entrypoints is summarized in [architecture.md](architecture.md#deep-review-gate-authority-boundary), while `docs/sessionstart-nudge.md` covers the silent hook-nudge use; `mx-gate-refuse-lib.sh`'s header owns its exact contract.

| Script                   | Purpose                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------ |
| `mx-session-start.sh`    | Compose lock, bootstrap, and wake drain into the single ordered session-start digest |
| `mx-launcher-install.sh` | Atomically install or remove the global bootstrap and register an adopted or managed root/home pair |
| `mx-launcher.sh`         | Validate the configured control plane and activate a shell or delegate one global command |
| `mx-launch-harness.sh`   | Refuse a known competing broker, change only the harness child cwd, and exec its captured real binary |
| `mx-launcher-lib.sh`     | Share literal path decoding, checkout validation, and recursion-safe executable discovery |
| `mx-sessionstart-nudge.sh` | Print the native session-start hook nudge when the primary has not already run the digest |
| `mx-operational-input.sh` | Construct and parse the canonical cross-language operational-input protocol |
| `mx-bootstrap.sh`        | Detect toolchain and system problems, run the locked session-start sweeps, and install approved tools |
| `mx-doctor.sh`           | Sweep system invariants read-only and optionally apply its two proof-bound repairs |
| `mx-probe-lib.sh`        | Share structured tool, Treehouse compatibility, and primary-tangle probes with bootstrap and doctor |
| `mx-system-sync.sh`       | Refresh project clones with safe fast-forwards, self-heals, `STUCK:` reports, branch pruning, and bounded recovery from an orphaned `.git/packed-refs.lock` |
| `mx-system-snapshot.sh`   | Print the read-only structured system snapshot JSON (schema `mx-system-snapshot.v1`)   |
| `mx-system-view.sh`       | Render the system snapshot as a human Markdown view                                   |
| `mx-status-snapshot.sh` | Project the system snapshot to the compact TOON catchup view; local-only unless `--include-prs` |
| `mx-update.sh`           | Fast-forward-only self-update of broker and daemon homes from origin          |
| `mx-backlog.sh`          | Operate the owned markdown backlog through its supported command surface       |
| `mx-backlog-lib.sh`      | Own backlog schema, parsing, atomic mutations, and Done retention              |
| `mx-backlog-handoff.sh`  | Validate and route queued backlog-item moves into a daemon home                  |
| `mx-headroom.sh`         | Report composite dispatch capacity and inspect, cancel, or drain parked requests |
| `mx-viz.sh`              | Start, inspect, and stop the disposable read-only loopback system dashboard |
| `mx-viz-server.mjs`      | Retain the explicit-legacy rollback implementation of the Rust-owned dashboard service until Portion 13 |
| `mx-vplan.sh`            | Create, serve, inspect, and stop one-shot loopback HTML review artifacts |
| `mx-vplan-server.mjs`    | Retain the explicit-legacy rollback implementation of the Rust-owned review service until Portion 13 |
| `mx-timeline.sh`         | Render and filter one task's best-effort event journal as text, JSONL, or self-contained HTML |
| `mx-journal-lib.sh`      | Validate and append closed-vocabulary task journal events without affecting writer success |
| `mx-workflow.sh`         | Validate, launch, inspect, reconcile, abort, and dry-run linear workflow definitions |
| `mx-workflow-lib.sh`     | Preserve the source-compatible workflow executor and rollback surface |
| `mx-upstream-diff.sh`    | Fetch upstream into a private review artifact, classify touched paths, render the report, and advance the validated review cursor |
| `mx-decision-hold.sh`    | Create, verify, complete, and resolve durable maintainer-held decisions                 |
| `mx-brief.sh`            | Scaffold delivery, scout, daemon-charter, and Herdr-lab briefs                       |
| `mx-herdr-lab.sh`        | Provision and guardedly operate an isolated, never-default Herdr lab session         |
| `mx-install-herdr.sh`    | Install CI's exact-version Herdr pin with official asset URL, SHA-256, and protocol checks |
| `mx-install-treehouse.sh`| Install CI's exact-version Treehouse pin for real-Herdr E2E that needs spawn worktrees |
| `mx-herdr-ci-cleanup.sh` | Snapshot and tear down only job-owned `mx-lab-*` sessions in the Herdr CI lane       |
| `mx-test-run.sh`         | Behavior-test runner: selection, resource manifest/scheduler, generated portable lanes, coverage, parity, timing/JSON |
| `mx-test-isolation-proof.sh` | Repeated conflict-matrix and leak proof consuming the runner manifest |
| `mx-ensure-agents-md.sh` | Ensure a project's real `AGENTS.md`, its `CLAUDE.md` symlink, and the canonical self-governance section |
| `mx-guard.sh`            | Warn on primary-checkout tangles, pending queued wakes, and stale watcher liveness   |
| `mx-primary-scope-lib.sh` | Shared marker-or-plain-checkout primary-home predicate for tracked hooks             |
| `mx-session-lock-lib.sh` | Shared session-lock harness identity (ancestry walk and holder liveness) for mx-lock.sh and the Claude Stop auto-arm |
| `mx-cursor-hook.sh` | Translate tracked Cursor session-start, command, delegation, and bounded stop hooks into shared Multplx guards |
| `mx-maintainer-override.sh` | Request, decide, consume, inspect, audit, and hand off exact single-use maintainer exceptions |
| `mx-maintainer-override-lib.sh` | Preserve the source-compatible exception adapter for later port portions |
| `mx-override-bindings.sh` | Print fresh subsystem-owned bindings for workflow, validation, cleanup, isolation, and lock exceptions |
| `mx-override-run.sh` | Bind and run exact direct-write, one-action elevation, and verified dependency-install exceptions |
| `mx-validation-waive.sh` | Enter the Rust review-delivery boundary to create an exact-SHA waived handoff without marking validation passed |
| `mx-claude-stop-autoarm.sh` | Claude Stop `asyncRewake` hook owning tokenless watcher continuity with single-flight exit-2 rewake (docs/watcher-continuity.md) |
| `mx-turnend-guard.sh`    | Shared primary turn-end guard predicate so no turn ends blind (docs/turnend-guard.md) |
| `mx-arm-pretool-check.sh` | Stable PreToolUse transport for the watcher-arm command policy (docs/arm-pretool-check.md) |
| `mx-arm-command-policy.mjs` | Explicit-legacy rollback implementation of the Rust-owned watcher-arm policy (docs/arm-pretool-check.md)   |
| `mx-subagent-pretool-check.sh` | Primary-home delegation-shape PreToolUse guard (docs/subagent-guard.md) |
| `mx-supervision-instructions.sh` | Render the session-start primary-harness supervision block or the one-line repair instruction |
| `mx-home-seed.sh`        | Transactionally provision a daemon home and maintain `data/daemons.md`       |
| `mx-spawn.sh`            | Spawn actors, scouts, `id=repo` batches, and daemons on the resolved harness and runtime backend |
| `mx-backend.sh`          | Runtime-backend selection, meta helpers, selector resolution, and operation dispatch |
| `mx-backend-hometag-lib.sh` | Shared per-installation home-tag derivation for cmux workspace titles |
| `mx-composer-lib.sh`     | Single system-wide owner of composer-content classification for all backends          |
| `backends/tmux.sh`       | Verified tmux session-provider adapter                                               |
| `backends/herdr.sh`      | Experimental herdr session-provider adapter                                          |
| `backends/cmux.sh`       | Experimental cmux session-provider adapter                                           |
| `mx-config-push.sh`      | Push declared inherited local material to live daemons mid-session and send a pointer to the literal-content config reread when config changed |
| `mx-deliver.sh`          | Enter the Rust review-delivery boundary to push one exact approved SHA and open its PR from a credentialed non-agent context |
| `mx-deliver-lib.sh`      | Preserve the source-compatible delivery-record and eligibility adapter during the rollback window |
| `mx-deep-review.sh`      | Enter the Rust review-delivery boundary to run, resume, or answer the actor-owned validation gate |
| `mx-deep-review-lib.sh`  | Preserve source-compatible schemas, trusted config parsing, prompt assembly, and harness adapters |
| `mx-project-mode.sh`     | Resolve a project's delivery mode and `+yolo` flag from `data/projects.md`           |
| `mx-merge-local.sh`      | Enter the Rust review-delivery boundary for an approved local-only fast-forward merge |
| `mx-review-diff.sh`      | Enter the Rust review-delivery boundary for a bounded authoritative-base diff review |
| `mx-marker-lib.sh`       | Compatibility entry point for the from-broker carrier owned by `mx-operational-input.sh` |
| `mx-pending-reply-lib.sh` | Parent-owned daemon pending-reply expectations, recovery, and one-shot escalation |
| `mx-daemon-report.sh` | Optional helper to append a correlated parent status or document-pointer report       |
| `mx-report`           | Validate and durably append a task-bound status event, then best-effort nudge the identity-matched watcher |
| `mx-report-mcp`       | Rust stdio MCP server exposing `report_status` through the shared task-bound reporter    |
| `mx-report-mcp.mjs`   | Explicit-legacy rollback implementation of the report MCP transport                      |
| `mx-gate-refuse-lib.sh`  | Refuse lifecycle entrypoints whenever a deep-review agent marker is present             |
| `mx-watch-arm.sh`        | Verified home-scoped watcher arm wrapper with loud cycle endings and bounded lifecycle ledger |
| `mx-watch-checkpoint.sh` | Run one bounded foreground watcher checkpoint for Codex-style supervision            |
| `mx-watch.sh`            | Singleton-safe watcher with interruptible polling: absorb benign wakes, queue and exit on actionable ones |
| `mx-afk-start.sh`        | Run the common sourceable away-mode daemon entry in the foreground                      |
| `mx-afk-launch.sh`       | Own away-mode entry, exit, rollback, and any backend terminal lifecycle                 |
| `mx-afk-return.sh`       | Own deterministic return shutdown, catch-up evidence, and the broker-actionable blocker gate |
| `mx-supervisor-target-lib.sh` | Resolve the shared supervisor target and backend for the daemon and launcher       |
| `mx-supervise-daemon.sh` | Presence-gated away-mode sub-supervisor: self-handle routine wakes, escalate batched digests, alert on failed delivery |
| `mx-actor-state.sh`       | Print one deterministic current-state line for an actor                                |
| `mx-tangle-lib.sh`       | Shared default-branch resolution and primary-checkout tangle classification          |
| `mx-supervision-lib.sh`  | Shared in-flight-work-without-fresh-watcher-beacon predicate                         |
| `mx-ff-lib.sh`           | Shared guarded fast-forward helper for origin pulls and local daemon syncs       |
| `mx-lock-lib.sh`         | Shared "is this git lock provably abandoned?" proof used by teardown and system-sync   |
| `mx-config-inherit-lib.sh` | Shared primary-to-daemon inherited local-material propagation and config-reread delivery |
| `mx-wake-drain.sh`       | Atomically drain queued watcher wakes, emit bounded best-effort status-event annotations, then assert watcher liveness |
| `mx-wake-lib.sh`         | Shared durable wake queue, portable locks, and watcher identity/health helpers       |
| `mx-classify-lib.sh`     | Shared maintainer-relevant and declared-external-wait wake classification vocabulary    |
| `mx-send.sh`             | Send one verified literal line or supported key through the target's recorded backend |
| `mx-tmux-lib.sh`         | Shared tmux pane primitives for busy detection, composer capture, and verified submit |
| `mx-peek.sh`             | Print a bounded tail of an actor endpoint                                          |
| `mx-check-register.sh`   | Bind an intentional custom watcher check to its current bytes through Rust          |
| `mx-check-lib.sh`        | Validate custom-check registrations and prepare private execution snapshots          |
| `mx-pr-lib.sh`           | Preserve the sourced PR-artifact ABI while Rust owns typed identities and closed records |
| `mx-pr-poll.sh`          | Execute the byte-static validated GitHub merge poll through Rust                    |
| `mx-pr-check-migrate.sh` | Enter the Rust review-delivery boundary for non-executing quarantine and canonical rebuild |
| `mx-pr-check.sh`         | Enter the Rust boundary to record canonical PR metadata and arm a static merge poll |
| `mx-pr-merge.sh`         | Enter the non-agent Rust boundary to record PR metadata and merge a canonical GitHub URL |
| `mx-promote.sh`          | Atomically promote a scout task in place to a protected delivery task through Rust  |
| `mx-teardown.sh`         | Fail-closed teardown: return landed delivery worktrees, require completed scout deliverables, retire daemon homes |
| `mx-harness.sh`          | Detect the running harness and resolve the actor or daemon harness, model, and effort |
| `mx-lock.sh`             | Per-home broker session lock                                                      |
