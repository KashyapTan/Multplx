Mode: Cursor stop-hook-owned park.
The named `bin/` commands select the Rust supervision runtime.

- Launch the primary interactively with `agent --trust`; project hooks must load, and `--print` does not provide stop events.
- Claim queued wakes with `bin/mx-wake-drain.sh`, reconcile current task state, record each durable disposition and acknowledge after handling.
- End the handling turn when no immediate work remains.
  The tracked stop hook owns the next watcher cycle with no model command or tokens while parked.
- A real watcher reason returns one marked follow-up; the next turn end parks again automatically.
  Ordinary wakes do not require a foreground checkpoint or manual arm.
- Human input remains available while parked.
  The next stop hook claims the home baton, and an older park retires without emitting after that claim.
  A real event arriving before the newer claim can still be delivered by the sole older park; handling remains idempotent through the durable queue.
- Away mode, lost session ownership, termination, or vanished supervision need retires the park's tracked child.
- Startup failure retries are bounded by `MX_CURSOR_PARK_ATTEMPTS` (default 2, maximum 3).
  Inspect the failure and repair its cause when a `turn-end-guard` follow-up appears; never enter a repeating model-authored checkpoint loop.
- Failure feedback stops after `MX_CURSOR_TURNEND_BLOCK_BUDGET` notices per session (default 3).
  The automatic follow-up ceiling is `MX_CURSOR_TURNEND_LOOP_CEILING` (default and maximum 180), below Cursor's registered `loop_limit: 200`.
  At the ceiling, one explicit notice retains queued wakes and names the next human message as the resumption trigger.
- All delegation, including nested delegation, defaults to managed standing agents; native delegation requires an explicit human request for that scope and is never a managed-spawn fallback.
- A worker done claim still permits parent validation, corrections and revision requests until the full agreed job is verified and delivered.
- After the full accepted job completes, stop automatic contact, polling, nudging and work routing; the agent remains available for user-directed follow-up.
- Available lifecycle events record evidence without granting a native-resume guarantee.
- The stop hook owns supervision until a real event, failure, human input, or ownership change requires attention.
