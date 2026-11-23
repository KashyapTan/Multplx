# WORK Mac: capture a failed managed launch

This experiment is for an existing working Multplx orchestrator on the WORK Mac where managed workers disappear around 13 seconds after launch, before a verified provider receipt.
A first failed launch is a useful result: it should preserve the transient evidence and stop the workload.
It does not diagnose a security vendor, change host policy, repair the runtime or establish that any model ran.

## Start with one paste

Run this single terminal block from your existing WORK Mac source clone of Multplx while this kit's PR is open.
It fetches only source material, extracts **all** kit files into a new absolute directory, and prints a prompt with that path embedded.
It does not switch the checkout, install a runtime or touch operational sessions.
Copy its printed prompt into your existing working orchestrator, replacing only the worker-choice line with your cheap harness, canonical model ID and supported effort.

```sh
bash <<'SH'
set -euo pipefail
git fetch origin experiment/work-mac-startup-capture
KIT_REF=origin/experiment/work-mac-startup-capture
KIT_DIR=$(mktemp -d "${TMPDIR:-/tmp}/mx-work-mac-kit.XXXXXX")
git archive "$KIT_REF" debug/work-mac-startup | tar -x -C "$KIT_DIR"
git rev-parse "$KIT_REF" > "$KIT_DIR/debug/work-mac-startup/source-commit"
python3 - "$KIT_DIR/debug/work-mac-startup" <<'PY'
from pathlib import Path
import sys
kit = Path(sys.argv[1]).resolve()
print((kit/'prompt.md').read_text().replace('__KIT_DIR__', str(kit)))
PY
SH
```

After this kit is merged, use `git fetch origin main` and `KIT_REF=origin/main` in the block instead.
While the PR is open, `main` does not contain this kit; use its branch above.
The source revision is retained in the extracted directory's `source-commit` file.
Keep the kit until the diagnostic finishes; copy its source revision into the private evidence manifest.
If Python is unavailable, preserve that error and extract/read [prompt.md](prompt.md) with the actual absolute kit path substituted manually; the capture fallback below must be arranged before launch.
Choose the model yourself; a display label is not necessarily a valid model ID, and syntax acceptance is not account/model availability.
The installed runtime's `mx spawn --help` and [dispatch configuration](../../docs/configuration.md#sub-agent-dispatch-profiles-configsubagent-dispatchjson) own the supported choices.
No global install, provider launch, live-session inspection or operational session start is needed to review or test this kit.

## Applicability and limits

