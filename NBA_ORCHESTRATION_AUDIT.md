# NBA main-conversation orchestration audit

This report audits the complete 19,816-line supplied main conversation against repository revision `7cef82e`, before any audit fixes.
The source is the user-supplied `Pasted text.txt` attachment; it remains outside Git and is not copied into this report.
All transcript references below are one-based source line numbers, not repository line numbers.
Private operational paths, process identifiers, session UUIDs and lease tokens are deliberately omitted.
Task names and shortened product commits are retained to connect findings with the separate worker audit.
No operational startup, private-home mutation, live worker input, product modification, commit or publication was performed for this audit.

## Coverage and confidence

The source contains 2,179,225 characters, 633 activity blocks, 78 assistant turns and 11 user turns.
The audit indexed every source line and activity boundary, inspected every conversational turn and every failure/diagnostic occurrence, and followed the relevant command/output sequences across the entire file.
Repeated embedded task JSON, copied skill/documentation text and terminal redisplays were collapsed for analysis rather than mistaken for distinct incidents.
The file includes 61 activity blocks calling peek, 150 calling wake commands, 68 calling task inspection, 63 calling help and 25 calling forge polling or watching commands.
These categories overlap and are activity-block counts, not independent command counts or token measurements.
There are 43 repeated stale-watcher warnings.
Elapsed time, billable token use, hidden reasoning and complete provider execution cannot be reconstructed reliably from this exported conversation.
The separate worker-transcript audit owns full worker JSONL analysis; its returned evidence is incorporated below and preserved outside Git in `/private/tmp/mx-nba-worker-audit.md`, including original JSONL pointers.
The separate hooks research owns trust prompts, hook ordering and native queue capability.
No fresh live backend or Codex experiment was run here, so current-code diagnoses identify reproducible paths and proposed fixture requirements without claiming empirical runtime verification.
The checked repository already includes PR #58, including packaged runtime discovery and environment-routing fixes; historical environment failures must not be relabeled as unfixed defects without checking those changes.

| Main transcript span | Covered subject |
| --- | --- |
| 1-1238 | Intake, full startup digest, dummy task inspection, trust transport, shell-only endpoint and discovery mistakes. |
| 1239-3553 | User drops dummy, persistent replacement failure, reservation/intent recovery loop and explicit fresh-assignment decision. |
| 3554-4647 | V2 scaffolding, placeholder refusal, tmux launch, repository/hook review and proposed research split. |
| 4648-8224 | Human Herdr correction, failed replacement, retained partial files, repeated recovery/document reads and clean V3/research dispatch. |
| 8225-9419 | Readiness verification, research evidence correction, user stack/test/CI steering, accepted revision and old-task hold. |
| 9420-11336 | Bun PATH investigation, React/test work, nested test launch, detached allocation misdiagnosis, model error and root sibling dispatch. |
| 11337-13211 | Product/test ownership handoffs, malformed-season fix, combined validation, completion evidence/report friction and initial dashboard delivery. |
| 13212-14703 | Launcher request, project binding, scaffold refusal, model-label launch, provider failure and premature account-access decision. |
| 14704-16810 | User retry, session-history size failure, model-ID diagnosis, replacement failure, clean retry, launcher evidence and delivery. |
| 16811-18021 | PR publication/merge order, repeated CI checks, missing Vite arguments, compatibility assignment and follow-up delivery. |
| 18022-19816 | Runtime-cwd/tool discovery, parent integration, final exact-commit checks/evidence, PR refresh, hosted CI and unmerged final outcome. |

## Task and outcome map

| Identity | Observed role and disposition in this transcript |
| --- | --- |
| `NBA_WEB_REFACTOR` | Preexisting dummy persistent implementer, later replaced unsuccessfully and retained. |
| `NBA_WEB_REFACTOR_V2` | Fresh tmux implementer, then failed Herdr replacement with partial work retained and backlog held. |
| `NBA_WEB_REFACTOR_V3` | Successful Herdr product implementer, accepted scope revision 2, final original dashboard commit `aa1cb5c`. |
| `NBA_WEB_API_RESEARCH` | Successful Herdr report worker, corrected revision typo before findings were passed onward. |
| `NBA_WEB_TESTS_CI` | V3-owned nested test worker, failed before useful execution with a model error. |
| `NBA_WEB_TESTS_CI_ROOT` | Successful root-owned test/CI sibling, integrated into V3. |
| `NBA_DEV_START_SCRIPT` | Launcher worker launched with a model display label, failed and retained. |
| `NBA_DEV_START_SCRIPT_RETRY` | Successful launcher worker using canonical model ID, original commit `e376db8`, later integration commit `72f62b2`. |
| `NBA_DEV_SCRIPT_E2E_COMPAT` | Successful launcher argument-forwarding follow-up, source commit `bff84c9`, integrated into the launcher PR. |

The transcript ends with both product PRs targeting `main`, both checks passing and both unmerged at lines 19789-19816.
That is a successful eventual product delivery, with substantial avoidable orchestration recovery beforehand.

## Findings

### 1. P1 - Persistent implementation replacement calls an allocation API that rejects persistent allocations

Classification: observed current repository defect, confirmed by matching current caller and callee.
Transcript evidence: lines 1448-1454, 4745-4750 and 15260-15265 all fail with `handoff requires an active or retained ordinary allocation`.
The V2 replacement also removed its former tmux endpoint while dirty files remained, as shown at lines 4755-5016.
Current cause: `crates/multplx-cli/src/lib.rs:4374` selects a retained previous implementation allocation and calls `Store::rebind` during replacement.
`crates/multplx-domain/src/lifecycle/worktree.rs:969` rejects `token.persistent` even when the allocation is active or retained.
Persistent workers are the contract default, so this incompatibility affects the ordinary replacement path rather than an obscure combination.
Small robust fix: support an explicitly validated same-task persistent implementation handoff, or select a separate allocation-owner operation designed for that case.
Keep exact owner home, project identity, allocation token, prior attempt, new attempt, generation, worktree identity and occupant checks.
Keep dirty progress in the same owned worktree without deleting, resetting or silently adopting borrowed work.
Preflight deterministic allocation eligibility before stopping the old endpoint; revalidate occupants after that endpoint is isolated.
Required verification: persistent and ordinary clean/dirty replacements, exact replay, stale token, foreign home, active unrelated occupant, generation drift and interruption boundaries.
Do not weaken release/prune authority merely to permit same-task replacement.

