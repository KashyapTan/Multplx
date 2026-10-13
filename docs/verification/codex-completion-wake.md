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

The public upstream continuity comparison is recorded in [upstream provenance](../upstream.md#codex-continuity-comparison).

A second native run exposed the initial queued-start gap after using the package's absolute CLI paths.
The accepted root-owned dispatch request retained its attempt, brief, parent route, and provider home, but no task metadata existed until dispatch.
The shared supervision predicate observed only task metadata, durable wakes, and explicit checks, so the initial Stop hook did not start a watcher for that queued request.
The existing watcher is the background owner that invokes admission queue draining.
Supervision now treats retained local dispatch-queue entries as an obligation without parsing, claiming, or executing their records; the admission owner continues to validate identity and capacity.
Unreadable or unsafe queue directories remain visible and are rejected by that owner.
A bounded private watcher health record publishes one durable failure incident until the owner observes repair, rather than repeatedly waking the model for the same malformed queue.
Interrupted observations do not establish repair.
The native runtime regression uses the actual queue owner to accept the request and drain it from an owned background watcher after a capacity marker changes, with a fixture spawn endpoint and transport.
It fails with the prior supervision predicate and passes with queued-work supervision, proving startup before any task metadata and once-only dispatch and wake delivery.

The final authenticated native CLI 0.162.1 run tested implementation commit `86ac1e58d5874b0cdedf3caf878c7fa0b4bbb9a0` through an isolated canonical package install.
Both primary and worker used `gpt-6-luna` with medium reasoning effort and the same isolated `CODEX_HOME`.
The fresh primary retained the previously accepted queued attempt, ended its initial turn, and let its Stop-owned watcher admit exactly one standing worker without manual admission, wake injection, foreground checkpoints, or model polling.
The primary handled and acknowledged the genuine progress wake, ended that turn, then automatically handled and acknowledged the completion wake 31.832 seconds after the background observer saw completion.
Each of the two wake sequences has one durable handled disposition and acknowledgement, and the task has one attempt with no prior attempts.
The worker's original command-completion event records one `time.sleep(120)` lasting 120.003134792 seconds; its report records a longer 134.121445458-second span because the model reconstructed its end timestamp after the original command completed.
The original report and full event stream remain retained rather than being rewritten to hide that distinction.

The disposable lab's `verified/evidence` directory retains both complete JSONL rollouts and readable message/tool timelines, the final native-ready receipt, accepted dispatch request, generated worker launch, canonical task and wake snapshots, original sleep output, and bounded background observer results.
The final primary is `01a135ae-1cc8-7702-9e0d-3fe6ec23c329`, and the worker is `01a135af-a9a6-7922-b640-2274f2a976f2`.
The canonical provider-session receipt identifies the worker UUID; the task runtime's separate `session_id` remains null, so this evidence does not establish native resume support.
The earlier trial that invoked a global runtime through reordered shell `PATH`, and the later queued-start failure, remain separately retained as diagnostic evidence.

The first final-head coverage run stopped in an existing installed-runtime fixture with Linux `ExecutableFileBusy` before producing a coverage percentage.
The fixture copied its executable inside the parallel test process; a sibling fork retaining the writable descriptor is a plausible cause consistent with the [Linux execve error contract](https://man7.org/linux/man-pages/man2/execve.2.html), but that runner's descriptor state was not captured.
The fixture now copies in a separate waited child process and preserves the executable mode, removing the parent writer-inheritance window without retries or time-limit changes.
A controlled inherited-writer probe on the Darwin host did not reproduce Linux's error and is retained with that limitation.
The focused ten-test runtime suite passes; CI still owns the Linux instrumented validation and the unchanged 93% line-coverage gate.
