Mode: Codex Stop-owned exact-thread queue bridge.
The named `bin/` commands select the Rust supervision runtime.

Activation: this protocol applies only when `MX_CODEX_IDLE_CLI=1` is present and a native SessionStart readiness receipt matches this live lock owner, thread and `CODEX_HOME`.
Managed `multplx codex` and managed Codex worker launches set it automatically without a user setting.
Direct terminal CLI launches must explicitly opt in with `MX_CODEX_IDLE_CLI=1 codex`; otherwise use the bounded foreground fallback in `codex-inactive.md`.
Codex Desktop event delivery is unverified and is not activated by this CLI flag automatically.
Complete the provider's native one-time hook trust review when prompted, and keep hooks enabled.
The tracked SessionStart `--register` handshake writes `state/.codex-idle-hook-ready.json`; a flag alone or an unrelated session's receipt does not prove readiness.
The renderer also requires the provider's current `CODEX_THREAD_ID` to match that receipt exactly; a missing identity retains the bounded fallback.
Without that matched receipt, follow `codex-inactive.md` and its bounded foreground waits instead of assuming queue delivery.

When this session owns supervision and away mode is not active:
1. Claim queued wakes with `bin/mx-wake-drain.sh`, reconcile current task state, record each durable disposition and acknowledge only after handling.
2. End the handling turn when no immediate work remains.
   The Stop hook owns a detached singleton watcher bridge, binds the exact hook-supplied thread UUID, `CODEX_HOME` and live session-lock identity, and returns so generation ends.
3. A real watcher event queues one marked input to that bound thread through `codex queue --thread <uuid> --message <input>`.
   The bridge owns subsequent watcher cycles; never start a foreground checkpoint or manual arm after an ordinary wake.
4. Human input remains available while generation is idle.
   A notification accepted while the thread is busy is handled after its current turn.
5. Away mode, lost session-lock identity, or no remaining supervision need stops the bridge and its tracked watcher.
   A queue transport receipt is separate from disposition or acknowledgement, so pending work stays durable until handled.
6. On a failure warning, inspect `state/.codex-idle-failure` and any uncertain submission before recovery.
   `bin/mx-codex-idle.sh --retry` requires the same live lock-owning session and its exact `CODEX_THREAD_ID`; use the recorded thread binding, never a guessed ID.
   An uncertain submission may already have reached Codex, so an explicit retry can duplicate that notification and handling must remain idempotent.
   The Stop hook retains the failure instead of silently resending or blocking repeatedly.
7. `bin/mx-codex-idle.sh --end` stops only this thread's bridge and has the same manual identity requirement.
   `--run` belongs to the runtime owner and is not a model supervision command.

If `codex queue --help` does not expose both `--thread` and `--message`, the Stop hook emits a visible compatibility warning.
Only in that unsupported case, use the explicit foreground fallback `bin/mx-watch-checkpoint.sh --seconds "${MX_CODEX_WATCH_CHECKPOINT:-180}"`.
The bounded checkpoint returns control for human messages and pending wakes; it does not provide turn-ended event delivery.
Never use shell `&` or Codex background tool tasks for supervision.