### 2. P1 - Replacement intent recovery can strand the task before any replacement agent starts

Classification: observed failure with current matching implementation; distinct recovery issue coupled to finding 1.
Transcript evidence: lines 1770-1775, 3093-3106, 3312-3390, 5168-5179, 7901-7912 and 16096-16105.
Retries using the original invocation, explicit backend, reserved request identity and changed request identity repeatedly fail with binding conflicts.
The result is new task identities instead of recovery of the original accepted task.
Current cause: `crates/multplx-cli/src/lib.rs:4080` compares the serialized retry binding with the durable intent before replacement replay can reconcile the reservation's later attempt/allocation state.
The same path mutates the intent to the replacement binding at lines 4180-4205 while canonical metadata may still describe the earlier attempt.
The exact originating mismatch for every exported incident remains inferred because the transcript does not retain all complete pre/post state snapshots.
Small robust fix: use the recorded reservation and launch action as the authoritative retry input and compare immutable operation identity separately from fields advanced by the owner transaction.
Provide an owner-mediated inspect/recover path with the exact supported retry command and a specific reason when automatic reconciliation is unsafe.
Retain submitted/uncertain executions; a timeout or missing capture must never be treated as proof of absence.
Required verification: fault injection before/after intent, endpoint isolation, attempt replacement, allocation rebind, action submission and canonical metadata commit; every repeat must converge or expose one precise retained blocker.

### 3. P2 - Brief scaffold inserts a template token into explanatory prose that the home seeder rejects

Classification: observed current repository defect.
Transcript evidence: lines 3896-3927, 10024-10027, 11159-11223 and 13862-13895.
Agents filled Task and Charter, but the generated Herdr safety explanation still contained literal `{TASK}` and blocked provisioning.
Current cause: `crates/multplx-domain/src/lifecycle/brief.rs:124` emits the literal token in its safety prose.
`crates/multplx-domain/src/lifecycle/home_seed.rs:778` rejects any remaining occurrence in the entire document.
Small robust fix: remove the literal token from the explanatory safety prose while retaining actual Task/Charter placeholders and the unfilled-brief guard.
Required verification: a standard persistent brief with Task/Charter filled seeds successfully without incidental prose editing; an actual unfilled assignment still refuses.

### 4. P2 - Provider-session receipts can be written larger than their supported read limit

Classification: observed current repository defect, with size source inferred from receipt structure.
Transcript evidence: lines 14725-14730 show `task-session history` failing with `file record exceeds 16384 bytes` during model recovery.
Current cause: `crates/multplx-cli/src/task_session.rs:184` stores the full provider process identity, including its command marker, and the write at line 216 has no corresponding serialized-size check.
Receipt rereads at lines 192, 317 and 351 use a fixed 16 KiB limit.
A large legitimate provider command can therefore create a receipt that its own owner cannot read.
Small robust fix: choose one explicit bounded receipt contract for writer, inspect, history and replay, and preserve the full process identity required for ownership verification.
Do not truncate command identity arbitrarily or replace it with a weak PID-only match.
History should report an invalid individual receipt with its retained path and reason while preserving available valid historical entries, if its public contract supports partial results.
Required verification: a legitimate large command marker, writer/read boundary, over-limit refusal, malformed receipt, foreign ownership and mixed valid/invalid history.

### 5. P2 - Empty persistent worker homes are projected as invalid portfolios

Classification: observed current projection defect; exact initialization choice is an implementation decision.
Transcript evidence: lines 541-561 and 4021-4029 show active task identities as unknown solely because their persistent homes lack a structured child backlog.
Current cause: `crates/multplx-cli/src/system_snapshot.rs:924` treats absent backlog as invalid, and lines 2492-2508 discard structured observations for that invalidity.
The home seeder creates the private home but does not establish a canonical empty child backlog.
A worker with no delegated children can legitimately have no work portfolio of its own while executing the parent-owned assignment.
Small robust fix: make provisioning initialize an owned empty backlog, or distinguish a legitimate empty worker portfolio from corrupt/missing coordinator state in the read-only projection.
Display the worker's exact parent-owned assignment and endpoint observation separately from its child portfolio.
Do not equate a parent progress report with fresh child activity or manufacture a running state.
Required verification: empty persistent implementer and researcher, worker with children, corrupt backlog, absent home, unavailable endpoint and historical parent fallback.

### 6. P2 - Model display labels reach the provider unchanged and create misleading account blockers

Classification: observed agent input mistake with a current validation/ergonomics gap.
Transcript evidence: line 13900 passes `--model 'GPT-6 Luna'`; lines 14334-14347 show HTTP 400; lines 14646-14711 ask the human to change models or wait; lines 15192-15269 identify the label/ID mismatch; lines 16150-16281 succeed with `gpt-6-luna`.
Worker evidence confirms the same root cause for nested `NBA_WEB_TESTS_CI`: V3 JSONL launch events 945, 989, 1010, 1017 and 1024 use the display label; the nested provider rejects it at event 9.
The original launcher provider repeats its rejection at events 9 and 15 because sending a retry instruction does not change its original process model argument.
Current code: `crates/multplx-domain/src/lifecycle/spawn.rs:595` accepts the model string directly, while the harness skill already instructs using supported discovery values.
Small robust fix: reject obvious display-label input before reserving resources, or normalize a narrow verified alias map while retaining the user's exact model intent.
Do not hardcode a closed allowlist that rejects newly available provider IDs.
After provider failure, surface the exact requested ID next to the error and compare with known successful launches before claiming account incompatibility.
Required verification: canonical ID preservation, a supported display alias if normalization is selected, unknown model handling and no silent model downgrade.

### 7. P2 - Replacement backend choice is re-resolved from ambient environment rather than the existing task

