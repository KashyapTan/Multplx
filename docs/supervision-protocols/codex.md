Mode: Codex foreground checkpoint.
The named `bin/` commands below select the Rust supervision runtime by default.

When this session owns supervision and away mode is not active:
1. Drain first with `bin/mx-wake-drain.sh`.
2. First cycle: run one foreground watcher checkpoint with `bin/mx-watch-checkpoint.sh --seconds "${MX_CODEX_WATCH_CHECKPOINT:-180}"`.
3. Ordinary wake: if the command prints `signal:`, `stale:`, `check:`, or `heartbeat`, drain queued wakes, handle that wake, then start the next checkpoint.
4. If the command prints `checkpoint:` or exits 124 with no wake, drain queued wakes anyway, process any queued user message now visible to Codex, then start the next checkpoint.
5. Never use shell `&` or Codex background tasks for broker watcher supervision.
6. Do not run `bin/mx-watch-arm.sh` as Codex's normal supervision command.
   If it is ever shelled anyway, a backgrounded, piped, or bundled anti-pattern is denied automatically by the PreToolUse seatbelt (`bin/mx-arm-pretool-check.sh`) registered in `.codex/hooks.json`.
7. Failure or missing cycle only: drain queued wakes, inspect the failure, then start a fresh foreground checkpoint.

Codex cannot reason while a foreground tool call is running.
The bounded checkpoint returns control regularly so user messages and queued wakes can be handled without relying on background-task wake semantics.
