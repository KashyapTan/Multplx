# Native delegation and observation

Multplx allows available native delegation tools in orchestrator and sub-agent sessions.

`bin/mx-subagent-pretool-check.sh` remains temporarily as an allowing compatibility entry for older hook installations.

It accepts the old `--tool` and `--claude` grammar, writes no decision output, and requires no escape environment variable.

Tracked Claude settings no longer register that entry.

Cursor pre-tool handling retains the watcher-arm and persistent-directory checks while allowing delegation-shaped tools.

The restrictions that remain serve separate ownership boundaries.

- Watcher commands cannot be hidden behind an unowned background shell.
- Persistent-home directory changes remain scoped to their recorded home.
- Turn-end and AFK hooks retain supervision ownership.
- No agent may merge, enable auto-merge, enter a merge queue or delegate those actions to automation.

## Canonical native observations

`bin/mx-native-observe.sh --provider <provider> --event start|result|interrupted|reconcile` accepts a provider lifecycle hook payload on standard input.

When the provider supplies a child identifier, Multplx records that identifier, the provider, available session and turn identifiers, the current parent attempt and any provider transcript path.

Current adapters declare native observations `session-bound` even when a child identifier is available for correlation.

An observation inside a canonical task is accepted only when `MX_TASK_ID`, `MX_ATTEMPT_ID`, `MX_ATTEMPT_GENERATION` and `MX_BRIEF_REVISION` match the current task owner and attempt.

Stale attempt observations remain private historical evidence and do not change current task metadata.

The observation and canonical task metadata publish through one recoverable transition with task metadata as the authoritative final write.

Repeated delivery of the same provider event converges on its stable observation identity.

Provider result and interruption events describe the native child lifecycle only.

They do not append `done`, change the owning task schedule or infer completion from assistant prose.

Session-start reconciliation runs with a five-second bound before delegation tools become available, then marks an unmatched session-bound start as interrupted and retains its artifact pointer.
Start/result observers remain nonblocking.

An unmatched native start becomes interrupted on parent restart because none of the current provider integrations proves that native child execution remains independently resumable.

The provider child identifier remains attached as correlation evidence.
Its presence never changes the recovery classification.

An unbound main-orchestrator event uses the same observation schema under `state/native-delegations/` with a null task identity.

Those records survive parent restart, while actual continuation still depends on the provider capability named by `recovery`.

The shared system snapshot exposes observation receipts as `native_delegations`.
Task-bound observations also appear inside the canonical task's coordination projection, using the same observation identity.

## Provider surfaces

Tracked Claude and Codex hooks route `SubagentStart` to `start` and `SubagentStop` to `result` using their documented provider lifecycle payloads.

Claude child start/result observation hooks run asynchronously so observation failure cannot block native delegation.
Its bounded session-start reconciliation completes before a new child can start.

The hook is dormant in this development checkout because root `AGENTS.md` is deliberately absent.

Installed runtime and task worktrees resolve the trusted runtime source from `MX_RUST_SOURCE_ROOT` before using the working directory fallback.

Spawned tasks receive that trusted source path explicitly.
Launch construction adds observer-only Claude settings, inline Codex lifecycle hooks, the task-local Cursor plugin hooks and the Pi observer extension without replacing the provider's report transport.
Generated settings and MCP files live under a home-and-attempt-qualified private temp path, so equal task names in separate homes cannot overwrite one another.
Tmux receives a short invocation of the private `tasktmp/launch.sh` file, preserving long environment and hook settings across shell startup.

Tracked Cursor hooks allow `subagentStart` and record the fields supplied by that event.
Cursor exposes no corresponding child-result hook here, so session-start reconciliation closes an unmatched start as interrupted.

The Pi extension observes configured delegation tool calls and results through Pi's `tool_call` and `tool_result` events.
It uses the tool-call identifier only for correlation and records the execution as session-bound.

Pi recognizes `agent`, `subagent`, `spawn_agent` and `delegate_task` by default.
`MX_PI_DELEGATION_TOOLS` can name the exact tool identifiers provided by another installed Pi delegation extension; ordinary tools and MCP calls are not guessed to be children.

Absence of observation never becomes a delegation prohibition.

## Verification

`tests/mx-subagent-pretool-check.test.sh` proves arbitrary current and future-shaped delegation tool names are allowed without `MX_ALLOW_SUBAGENT` and confirms Claude retains its unrelated supervision hooks.

`crates/multplx-cli/tests/supervision_runtime.rs` covers current-attempt acceptance, repeat delivery, stale-event retention, canonical task linkage, result non-completion and session-bound fallback.

`tests/mx-cursor-adapter.test.sh` covers Cursor allow translation and its bounded stop continuation.

`MX_TEST_CASE_GROUP=native-observers tests/daemon-harness-helpers.sh` verifies that every spawned provider receives the trusted runtime source and its generated observer configuration.
`tests/mx-pi-primary-types.test.sh` includes the Pi observer in the strict installed-package typecheck.

Live provider evidence must name the actual installed provider version and distinguish observed hook events from schema documentation or fixture results.

The retired guard's dated A/B history remains in [guard verification](verification/guards.md#primary-session-delegation-guard), where it is explicitly marked superseded.