Classification: observed routing surprise; current code confirms the default behavior, but product policy needs an explicit choice.
Transcript evidence: original task metadata is tmux, the replacement at lines 1448-1458 auto-selects Herdr, then the assistant chooses tmux for V2 and the human corrects it at lines 4648-4655.
Current cause: `crates/multplx-cli/src/lib.rs:2720` resolves new spawn backend from environment/configuration, including `HERDR_ENV`, without making an existing task's backend the replacement default.
The first failure was finding 1, not evidence that Herdr itself could not run the task.
Small robust fix: preserve the recorded backend on same-task replacement unless an explicit requested override selects another backend; retain normal discovery for genuinely new tasks.
Document and test precedence between explicit CLI choice, task binding, saved configuration and terminal detection.
Agent correction: do not add backend requirements to accepted prose merely as a local workaround unless they reflect the user's intent.

### 8. P2 - Backend requirements were revised after starting the incompatible replacement

Classification: observed agent sequencing mistake, amplified by findings 1 and 2.
Transcript evidence: the assistant attempts Herdr replacement at lines 4745-4750, but only identifies and revises the brief's tmux constraint at lines 7778-7900.
The preceding claim that the frozen prose caused the allocation error is not supported by the exact error or current `Store::rebind` implementation.
Small robust correction: record the user's changed transport requirement and inspect the accepted revision before dispatch, and distinguish a prose constraint from the concrete allocation-owner failure.
This needs an instruction/example improvement and matching launch diagnostics, not a parser that infers backend authority from arbitrary brief prose.

### 9. P2 - Repeated foreground polling dominates supervision despite an emitted idle protocol

Classification: observed agent supervision mistake; actual automatic queue readiness remains unproven in this export.
Transcript evidence: session start at lines 38-90 describes Stop-owned queue behavior and fallback conditions; lines 9545-10940, 11337-11863 and 17519-17821 contain repeated immediate wake/peek/task/Git polls.
The transcript's 61 peek blocks and 150 wake blocks show observation overhead, but do not prove that every check was unnecessary.
Small robust correction: check the exact session readiness receipt once, select the supported queue or bounded foreground path, and wait between meaningful state changes.
After a task is done and the full job delivered, stop supervising it unless the user requests follow-up.
Any runtime changes should expose readiness and the supported next observation clearly; hooks research owns the actual provider integration.
Do not claim automatic Desktop queue delivery from a CLI flag or a copied operating block.

### 10. P3 - Stale watcher warnings repeat throughout an unchanged episode

Classification: observed current usability defect, separate from proving watcher delivery is broken.
Transcript evidence: 43 occurrences of `watcher still down (same stale episode...)`, including lines 4057, 4244-4268, 8481-8487 and 9257.
Current cause: `crates/multplx-cli/src/supervision.rs:4494` emits another warning for every guarded call after its full-banner marker has already been set.
Small robust fix: rate-limit repeat notices per owned stale episode while preserving the initial actionable warning, state changes and a queryable diagnostic.
Do not hide a fresh lapse, suppress the initial warning or silently claim repair.
Required verification: first lapse, unchanged repeated operation, renewed lapse, restored beacon, read-only owner and queued wake changes.

### 11. P2 - Task state and pane liveness are presented as execution acceptance too early

Classification: observed communication problem with an existing explicit runtime distinction.
Transcript evidence: lines 159-162 call the dummy task running; lines 3928-4053 show a successful launch still waiting at trust; lines 14049-14351 say the launcher is accepted/running before discovering a two-second provider rejection.
The runtime task schedule and backend endpoint confirm orchestration placement, not that the model accepted the brief and started useful work.
The task-session help explicitly says its receipt does not prove assignment acceptance.
Small robust correction: report reservation, endpoint launch, provider readiness and first task-bound working acceptance as distinct facts.
A compact launch diagnostic should combine these observations without collapsing unknown readiness into running.
Required verification: trust prompt, provider model rejection, shell-only pane, launch submitted but response lost and first accepted report.

### 12. P3 - Ordinary inspection repeatedly prints full accepted briefs and implementation history

Classification: observed agent over-reading with a current CLI efficiency opportunity.
Transcript evidence: lines 288-389, 1459-1560, 7659-7777, 13220-13614 and 18151-18814 repeat large task structures and embedded brief text.
One exported source line alone is 53,994 characters long.
Current code serializes the entire inspected task in `crates/multplx-cli/src/lib.rs:5900`; existing snapshots are intended to provide bounded portfolio observations.
Small robust fix: offer a compact task inspection projection with identity, revision, endpoint, allocation, current state, pending obligations and artifact pointers; retain explicit full output for forensic use.
Agent correction: use targeted projections after the initial digest and avoid repeated full skill/backend-document dumps when the needed contract is already loaded.
Do not estimate token savings without measurement.

### 13. P3 - Command discovery has inconsistent help and entry-point behavior

Classification: observed current CLI ergonomics defect plus agent command guessing.
Transcript evidence: `mx doctor --help` fails at lines 1318, 1584 and 7169; `mx backlog add --help` fails at 13910-13912; `mx backlog hold --help` fails at 14573-14576; bare `mx-deliver.sh` is missing at 13119-13121 and 18056-18084.
Current code: `crates/multplx-domain/src/backlog.rs:1207` recognizes help only as the first argument, then its option parser rejects nested `--help`.
The supported doctor entry point is already documented as `multplx doctor` or `bin/mx-doctor.sh`, so lack of `mx doctor` alone is not a correctness bug.
Small robust fix: make supported subcommand help requests return useful usage with success, and keep alias/compatibility entry points discoverable from root help.
Keep generated brief commands absolute or use the documented multicall owner rather than assuming every compatibility script is on PATH.
Required verification: root and nested help, missing operands and unknown commands; help must not mutate state.

### 14. P2 - Root recovery impersonates worker reporting context to record task outcomes