This kit works with current `main` at `4d8d41e1e520fd43826e3e8159ac9cae0c1e295b` and with [PR #83](https://github.com/KashyapTan/Multplx/pull/83) at `2deb99249c6f775643596888eb6f74630aac2b81`.
Both have task-bound `tasktmp`, `launch.sh`, the two-line `launch-started` receipt, `task-model inspect`, and exact managed Codex `task-session inspect`.
PR #83 refreshes unavailable captured harness paths and adds startup error wording with the recorded launcher PID/executable.
It is not required for the observer, and this branch changes none of PR #83's files or diagnostics.
Record the actual WORK Mac runtime revision/version; fetching this source does not upgrade it.
An older runtime may lack these fields/commands: capture its help/error, use the scoped fallback below and mark unavailable stages, rather than inventing a compatible receipt.

The relevant layers are:

| Layer | Evidence and limit |
| --- | --- |
| Admission and task | Exact owner/task inspection, accepted brief, attempt/generation/revision; queued admission does not prove a launch. |
| Backend endpoint | Exact tmux/Herdr/cmux target and capture; the endpoint can outlive its provider. |
| Launcher execution | `launch-started` contains launch identity then PID; a shell or `/usr/bin/env` at this PID is not verified Codex startup. |
| Provider process | Exact PID/start time/executable and provider-hook receipt where available; no receipt does not by itself prove the provider never ran. |
| Assignment and work | Actual worker acceptance/report/artifact/evidence; provider startup alone is insufficient. |
| Host explanation | Process exit, sample or a reported signal may narrow the boundary; host administrator logs are needed for policy/vendor attribution. |

`runtime.provider` in task metadata is the terminal backend, while the harness field names Codex/Claude/Cursor/Pi.
Keep those separate in the report.
The raw generated launch script and metadata may contain private paths, prompt text or secrets; they remain private.

## Private capture before spawn

The orchestrator creates a new directory outside all source checkouts with `umask 077`, for example a run-specific directory under the operator's private diagnostics location.
Use a unique run ID, a synthetic local Git repository with an initial commit, and at most three managed tasks matching the failing launch lifecycle/backend/route.
Do not use a private operational home as a fixture or touch existing task sessions.
Match the failing launch lifecycle as well as its backend: standing managed workers are the default.
Use the canonical failing task record read-only if available; do not silently substitute temporary tasks with different home/brief startup.
Standing implementers use filled briefs, home seeding and exact synthetic project/base binding; standing researchers/reviewers use the same existing lifecycle contract.
Bounded task count does not require automatic retirement of those homes.
The current working orchestrator continues to own the real dispatch state; only the new run's exact task paths are observed.
Never copy the entire operational home or provider store.

Resolve `MXBIN` to the absolute installed **mx runtime executable**, not a guessed source build, public launcher or shell shim.
Obtain `OWNER_STATE` from the current canonical task owner/home and preserve that same route for inspection, reporting and question answers.
Use the following pattern for each exact task before its spawn, with real absolute paths substituted by the orchestrator:

```sh
umask 077
python3 /absolute/kit/observe.py \
  --state /absolute/owner/state \
  --task RUN_SCOPED_TASK_ID \
  --request-id RUN_SCOPED_REQUEST_ID \
  --mx /absolute/installed/runtime/target/release/mx \
  --output /absolute/private/run/worker1-before-spawn \
  --seconds 30 \
  > /absolute/private/run/worker1-observer.stdout \
  2> /absolute/private/run/worker1-observer.stderr &
OBSERVER_PID=$!
```

Record `OBSERVER_PID` and verify `events.jsonl` has a `start` event before submitting the managed launch.
The output directory must not already exist; its parent must already exist.
The observer exits naturally, with a 60-second maximum configured duration (command timeouts use the remaining observation budget); it never sends input, writes runtime state, stops processes or consumes wakes.
It is an explicit short diagnostic observer, not an orchestrator supervision loop.
It polls exact task files every 0.2 seconds, saves changed snapshots with SHA-256 hashes and UTC/monotonic timestamps, and attempts bounded process/endpoint/session observations about once a second.
Subprocess timeouts can reduce cadence; `events.jsonl` records actual times and failures, and a sample can add up to two seconds.
Do not claim continuous tracing or that a snapshot caught the last instant before termination.

The helper reads only `<task>.meta`, `<task>.status`, `.spawn-<task>.intent`, `.spawn-<task>.operation`, `.spawn-actions/<request-id>.json`, the exact metadata `tasktmp` launch files, and the transcript pointer validated by this task's installed inspection command.
The native macOS `sample` command is attempted once per observed launcher PID for one second before the suspected loss, with a two-second command timeout.
`ps` captures only that PID's PID/parent/start/elapsed/executable, without environment or argument dumps.
PID reuse is possible: retain receipt identity and process start/executable observations; the sample is a launcher observation, not provider ownership proof.
The process may already be gone, and sampling may be unavailable or denied; those are recorded gaps.
The observer captures at most 400 endpoint lines per observation and caps individual raw files at 8 MiB, worker transcripts at 64 MiB.
A bound refusal is evidence unavailable, never a silent complete backup; preserve the exact oversized file separately if needed.

Pass the same chosen stable request ID to the observer and spawn.
The durable `.spawn-actions/<request-id>.json` receipt preserves stage/binding/backend/endpoint even before task metadata appears.
Save the spawn tool's full stdout/stderr and exit code with UTC start/end in the root chronology.
Do not pipe it through a command that discards its exit status.
A failing spawn can leave canonical metadata, intent and an endpoint: retain all of them and reconcile before any future retry.
Do not clean up an uncertain launch.
After completion/failure and final provider flush, run the helper again into a new directory with `--seconds 1` to save final state/transcript snapshots.
Keep the live observers' own stdout/stderr and event files even when they failed.

If Python is missing, use the exact metadata `tasktmp` path and backend endpoint as soon as the launch publishes them.
Before spawn, arrange a separate bounded local shell observer for these **new task IDs only**, retaining timestamped copies of the two launch files and exact task metadata; capture the backend target using locally verified `mx peek TASK_ID 400` syntax, which resolves the task's recorded backend/endpoint.
Save the exact launch PID's `ps` output and, on macOS, `sample PID 1 10 -file PRIVATE_PATH` while it is alive.
Do not install tools or start the experiment without any pre-loss capture arrangement.
If the runtime cannot expose an endpoint until after a synchronous spawn failure, capture launcher files concurrently and mark endpoint capture unavailable.
Record what could not be arranged rather than retrying or silently expanding scope.

## Transcripts and logs: copy bytes, retain gaps

Preserve three distinct things: the native conversation/tool transcript, endpoint text and runtime/tool evidence.
A successful file copy is a point-in-time snapshot, not a promise that later provider output has flushed.
Keep source path/session identity, copy timestamp, byte count and SHA-256 in the private manifest.
Retain original command output and its nonzero exit code for unavailable logs.

For **every launched managed Codex worker**, inspect using the exact canonical owner:

```sh
MX_STATE_OVERRIDE=/absolute/owner/state /absolute/installed/runtime/target/release/mx task-session inspect RUN_SCOPED_TASK_ID
```

The top-level `transcript_available` must be `true` before copying `.provider_session.transcript_path`.
The observer performs that inspection/copy itself and records the inspection output, including exact attempt and provider UUID when present.
The runtime validates session header UUID and checkout; an available pointer is not a backup until the actual file is copied.
A missing `provider_session` means no exact managed receipt was available, not “transcript captured” or proof of a particular failed executable.
Other harnesses may have no receipt even when working; use only a locally verified export mechanism for the exact session and save its help/version and export result.
If no verified native export exists, mark it unavailable and keep bounded endpoint text, all reports/artifacts, raw launch evidence and the root's complete tool outputs.
Do not claim terminal text covers hidden tool messages or full provider history.

For the **already-running root**, `task-session inspect` does not establish a root transcript.
Identify its exact native session UUID from the current harness/session UI or its own explicit session identity, not a backend endpoint or a recent filename.
For Codex with a known UUID and expected session metadata cwd, `export-codex.py` provides an actual private backup from the explicitly selected Codex home:

```sh
python3 /absolute/kit/export-codex.py \
  --session EXACT_NATIVE_UUID \
  --cwd /absolute/cwd/from/this/session/metadata \
  --store /absolute/selected/CODEX_HOME/sessions \
  --output /absolute/private/run/root-before
```

The helper looks only for filenames containing that exact UUID under the explicitly named session store, requires exactly one owned regular file whose `session_meta` header matches both UUID and cwd, and copies the complete file up to 256 MiB with a hashed manifest.
An archived session requires explicitly selecting `archived_sessions` instead; do not search unrelated homes or choose a newest-by-cwd file.
Wrong or ambiguous identity fails closed; it never selects a replacement conversation.
The UUID/cwd must already be known independently, not inferred from a matching filename.
This is a diagnostic file-export mechanism, not a change to native hook trust or the runtime's managed-receipt contract.
Repeat into `root-after` when all tool work is finished.
An existing root may contain earlier unrelated work: retain that original only privately and scope/redact the shareable copy.

For a different root harness or an unavailable exact UUID/path, inspect that installed harness's help/UI for an export of this exact conversation and use it only if actually supported.
If there is no verified export, save the complete visible conversation with expanded tool output where possible; save separately every experiment tool invocation/result and chronological root log.
For a root hosted in an identified tmux pane, `tmux capture-pane -p -S - -t EXACT_ROOT_PANE > PRIVATE_FILE` saves retained scrollback, not a complete native transcript.
A native application's verified export or macOS copy/paste into a private text file is another manual fallback; label the result and what it omits.
Do not invent a universal `/export` command or claim a screen recording contains all tool history.

The root must emit the `ROOT FINAL EXPORT` requested by the prompt even if all native exports worked.
The final answer uses a logical `PRIVATE_RUN` alias and reviewed content only; full host paths, raw commands and private history stay in private files.
After its final answer, the human saves that exact final message as `root-final-response.txt` under the private run directory and, if supported, performs one final native export after the final message flushes.
That human action is necessary because an in-turn snapshot cannot guarantee inclusion of the model's later final response.
If the human cannot export, retain that file plus `native-root-transcript: unavailable` and the precise gap in the manifest.
Do not withhold the diagnostic report while waiting for an unavailable export.

## Small workload and verdicts

The first worker is an implementer with one deterministic output and a real local assertion, for example a Python script that writes `BLUE\n` only after receiving the durable answer, and an independent check that reads its output and rejects `AMBER\n`.
Require a working report, a real pending question (task-bound reporter `--state needs-decision --key QUESTION_ID`, with the actual message and identity from its brief), the exact revision-bound answer, typed implementation evidence at the actual commit and a fresh done report.
Use [dispatch](../../.agents/skills/task-dispatch/SKILL.md), [supervision](../../.agents/skills/task-supervision/SKILL.md) and [delivery](../../.agents/skills/task-delivery/SKILL.md); this kit does not replace their schemas.
Do not use hand-edited status records to simulate success.
The reviewer receives the first worker's exact current completion evidence/commit, independently checks the artifact and returns a report artifact.
Actual managed wake delivery and durable disposition are part of the experiment; a worker printing “done” is not a received wake.

Stop further launches at the first failed/uncertain launch; do not spend another launch on a retry.
Already-running siblings are retained and their independent authorized work/evidence can finish.
After a verified/accepted first launch, launch a tiny independent researcher while the first is active, then a dependent reviewer after the implementer completes, with no nested delegation.
This gives three launches and a small real concurrency overlap if launch startup succeeds.
If admission or task speed prevents actual overlap, record that fact rather than claiming concurrency coverage.
Use event-driven supervision and a ten-minute job budget; record waiting/capacity/trust blockers and retain uncertain processes/work rather than terminating them at the deadline.
Record the deadline and its wake/recheck trigger before entering an idle wait.
Use an available supported scheduler for the deadline if one is already configured; otherwise tell the human the deadline and to send “finalize this diagnostic” if no wake arrives.
A sleeping model cannot enforce a wall-clock timer by itself.
The short observer's end does not mean a healthy worker must finish within 30 seconds.

Use [report-template.md](report-template.md), retaining private original evidence pointers.
Every stage gets PASS, FAIL or NOT RUN, with unavailable/unknown detail in the observation column.
A stage that was attempted but failed is FAIL; a stage gated by the earlier failure is NOT RUN; only an observed satisfied criterion is PASS.
The diagnostic collection can succeed while the managed workflow fails.

## Review, share and retention

Keep `raw/`, native transcripts, state snapshots, samples, root history and complete tool output outside the source checkout, mode 0700 directories/0600 files.
No raw evidence is committed by this kit.
Create a separate `reviewed/` folder containing the filled report, a minimal chronology and relevant redacted excerpts.
Replace host/user paths consistently, remove credentials/private URLs/prompt history and unrelated root conversation content, but retain relative times, task/attempt mapping, executable basename, version/revision, verdicts and failure boundary.
Record each reviewed excerpt's original private file/hash/range in a private index so redaction does not erase provenance.
A claim such as “host policy killed Codex” requires administrator evidence; absent that, state exactly which PID/executable was last observed and what remained unverified.

Review the explicit candidate file list and diff before sharing.
Result publication requires explicit human approval after review of the concrete file list/content.
Once approved, stage only named reviewed files; inspect the staged diff and file list, then commit/push the reviewed result branch.
Never use `git add .`, copy the raw directory into a repository, or put private recordings in this kit's PR.
Leave synthetic worktrees, dirty work, unpushed commits and uncertain sessions retained for a separate explicit disposition.
Only humans merge PRs.

## Safe verification of this kit

```sh
python3 debug/work-mac-startup/check.py
```

The check uses isolated temporary fake state, an inert fake `mx`, synthetic transcript files and the helper processes only.
It launches no model, modifies no operational state and samples no real worker.
Repository validation and observed CI results belong in this kit's PR; actual WORK Mac outcomes remain NOT RUN until the operator runs the prompt there.
