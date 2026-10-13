# Codex completion publication race investigation

The authenticated Codex CLI 0.162.1 smoke run retained at the original disposable lab’s `evidence/live-smoke-results.md` ended its main turn after handling a genuine progress report, but its later completion wake remained unacknowledged for more than 90 seconds.
The exact main rollout is `instance5-main-rollout-01a1355a-d3a7-73d1-969b-9251efdde2ae.jsonl`, and the worker rollout is `instance5-worker-rollout-01a1355d-62c3-7c41-b87e-5d71bea582ad.jsonl` in that evidence directory.
The first watcher input appears as an actual user message at 13:04:46.409Z, proving that the queue transport delivered it.
Empty transport receipts after its acknowledgement are expected pruning, rather than proof that transport never ran.
An observer outside the owner ancestry cannot establish native readiness for that session.

The report owner commits task completion through `recoverable_transition_wait` before persisting the parent outcome and calling `publish_report_wake`.
The idle bridge previously retired whenever the supervision predicate reported no work.
Once the earlier progress wake was acknowledged, a completed task with its completion wake still being published made that predicate false.
No main turn follows completion until a bridge delivers the wake, so relying on another Stop callback cannot repair that gap.
The retained task metadata and queue mtimes are 13:06:46.891335696Z and 13:06:46.989279124Z respectively, consistent with this publication window; filesystem timestamps alone do not prove which particular bridge sample observed it.
A deterministic runtime regression pauses in the no-work interval before appending the next report wake, rather than relying on timing luck.

The fix retains the already activated exact-session bridge while no work exists, stopping its watcher child and waiting for future durable work.
The bridge remains bound to the same process lifetime, thread, and Codex home, and retires on verified ownership loss, away mode, or explicit session end.
It sends no model input for an idle home and does not change completion evidence, task closure, wake disposition, acknowledgement, or uncertain delivery rules.

The requested upstream Firstmate comparison used public commit `6407052f4c521e39401ce2305dfa45d201249827`, without loading the private local `firstmate/` directory.
Its [watcher loop](https://github.com/kunchenguid/firstmate/blob/6407052f4c521e39401ce2305dfa45d201249827/bin/fm-watch.sh#L2681-L2717) remains armed until home teardown or singleton supersession, and [publishes a durable wake before advancing its seen marker](https://github.com/kunchenguid/firstmate/blob/6407052f4c521e39401ce2305dfa45d201249827/bin/fm-watch.sh#L2983-L3005).
Those continuity and durable-publication principles support retaining the small existing exact-session bridge through the publication gap.
Its [Codex protocol](https://github.com/kunchenguid/firstmate/blob/6407052f4c521e39401ce2305dfa45d201249827/docs/supervision-protocols/codex.md#L1-L16) uses foreground checkpoints, so it does not supply a proven Codex background queue transport to transplant.