Classification: observed supported-authority ambiguity and communication friction; do not bypass binding validation.
Transcript evidence: explicit `--id` and attempt flags fail at lines 14629-14634 because cwd is not the worker checkout; manually supplied `MX_TASK_ID` and parent-state environment succeeds at 14635-14640.
The same pattern is used for the final integrated done report at lines 19282-19287.
Current cause: `crates/multplx-domain/src/supervision.rs:173` derives the calling task from environment/cwd separately from parsed `--id`.
That check correctly prevents arbitrary task-ID selection from granting worker reporting authority, but the parent lacks a clearly distinguished recovery/outcome path in the observed workflow.
Small robust fix: document or add a parent-authorized task outcome operation with explicit actor provenance, current revision and retained worker evidence.
Do not make `--id` alone authorize a foreign worker report, and do not prescribe fabricated worker identity as routine parent bookkeeping.
Required verification: genuine worker, unbound root caller, exact parent route, foreign parent, stale revision and explicit integration evidence.

### 15. P3 - Correlation and completion evidence checks work, but repeated retries reveal poor discoverability

Classification: observed valid safety refusals, with an additional confirmed request-answer defect separated in finding 30.
Transcript evidence: missing explicit reply correlation rejects reporting at lines 15996-16015; stale expected delivery commit rejects evidence at lines 12338-12346; repeated publication `--checks` rejects at lines 16688-16698.
The exact-correlation and compare-and-swap refusals are valid rather than defects to remove; finding 30 explains why informational requests unnecessarily remain outstanding.
Small robust improvement: show current pending request identities and expected evidence revision in one compact inspect result, and provide a structured checks-file option if repeated scalar flags remain unsupported.
Agent correction: read the acceptance receipt, use the exact current commit and bind each reply to its actual parent request.
No automatic guessed correlation should be introduced when multiple requests exist.

### 16. P3 - Backlog, schedule and task identity remain separate enough to create misleading operational views

Classification: observed lifecycle friction; root cause needs focused fixture confirmation before code changes.
Transcript evidence: the already spawned launcher is subsequently added to backlog at lines 14030-14046 and appears queued with `repo: broker`; V2 receives its protective hold only at lines 9404-9420, well after V3 was created.
Current task records carry the real project/attempt, while backlog additions without explicit project/start metadata can retain defaults.
Small robust fix: provide a supported intake/spawn path or compact reconciliation action that preserves separately owned backlog and task records consistently.
Make superseded retained task disposition explicit when replacing a failed task with a fresh identity.
Do not infer completion or delete retained work merely from a backlog row.
Required verification: add-before-spawn, add-after-spawn, queued admission, replacement, retained failed task and completed local delivery.

### 17. P3 - Nested task lookup initially uses the root state directory

Classification: observed agent routing mistake, partly addressed by current scoped-owner contracts.
Transcript evidence: lines 10093-10115 inspect `NBA_WEB_TESTS_CI` in root state and receive missing metadata; line 10405 correctly supplies V3's own state directory.
Worker self-inspection also encounters its task missing from its child-home state at line 9343.
Current briefs explain the separation between parent status owner and the worker's own operational home, but command construction remains cumbersome.
Small robust correction: use a task handle containing owner state and current attempt, and give child records explicit parent task links in compact inspection.
The existing packaged environment-routing changes in PR #58 must be checked before treating worker environment confusion as a new defect.
Do not solve this by globally scanning unrelated private homes or changing another home's state.

### 18. P3 - Detached allocation was mistaken for a worker branch violation before execution failure was checked

Classification: observed agent diagnosis mistake with a scaffold clarity opportunity.
Transcript evidence: lines 10976-11035 first request branch repair, then discover the nested model had failed before execution.
Current cause of detached state: `crates/multplx-domain/src/lifecycle/worktree.rs:777` intentionally allocates with `git worktree add --detach`.
The brief at `crates/multplx-domain/src/lifecycle/brief.rs:234` requests the task branch, but no model had run to establish it.
Small robust correction: verify provider execution before judging branch setup, and state in the generated brief that the allocation starts detached and the implementer establishes its task branch before committing.
Do not treat every detached worktree as corruption or retarget the user's primary checkout.

### 19. P3 - Trust-prompt transport and app approval were conflated with task execution

Classification: observed provider/app interaction plus agent handling friction; dedicated hooks audit owns detailed fixes.
Transcript evidence: lines 565-575 attempt ordinary text `1`, which returns `Enter swallowed`; lines 883-896 then require human trust confirmation; lines 4050-4617 perform many key/navigation/capture calls for repository and hook trust.
The initial refusal of an action is an app/tool approval event, not evidence of a Multplx allocation failure.
Text submission and single-key prompt navigation are different operations; the current send owner exposes `--key`.
Small robust correction: detect and report readiness prompts explicitly and use the supported key operation after valid authorization.
Do not remove provider trust review, invent approval, or assume a transport success proves a choice was accepted.
The user supplied trust authorization at lines 893-896, so repeat requests for the same already-authorized scope were unnecessary.

### 20. P2 - Final launcher validation omitted the existing browser invocation contract

Classification: observed NBA product regression, fixed within the transcript, not a current Multplx runtime defect.
Transcript evidence: launcher delivery at lines 16803-16810 cites unit/coverage and HTTP smoke checks; lines 17449-17452 identify ignored Playwright Vite port arguments; lines 18876-18980 rerun the integrated branch's checks; final CI passes at lines 19789-19816.
The combined launcher ignored the existing `bun run dev -- --port 4173 --strictPort` contract and caused hosted E2E waiting.
Small robust process correction: when replacing an entry point, inspect existing callers and run their actual invocation against the integrated commit.
Keep the already implemented argument-forwarding regression test rather than introducing a generic mandatory review ceremony for every small change.
No further NBA product edit is authorized by this audit alone.

### 21. P3 - CI observation uses repeated checks and duplicated trigger runs before bounded waiting

Classification: observed agent observation overhead and NBA workflow behavior, not proof of a Multplx defect.
Transcript evidence: lines 17087-17448 repeatedly inspect the same two E2E runs; lines 19591-19788 finally use `gh run watch` for the replacement checks.
Both push and pull-request triggers create runs; their existence is expected with that workflow configuration.
Small robust correction: poll once to identify the exact runs, then use bounded watch or a meaningful backoff and inspect logs only after a concrete timeout/failure signal.
A product CI concurrency policy can cancel superseded runs, but duplicate triggers should not be changed in Multplx merely because they were visible here.

