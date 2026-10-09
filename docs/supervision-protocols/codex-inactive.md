Mode: Codex bounded foreground fallback; queue bridge inactive.
The named `bin/` commands select the Rust supervision runtime.

This session has no `MX_CODEX_IDLE_CLI=1` activation or no matched native SessionStart readiness receipt, or a retained bridge failure, so event delivery is degraded.
Managed `multplx codex` and managed Codex worker launches set that flag automatically without a user setting.
A direct terminal CLI launch can explicitly opt in with `MX_CODEX_IDLE_CLI=1 codex` when its queue capability is available.
Codex Desktop event delivery is unverified; do not infer a functioning queue bridge from a Codex harness name or queue capability alone.
Complete the provider's native one-time hook trust review when prompted, and ensure hooks are enabled.
The tracked SessionStart `--register` handshake must write `state/.codex-idle-hook-ready.json` matching the live lock owner, exact thread and `CODEX_HOME` before the renderer selects queue delivery.
The provider's current `CODEX_THREAD_ID` must also match exactly; missing or mismatched identity remains on this fallback even if the receipt's process owner is live.
The integration does not override a user's disabled-hook setting or change private trust state.

When this session owns supervision and away mode is not active:
1. Claim queued wakes with `bin/mx-wake-drain.sh`, reconcile current task state, record each durable disposition and acknowledge after handling.
2. Inspect `bin/mx-codex-idle.sh --status` for the exact readiness or capability reason and repair that cause.
3. If immediate recovery needs a wait, use one bounded `bin/mx-watch-checkpoint.sh --seconds "${MX_CODEX_WATCH_CHECKPOINT:-180}"` checkpoint, then handle its actionable wake or human input.
   This is explicit degraded recovery, not routine model polling.
   If native delivery remains unavailable, explain the concrete supervision blocker and retain unfinished wakes; the next human message or repaired native startup is the resumption trigger.
   Do not repeat quiet checkpoints, task peeks, clock sleeps or re-arm commands indefinitely.
4. Do not call `bin/mx-codex-idle.sh --retry` as repair for an inactive bridge or assume that ending generation provides event delivery here.
5. Never use shell `&` or Codex background tool tasks for supervision.

The checkpoint returns control regularly for human input and durable wakes.
This compatibility path does not provide turn-ended event delivery.