### 22. P3 - Publication refresh overwrites hand-edited PR presentation

Classification: observed delivery ergonomics issue with current behavior to confirm in the delivery owner before implementation.
Transcript evidence: lines 16969-17056 set human-oriented titles/bodies; line 19309 says the title must be restored after publication refresh; lines 19311-19379 rewrite both descriptions again.
Delivery help at line 16684 explicitly says an existing open PR revision updates its title/body.
Small robust fix: keep a canonical requested title/body in the publication request, or preserve existing manually curated presentation unless the current request explicitly replaces it.
Continue verifying canonical PR identity, head and base before any update.
Required verification: first publish, exact replay, new commit refresh, explicit body replacement and uncertain remote mutation.

### 23. P3 - Stacked publication presents cumulative changes against main

Classification: observed permitted publication choice, not a correctness failure.
Transcript evidence: lines 16815-17086 publish both dashboard and launcher PRs against `main` with a stated merge order; lines 19809-19816 repeat that order.
The second branch includes its prerequisite dashboard commit, so review against `main` includes already reviewed prerequisite changes until the first PR lands.
Small robust improvement: expose an explicit stacked base option and describe dependency/merge order in the generated publication output.
The existing `--base` owner already supports task-bound publication bases; the agent could choose a parent branch when appropriate.
Do not automatically retarget a user's requested base or merge either PR.

### 24. P3 - Small command-construction mistakes repeatedly interrupt progress

Classification: observed agent mistakes, not grounds for runtime weakening.
Transcript evidence: malformed project path at lines 1251-1256, context file used before creation at 3578-3583, unsupported retain `--apply` at 5017-5022, invalid regex option handling at 945-961, unescaped apostrophe at 16213-16223 and Git inspection from the runtime directory at 18022-18027.
Small robust correction: reuse exact recorded paths, construct prerequisite files before passing them, use command help once, use explicit cwd and pass structured arguments or safe quoting for free text.
Unknown worktree project selector at lines 13640-13644 is likewise an exact-registration lookup limitation, not evidence that Git work was lost.
Add targeted examples only where repeated errors demonstrate an unclear public contract.

### 25. P3 - Research evidence typo causes a valid but avoidable correction round

Classification: observed worker reporting mistake, successfully caught before use.
Transcript evidence: lines 8836-8973 identify and correct a transposed source revision in the research report.
Small robust correction: capture the inspected revision directly from Git into structured result evidence and render it into the report once.
Preserve the original artifact and correction history; do not infer that the substantive endpoint research was invalid from one revision typo.

### 26. P3 - Successful quality checks were repeated after integration, but their scope was not always explained

Classification: observed communication/process opportunity; repeated final integration checks were justified by changed code.
Transcript evidence: initial product result at lines 13202-13211, launcher result at 16803-16810 and integrated launcher verification at 18876-19267 each report different exact revisions.
Tests against the source worker branch do not validate a later cherry-pick automatically, so the final integrated E2E rerun was appropriate.
The initial launcher smoke and frontend coverage did not establish all existing browser startup behavior, as finding 20 shows.
Small robust correction: state which new change or integration risk each verification resolves and stop repeating checks after the exact final revision passes unless another change or concern appears.

### 27. P3 - Orchestrator directly takes over final integration and testing despite its delegation boundary

Classification: observed agent role-contract violation, distinct from the technical correctness of the final result.
Transcript evidence: lines 18821-19287 show the main conversation cherry-picking the follow-up, running product lint/tests/build/E2E, creating delivery evidence and reporting worker done.
The operating contract delegates implementation, testing and reviews and permits narrow parent inspection; it does not make integration implementation exempt.
Small robust correction: assign final integration and exact-commit verification to the responsible implementer, then have the parent validate returned evidence and deliver the result.
Retain methodological freedom for the worker and avoid compulsory extra research/review layers for a small fix.

### 28. P3 - Useful product limitations are disclosed, but full visual and live-data acceptance remain unproven

Classification: observed evidence limitation rather than a demonstrated implementation defect.
Transcript evidence: lines 13202-13211 disclose retained desktop analytics and NBA API limitations; tests are deterministic/mocked and the parent repeatedly reads source rather than recording a browser visual acceptance artifact.
The supplied transcript does not establish a full visual inspection of desktop/narrow layouts, live current-season data correctness or every participating player's real-data path.
Small robust correction: preserve a compact exact-revision browser acceptance artifact for visible product criteria and explicitly label mock/live boundaries.
Do not demand live NBA endpoints for deterministic CI or fabricate successful unavailable data.

### 29. P1 - Own-task publication helpers route to the worker's child state instead of its recorded parent owner

Classification: observed current repository defect confirmed by independent worker transcripts and current code.
Worker evidence: V3 JSONL events 1916-1919 fail `mx-deliver.sh prepare` with `private task metadata unavailable`; event 1933 confirms separate child operational state and parent reporting state; events 1937-1940 succeed with an explicit state override.
`NBA_WEB_TESTS_CI_ROOT` events 351 and 380 retain the same helper failure in delivery limitations; `NBA_DEV_SCRIPT_E2E_COMPAT` events 500-510 repeat the failure and workaround.
The main transcript reflects that workaround at lines 12322-12329.
Current cause: `crates/multplx-cli/src/review.rs:88` selects `MX_STATE_OVERRIDE` and ignores the separately configured report owner; prepare at line 3285 reads metadata from that child state.
Small robust fix: resolve an explicitly named bound own task through its validated recorded parent/status owner, including `MX_REPORT_STATE_OVERRIDE` only after exact task, attempt, home and owner checks.
Keep child-task commands and no-argument portfolio scans in the worker's own operational state.
Do not globally reverse state precedence, which could redirect child operations into the parent's portfolio or conflate colliding task IDs.
Required verification: distinct root/worker homes, same task ID in both homes, exact own-task prepare and registration, child-task prepare, stale binding, foreign report owner and no-argument scans.

### 30. P2 - Informational request answers remain outstanding until workers emit terminal task reports

Classification: observed current repository design defect, not an agent forgetting correlation IDs.
Worker evidence: V3 correlated working replies at JSONL events 370, 416, 447, 1265, 1778 and 1900 include revision acceptance, a Bun PATH acknowledgement, a product fix and integration progress, but leave those correlations outstanding.
Final done at event 1974 rejects for pending correlations, then events 1980-1995 emit seven done reports to settle separate requests, including already acknowledged informational guidance.
Root CI events 284-287 and 365-375 show the same pattern.
The main transcript's accumulated wake settlement at lines 12941-13093 is consistent with that worker reporting burden.
Current cause: `crates/multplx-domain/src/lifecycle/pending_reply.rs:477` excludes working from canonical response kinds, while `crates/multplx-domain/src/supervision.rs:597` gates terminal reports on outstanding requests.
Small robust fix: model request acknowledgement and answer separately from task lifecycle, with a durable exact-bound envelope for each request.
An informational answer can settle that request without marking the implementation complete; a task-producing request must retain its actual deliverable obligation until proven satisfied.
Do not make every working report silently settle every request and do not weaken completion evidence, terminal status or human-blocker gates.
Required verification: two simultaneous informational/task-producing requests, acknowledgement versus answer, exact retry, wrong correlation, foreign owner, stale revision and later independently proven task completion.

### 31. P2 - Scope revision changes leave live worker report and child-spawn binding stale

Classification: observed current workflow ergonomics defect with valid stale-evidence rejection.
Worker evidence: V3 JSONL event 340 reads accepted revision 2, event 347 rejects an ordinary report as stale, and event 363 shows the process still carries `MX_BRIEF_REVISION=1`.
The worker then supplies explicit attempt/generation/revision fields for reports; child launch event 1020 rejects the stale parent binding and event 1024 succeeds with revision 2 supplied.
The main transcript records the accepted scope revision at lines 9234-9258 but does not show a supported process-binding acknowledgement.
Current cause: launch environment binds the accepted revision once in `crates/multplx-cli/src/lib.rs:4805`; `report_mcp.rs` lacks explicit attempt/revision fields and the task-dispatch skill only says to send the revised assignment.
Small robust fix: provide a supported exact-current revision acknowledgement/rebind operation or explicit structured binding fields, and include its use in revision notifications.
The worker must acknowledge the new scope before reporting against it; do not silently promote historical evidence or old MCP bindings to the new revision.
Required verification: accepted revision change during execution, explicit acknowledgement, stale report rejection, child launch after acknowledgement, evidence still bound to its original revision and multiple simultaneous revision messages.

### 32. P3 - Worker tool errors include valid limits and local test collisions, not further runtime defects

Classification: observed worker mistakes or already-fixed NBA behavior.
Worker evidence: V3 event 440 rejects a report polluted by shell backtick execution; corrected quoting succeeds at 447.
Nested launch event 1013 uses unsupported ordinary-spawn `--json`; JSON output is documented for coordinator forms only.
E2E event 489 exceeds the advertised 300-character MCP summary limit; shortened event 496 succeeds.
Launcher retry event 130 runs concurrent pytest commands sharing `.pytest-tmp`; serialization passes, with no evidence that Multplx scheduled two workers into the same worktree.
The E2E worker also fixes detached product process shutdown in `bff84c9`; that is NBA launcher behavior, not Herdr lifecycle failure.
Small robust correction: keep short structured summaries with artifact pointers, follow the advertised flag combinations and avoid shared scratch paths within parallel local checks.
Do not loosen schema limits, turn unknown flags into silent success or add generic retry loops.

## Grouped assignments

1. Hook/launch readiness implementer: findings 6, 9, 10, 11 and 19, plus the dedicated hook audit's final evidence.
Own provider-aware model input preflight, explicit readiness/approval states and the verified supervision path; coordinate any shared `lib.rs` edits with assignment 2.
The hooks research proposes stable CLI worker hook keys/adapter hashes loaded in the primary, guarded no-ops in primary context, and native approval invalidation when the hook bundle fingerprint changes.
Treat that as a proposal requiring native-provider empirical verification, with no bypass of trust review or provider private-state edits.
2. Persistent lifecycle and observation implementer: findings 1-5, 7, 12, 13 and 18, with `lib.rs`, lifecycle spawn/worktree/brief/home owners, `task_session.rs`, snapshot projection and their focused fixtures.
Complete exact persistent replacement semantics before expanding recovery commands; preserve process identity, private-home isolation and bounded read-only observations.
3. Reporting and publication implementer: findings 14-17, 22, 29-31, with `pending_reply.rs`, report owners/MCP, task revision handshake and task-specific delivery state routing.
Preserve terminal/evidence gating while separating informational request settlement from implementation completion, and keep own-task publication separate from child portfolio commands.
Do not solve current revision or owner selection by fabricating worker identity or globally reversing environment precedence.
4. Instruction and operational guidance can be integrated into those scopes for findings 8, 17, 18, 21 and 23-28, plus finding 32.
Consolidate concrete examples without expanding the core contract into another large generic checklist.
Findings 20, 21 and 28 include NBA product/testing limitations; any further product work needs its own explicit scoped assignment rather than being silently folded into a Multplx patch.

Each implementation assignment should return its exact commit, focused check results, retained uncertainty and original artifact pointers.
Use fresh user-requested GPT-6.1 SOL Medium agents for the implementation assignments; this audit does not launch or select agents itself.
Run repository-required full validation after the final coordinated implementation, and leave PR merging to the human.

## Limits and exclusions

The worker audit incorporated here covers eight provider transcripts for nine tracked tasks, with 3,446 total JSONL events, including the nested worker.
Its coverage is orchestration events and relevant tool evidence rather than a second full NBA source-patch review.
No provider conversation was discovered for the original dummy `NBA_WEB_REFACTOR`; early parent conversations must not be misidentified as worker runs.
The raw worker paths and event mapping remain in the outside-Git worker audit artifact; this report uses task/event references to avoid distributing private session UUIDs.
Source inspection and report validation are the only checks performed by this researcher; proposed defect fixtures and full repository validation belong to the implementation assignments.
The hooks audit may establish whether a usable queue readiness receipt existed; this report intentionally does not assume it did.
No transcript evidence supports a specific MX Viz server startup failure, wrong Viz URL or rendered graph failure in this main conversation.
The observed Viz-relevant issue is the shared snapshot's invalid empty-home projection in finding 5.
A successful guarded command is not proof that watcher repair occurred, and stale warning repetition is not proof that the current installed queue bridge is broken.
PR #58 packaging/environment changes are already present in the audited base and require regression verification rather than duplicate fixes.
No private state, stale task records, retained worktrees or uncertain executions should be cleaned up as part of these source fixes.

## Integration disposition

The findings above preserve the pre-fix audit at `7cef82e`; their source line references and proposed checks describe that baseline.
The following dispositions distinguish implemented Multplx changes from agent mistakes, historical product fixes and unavailable provider evidence.
All implementation and validation used this development checkout or isolated temporary fixtures, without operational startup in this checkout, activation of user homes, existing user Herdr session control or NBA product changes.

| Finding | Final disposition |
| --- | --- |
| 1 | Fixed in the allocation owner: exact same-task persistent handoff preserves progress, checks ownership before endpoint isolation, rechecks occupants and retains release/prune restrictions. |
| 2 | Fixed in spawn recovery: durable immutable invocation identity and exact reserved successor reconciliation prevent a retry from consuming another attempt/allocation generation; uncertain submissions remain retained. Queued replacements durably publish the successor before finalization and retain the exact dirty allocation through queue drain. |
| 3 | Fixed in brief scaffolding: explanatory prose no longer contains the unfilled task token; actual unfilled assignments still refuse provisioning. |
| 4 | Fixed in provider-session receipts: writer and readers share a 1 MiB bound and retain full process identity; history preserves valid entries alongside path-specific invalid receipt diagnostics. |
| 5 | Fixed in seeding and snapshot projection: new homes receive an empty canonical child backlog; legitimate empty historical worker portfolios remain valid, with the parent assignment shown separately. Seed journal recovery recognizes its exact owned backlog backup. |
| 6 | Fixed with provider-aware model syntax preflight before provisioning/allocation/launch; display labels refuse with canonical-ID guidance and unknown single-token IDs remain unchanged. |
| 7 | Fixed backend precedence: a same-task explicit replacement preserves its recorded backend unless the invocation selects an override. |
| 8 | Agent sequencing correction; revision/transport guidance now distinguishes accepted scope changes from allocation failure, without parsing arbitrary prose as authority. |
| 9 | Agent supervision correction and provider evidence limit: existing readiness-gated queue/foreground protocol remains authoritative; this export cannot prove Desktop queue delivery. |
| 10 | Fixed stale episode notices: the initial actionable warning remains and repeated unchanged reminders stop; a recovered/new episode and queued wakes remain distinct. |
| 11 | Documentation and diagnostics distinguish endpoint creation, native readiness and assignment execution; native trust probes establish consent reuse, not paid model execution. |
| 12 | Fixed with explicit read-only compact inspection containing owner routes, current identity, obligations and evidence pointers; complete forensic inspection remains available. |
| 13 | Fixed supported nested backlog help without mutation; existing launcher/doctor and absolute helper contracts remain the supported entry points. |
| 14 | Fixed with a parent-authored failed/blocked/paused outcome operation carrying exact actor provenance and evidence, separate from worker completion or endpoint stop authority. |
| 15 | Discoverability fixed through compact pending correlations/current commit, scalar-check diagnostics and typed evidence examples; valid correlation and compare-and-swap refusals remain. |
| 16 | Intake guidance/default clarified: backlog rows remain explicitly queued in `workspace` unless project/start metadata is supplied, and protective holds remain explicit owner operations. |
| 17 | Routing clarified through compact owner/child-state handles and exact own-task publication routing; child commands and portfolio scans retain worker-local state. |
| 18 | Brief/docs clarify intentional detached allocation and task branch establishment before committing; detached state alone does not prove a worker violation. |
| 19 | Codex shared primary/worker hook bundle implemented and tested with native consent; separate project/user trust remains native, with Claude/Pi live evidence unavailable and no broader Cursor primary trust. |
| 20 | Historical NBA argument-forwarding regression was fixed in the supplied transcript; no additional NBA change belongs to this Multplx patch. |
| 21 | Agent observation correction: bounded watch/backoff guidance applies; NBA duplicate workflow triggers are not a Multplx runtime defect. |
| 22 | Fixed publication refresh to preserve curated PR presentation after canonical identity/branch/base/commit validation; explicit presentation edits use ordinary forge commands. |
| 23 | Existing explicit stacked publication base already supports the permitted choice; no automatic retarget or merge added. |
| 24 | Agent command-construction correction; exact recorded paths, structured arguments and supported help remain required, with no guard weakening. |
| 25 | Historical research revision typo was corrected before use; original evidence and correction history remain the source of truth. |
| 26 | Agent evidence communication correction: source-worker and integrated commits require distinct current verification; repetition without new changes is not a runtime requirement. |
| 27 | Agent role correction: final integration and verification are assigned to a worker; parent validation does not impersonate worker completion. |
| 28 | Product acceptance evidence limit retained: deterministic NBA checks do not establish full visual or live-data acceptance, and no new product scope is inferred. |
| 29 | Fixed own-task delivery/registration authority resolution with exact task/attempt/revision/runtime-home/state validation, including collision, child and scan isolation. |
| 30 | Fixed exact correlated information acknowledgement/answer dispositions; answers settle only their request and preserve independent completion evidence, human waits, actor/Viz lifecycle projection and watcher pause cadence. |
| 31 | Fixed explicit MCP attempt/generation/revision fields and revision notifications; overrides apply per call and never promote old evidence or silently rebind a live process. |
| 32 | Guidance clarified for short MCP summaries, artifact/body files and exact bindings; schema limits and unsupported-flag refusals remain intact. |

### Integration validation

The first complete behavior run finished in 445.845 seconds with 134 suites, seven failures and eight gated skips. Six failures required fixtures to follow the new hook argument/fingerprint/model contracts; one exposed the missing backlog target in the home-seed recovery whitelist. All seven were corrected without weakening guards or adding skips. Independent queued-replacement fault cases additionally exposed stale legacy endpoint metadata and lost dirty-allocation reuse; those production paths were repaired and all four queue finalization cases passed. The two complete settlement/home-safety suites then passed (156.469 seconds, zero failures or skips).

Validation against the frozen source and release binary:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --locked --workspace` | 861 passed, zero failed, zero ignored; 40 result groups including doc-test groups. |
| `cargo build --release --workspace --locked` | Passed. |
| `target/release/mx shadow-diagnostic` | Ready. |
| `bin/mx-doc-audience-check.sh` | 103 surfaces, 592 local links passed. |
| `target/release/mx test-run --check-coverage` | Inventory complete: 134 total = 113 accelerated + 11 serial + 10 Herdr. |
| Shell syntax | `bash -n` passed for all 255 top-level bin/backend/test shell files. |
| `target/release/mx test-run --all --jobs auto` | 134 suites, zero failures, eight existing gated skips; 492.166 seconds. |
| Unchanged CI workspace/all-target line-coverage gate | Passed: 93.07901489907697% (72,906 of 78,327 lines); 862 tests passed, zero failed/ignored across 34 groups. The 93% threshold and existing exclusions were unchanged. |
| `git diff --check` | Passed. |

The eight gated skips were the optional cmux smoke, Claude stop/autoarm, Codex continuity, Cursor live, launcher live, Pi primary live, Pi primary types and Herdr daemon-marker suites; their gates were not changed. The first unchanged coverage attempt stopped before calculating coverage: an initial ordinary launch in the instrumented settlement suite hit the existing 10-second fake tmux command deadline; 69 of the 70 session-contract suites passed. Its original log remains `/private/tmp/mx-integration-coverage-final.log`.

A bounded diagnosis retained private command/stage traces without source changes. The isolated full settlement matrix passed 618 fake tmux calls (maximum 32.152 ms). Concurrent settlement/push-service/deep-review passed all three suites (618 calls, maximum 39.974 ms); a final comparison inheriting the coverage wrapper environment also passed all three (618 calls, maximum 33.888 ms, Git stage maximum 16.729 ms, inert-helper maximum 23.156 ms). Those private comparisons did not reproduce the canonical runner environment: its isolated workers clear the environment except for selected runtime/profile variables, and use their own home and temporary paths. A second full unchanged coverage run then failed at the same initial ordinary launch (`replay-ordinary-after-replacement-intent`), again with 69 of 70 session-contract suites passing. That repeated failure ruled out classifying it as transient; the canonical diagnosis and fixture correction follow below. Neither deadline, fault cases, exclusions, threshold nor scheduler was changed. Child entry/exit traces also cannot measure delay before the first trace event; cached builds cannot recreate historical cache/scheduling conditions. Full diagnosis and raw results are retained in `/private/tmp/mx-lifecycle-trace/diagnosis.md` and its sibling logs/JSON.

The second failed run is retained in `/private/tmp/mx-integration-coverage-rerun.log`. Canonical fixture diagnostics retained bounded command/stage and worker environment evidence before cleanup. The exact instrumented 70-suite contract reproduced the same failure (69 passed, one failed; 352.790 seconds) in `/private/tmp/mx-integration-coverage-canonical-trace.log`. Its ledger contains Git calls but no first tmux entry; a live process snapshot identified the timed-out fake command as `tmux display-message -p #S`, still before its first shell builtin. Generated settlement mocks now use the runner's exact absolute `$BASH` interpreter, removing the extra `/usr/bin/env` interpreter handoff without changing mock bodies, worker environment, scheduler, deadline or fault cases. The identical canonical contract then passed both integration tests, including all 70 suites, in 450.94 seconds (`/private/tmp/mx-integration-coverage-canonical-fixed.log`). Read-only samples of delayed direct-Bash children showed `_dyld_start +0`; they establish a loader-entry delay but do not identify its OS cause. The following full unchanged coverage run stopped on a separate existing service-test fixture arithmetic bug: a 20-port reservation beginning at 65516 overflowed its exclusive `u16` end, then poisoned the shared test mutex. Checked inclusive reservation now permits port 65535 and rejects overflow/overlap atomically; its new upper-bound regression and all 10 instrumented service tests passed (2.02 seconds). Formatting and strict workspace Clippy passed again. This failure remains in `/private/tmp/mx-integration-coverage-post-startup.log`; the subsequent full unchanged 93% gate passed in `/private/tmp/mx-integration-coverage-post-port.log`, including the 70-suite contract (451.67 seconds) and all 10 service tests. Raw exact totals are retained in `/private/tmp/mx-integration-coverage-summary.json`. The redundant release134 rerun was canceled at 31 seconds at integration direction; the prior full134 pass validates unchanged runtime code, while the final canonical70 pass validates the corrected shell fixture.

The final source freeze contains 44 changed Rust/shell/hook files; private manifest `/private/tmp/mx-integration-source-manifest-final.json` records their hashes. All hashes matched after the passing gate. The normal workspace run above preceded the new port-boundary test; the final instrumented workspace/all-target gate passed all 862 tests including that regression. Tested release SHA-256: `2f595e22996238f7a877c5a53190df57fefecdcd5d78e226a1f90c88dd2ca6e4`. Raw final behavior artifacts: `/private/tmp/mx-integration-behavior-all-final.log` and `.json`.

Provider evidence remains bounded: deterministic launch/communication/lifecycle fixtures exercise mocked transports. The isolated native Codex experiment first displayed four new/changed hooks for native review, then verified consent reuse for the exact primary bundle from two worker directories. A native disabled-hook choice persisted; an unrelated worker project hook still required its own review and remained unapproved. Changing owned script bytes or the selected executable bytes marked all four shared hooks modified; restoring the original bundle restored trusted status and preserved the disabled choice. The experiment performs no paid model turn. It does not establish live task execution, readiness or authenticated Claude/Pi behavior. No new authenticated Cursor trial was run. Original private probe logs and raw integration logs/JSON remain outside the repository in `/private/tmp`; they are not production task records.
