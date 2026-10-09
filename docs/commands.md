# Multplx features and human commands

Use this as a reference after [installation](getting-started.md).
The [user guide](user-guide.md) explains the workflows in more detail.
Replace example aliases, task IDs, run IDs and paths with your own values.
Most daily work happens in the main chat; the orchestrator performs worker setup and bookkeeping for you.

## Install, upgrade and remove

From a clone of the repository:

```sh
./install.sh
./install.sh --help
./install.sh --upgrade
```

The first command builds and installs the complete application, including global `multplx` and `mx` entries.
An older installed runtime keeps its existing behavior until you upgrade it.
The installer accepts `--bin-dir PATH`, `--config-dir PATH`, `--data-dir PATH` and `--home PATH` for explicit locations.
It does not install provider CLIs or authenticate them.

After installation:

| Command | Purpose |
| --- | --- |
| `multplx --version` | Show the runtime version. |
| `multplx paths` | Show the selected binary, runtime, configuration and home. |
| `multplx --help` | Show the global command entrypoints. |
| `multplx launcher-install --help` | Show advanced installer options. |
| `multplx launcher-install --uninstall` | Remove owned application files; preserve data and repositories. |
| `multplx update` | Update a legacy source-mode installation; use `./install.sh --upgrade` for the new full installation. |

See [upgrade and uninstall](getting-started.md#upgrade) for active-session and custom-directory requirements.

## Open the workspace or chat

| Command | Purpose |
| --- | --- |
| `multplx` | Open the interactive projects/tasks/decisions/domains workspace. |
| `multplx workspace --plain` | Print workspace state without an interactive terminal. |
| `multplx my-app` | Select a remembered project for the next request. |
| `multplx /path/to/repository` | Select an explicit checkout, registering it when needed. |
| `multplx chat codex` | Start or connect using Codex CLI. |
| `multplx chat claude` | Use Claude Code. |
| `multplx chat cursor` | Use Cursor CLI. |
| `multplx chat pi` | Use Pi. |
| `multplx chat` | Use the remembered harness. |
| `multplx shell` | Open a shell with this installation's runtime and home bindings. |

The shorter `multplx codex`, `multplx claude`, `multplx cursor` and `multplx pi` forms are also supported.
The workspace is a dashboard and task-entry surface; use a chat command for the AI conversation itself.
The terminal keys are:

| Key | Action |
| --- | --- |
| `Tab` | Switch projects, tasks, decisions and domains. |
| Arrows, `j` / `k`, or mouse wheel | Move through the current list. |
| `/` | Search projects. |
| `t` | Submit a task for the selected project. |
| `r` | Refresh discovery and state. |
| `c` | Enter the main conversation. |
| `v` | Open MX Viz. |
| `q` | Close the workspace while independent work continues. |
| `Ctrl+C` | Interrupt the workspace and restore the terminal, including while entering text. |

There is one main conversation per operational home, not one per selected project.
An unavailable attachment route is reported rather than replaced with another main orchestrator.

## Register and discover projects

```sh
multplx projects register ~/dev/my-app --alias my-app
multplx projects list
multplx projects resolve my-app
multplx projects --help
```

Registration remembers an existing Git checkout without changing its branch or files.
Optional discovery helps find nested repositories:

```sh
multplx projects roots add ~/dev --depth 6
multplx projects roots list
multplx projects discover --refresh
multplx projects configure --no-follow-symlinks --exclude node_modules --exclude target
multplx projects roots remove ~/dev
```

Supplying exclusions replaces the configured exclusion list.
Discovery results do not become registered projects until selected or explicitly registered.
To repair a moved checkout or forget a location:

```sh
multplx projects repair CHECKOUT_ID /new/path/to/my-app
multplx projects forget CHECKOUT_ID
```

Forgetting a checkout does not delete it.
See [workspace entry](workspace-entry.md) for identity and ambiguity rules.

## Submit tasks and dependencies

Ask in chat to delegate implementation, research or review:

```text
Fix issue A in web, investigate the flaky tests in api, and research feature B in mobile.
```

For durable command-line submission:

```sh
multplx task --project my-app "Fix login and run the relevant tests"
multplx task --project my-app --request-id login-1 "Fix login"
multplx task --help
```

Retry an uncertain submission with the same `--request-id` and identical request fields.
A changed request needs a new identity or an explicit scope revision through the main conversation.
Useful options are:

| Option | Purpose |
| --- | --- |
| `--project SELECTOR` | Bind the task to the selected project. |
| `--request-id ID` | Make retries converge on the same accepted request. |
| `--batch-id ID` | Group related requests while preserving their individual identities. |
| `--task-id ID` | Choose the requested task ID. |
| `--start REVISION` | Select a committed starting revision instead of current `HEAD`. |
| `--depends TASK` | Wait for a prerequisite; repeat for several dependencies. |
| `--artifact PATH` | Attach an existing context artifact. |
| `--domain COORDINATOR` | Route to a recorded coordinator whose domain includes this project. |

For example:

```sh
multplx task --project api --request-id api-change --task-id api-change \
  --batch-id export-1 "Add the agreed export endpoint"
multplx task --project web --request-id web-change --batch-id export-1 \
  --depends api-change "Connect the export screen to the endpoint"
```

Submission records intake; it does not claim an agent has already started.
Worktrees begin at committed revisions, so uncommitted source files are not silently included.
Ask the orchestrator to change scope, stop or resume assigned work using its exact task ID; changing the selected project never retargets existing tasks.

| Command | Purpose |
| --- | --- |
| `multplx task-session inspect TASK` | Read the exact managed Codex UUID and validated transcript reference for the current task attempt. |
| `multplx task-session history TASK` | Read retained per-attempt identities without asserting live execution or resumability. |

See [exact managed Codex session inspection](configuration.md#exact-managed-codex-session-inspection) for provider readiness, owner selection and transcript limits.

## Coordinate a project or research domain

A scoped coordinator can own related work and delegate its own workers while reporting to the main orchestrator.
Usually ask for this in chat; direct commands are available:

```sh
multplx spawn api-design --sub-orchestrator --project my-app \
  --scope "Coordinate the accepted API compatibility work" \
  --persistent --request-id api-design-initial --harness codex
multplx domain inspect api-design
multplx task --project my-app --domain api-design "Investigate the compatibility failure"
```

For a persistent research assignment without a repository:

```sh
multplx spawn onboarding --sub-orchestrator --idea onboarding \
  --scope "Research onboarding alternatives and report open decisions" \
  --persistent --request-id onboarding-initial --harness codex
```

Use `multplx domain --help` for revision-bound scope changes and project binding, and `multplx spawn --help` for launch options.
Standing workers and coordinators are the agent dispatch default; low-level CLI omission of `--persistent` remains task-scoped for compatibility.
After the parent validates the full agreed job, resolves gaps and delivers it, stop automatic contact, polling, nudging and work routing; a premature done claim still permits corrections.
Persistence retains availability for explicit user-directed follow-up, without automatic reuse or retirement.
See [scoped coordinators](scoped-coordinators.md) for lifecycle and transfer details.

## Standing implementation worker

Use the existing owners to scaffold a worker, fill its task placeholders, seed its private home and launch at a separate exact-base project worktree:

```sh
mx brief fix-login my-app --persistent --role implementer --output implementation
# Fill every {TASK} placeholder in data/fix-login/brief.md before seeding.
mx home-seed fix-login - my-app
mx spawn fix-login --persistent --role implementer --output implementation \
  --project my-app --base FULL_ACCEPTED_COMMIT --request-id fix-login-initial
mx task-model inspect fix-login --compact
```

`--project` must resolve to a checkout already referenced in the seeded home, and `--base` must be the full accepted starting commit, not a moving branch name.
Spawn records the project, base and persistent allocation independently of the private home, then launches the implementer at that allocation's exact path.
The report route remains the parent owner; use the brief's routing instructions and [typed delivery guidance](delivery.md#local-completion-and-dependent-work).
Completion requires current task/attempt/brief evidence matching that project worktree's actual HEAD; wrong or stale HEAD leaves dependency gates closed.
Home and endpoint availability survive completion; the parent must validate the full job before ending corrective follow-up.
Cmux persistent-home launch and persistent task delivery mode/yolo overrides remain unsupported; do not silently fall back to native delegation or temporary lifecycle.
An explicitly temporary worker omits `--persistent`, passes its project positionally to spawn and needs no home seeding.

## Dashboard, workflows and advanced tools

After starting the orchestrator, run `mx viz` or `multplx viz` from any directory, or press `v` in the `multplx` terminal workspace.
They select the configured orchestrator home automatically and open the actual returned dashboard URL, including a fallback port when needed. Normal use requires no environment variables.

| Command | Purpose |
| --- | --- |
| `mx viz` or `multplx viz` | Start or reuse the read-only dashboard and open it. |
| `mx viz --no-open` | Start or reuse the dashboard and print its URL without opening a browser. |
| `mx viz serve` | Scriptable service entry: print the URL without opening a browser. |
| `mx viz status` | Show the dashboard service state. |
| `mx viz stop` | Stop the dashboard, not the agents. |

See [Live system dashboard](viz.md) for advanced home and port overrides.

Some operational tools retain their `bin/` entrypoints instead of a global alias.
To run them from anywhere, open the installation's activated shell and enter its runtime directory:

```sh
multplx shell
cd "$MX_ROOT_OVERRIDE"
```

Run the commands below in that shell; `exit` returns to your original shell.

| Command | Purpose |
| --- | --- |
| `bin/mx-workflow.sh validate workflows/new-feature.workflow.md` | Check a workflow definition. |
| `bin/mx-workflow.sh dry-run workflows/new-feature.workflow.md --input "Add export"` | Preview its stages. |
| `bin/mx-workflow.sh run workflows/new-feature.workflow.md --project my-app --input "Add export"` | Start a project-bound workflow. |
| `bin/mx-workflow.sh status RUN_ID` | Inspect a workflow run. |
| `bin/mx-workflow.sh --help` | Find resume, reconcile and abort syntax. |
| `bin/mx-vplan.sh --help` | Create or serve annotated HTML review artifacts. |
| `bin/mx-deep-review.sh --help` | Inspect the optional review pipeline's commands. |
| `bin/mx-timeline.sh --help` | Inspect a task's journal as text, JSONL or HTML. |
| `bin/mx-system-view.sh` | Show a human-readable system snapshot. |
| `bin/mx-system-snapshot.sh` | Emit the canonical JSON snapshot. |
| `bin/mx-headroom.sh` | Inspect dispatch capacity. |
| `bin/mx-headroom.sh --queue` | Inspect queued requests. |
| `bin/mx-headroom.sh --queue-cancel REQUEST_ID` | Cancel a queued request that has not begun dispatch. |
| `bin/mx-headroom.sh --queue-priority REQUEST_ID PRIORITY` | Reprioritize a queued request. |
| `bin/mx-headroom.sh --queue-drain` | Attempt dispatch and reconcile released capacity. |
| `bin/mx-backlog.sh --help` | Manage the durable backlog through its command owner. |
| `mx task-model inspect TASK --compact` | Read exact owner/attempt, child-state route, revision, endpoint, allocation, pending request IDs and evidence commit without embedded briefs. |

MX Viz's Agents view shows the main orchestrator, nested coordinators and workers, with search, branch controls and task-detail links.
Tasks shows workflow, decisions, delivery, artifacts and freshness.
Missing or partial observations remain explicit.
Workflows follow their declared stages; deep-review and vplan run only when requested or selected by a workflow.

## Health, recovery and lower-level commands

```sh
multplx doctor
multplx doctor --json
multplx doctor --check watcher-lock
```

Doctor exit codes are `0` for healthy, `1` for warnings and `2` for failures.
Its explicit `--fix` mode has narrow repair authority; read [doctor](doctor.md) before using it.
Restarting the main conversation preserves durable tasks but cannot restore a provider transcript the provider itself cannot resume.
Use [home migration](state-migration.md) before reusing a legacy operational home.

For advanced native command discovery, in `multplx shell`:

```sh
MX_MULTICALL_EXPLICIT=1 "$MX_RUST_BIN" --help
MX_MULTICALL_EXPLICIT=1 "$MX_RUST_BIN" worktree --help
MX_MULTICALL_EXPLICIT=1 "$MX_RUST_BIN" task-model --help
MX_MULTICALL_EXPLICIT=1 "$MX_RUST_BIN" request --help
MX_MULTICALL_EXPLICIT=1 "$MX_RUST_BIN" parent-channel --help
MX_MULTICALL_EXPLICIT=1 "$MX_RUST_BIN" migrate --help
```

These commands expose canonical task, worktree, request, outcome and migration operations.
The full [toolbelt index](scripts.md) lists the remaining agent-facing transports; most humans do not need to manipulate their low-level coordination state.

## Skills you can request in chat

| Skill | Use it to |
| --- | --- |
| `afk` | Enter explicit away-mode supervision and return through its owned lifecycle. |
| `catchup` | Summarize current structured state. |
| `recap` | Summarize recent visible events and unanswered questions. |
| `stow` | Save useful session knowledge before a reset. |
| `create-workflow` | Author and validate a reusable process. |
| `updatemultplx` | Update a legacy source installation and registered homes; full installs use the installer upgrade path. |

Claude uses slash forms such as `/catchup`; Codex uses skill forms such as `$catchup`.
You can ask in ordinary language as well.

## Delivery and control

Ask for a local-only outcome when you do not want a push or PR.
Otherwise agents can commit, push task branches and create/update PRs using ordinary authentication.
Only the human merges PRs.
An outcome should include the exact commit, checks, limitations and PR or local artifact.
You remain responsible for provider credentials, product decisions and reviewing the final result.

Backlog rows and task execution have separate owners.
New backlog rows default to queued work in `workspace`; pass `--repo PROJECT --start` when recording work that has already launched.
The intake owner does not infer completion, replacement or project identity from a same-named task.
For a failed task retained while a fresh identity proceeds, record its protective hold explicitly with `mx backlog hold TASK --reason TEXT --kind KIND`.
Root and nested supported backlog help forms, such as `mx backlog add --help`, are read-only.
Use `mx task-model inspect TASK --compact` for subsequent bounded observations, and `--full` to inspect retained brief/history evidence.
A child task's owner state and its worker home's child-state directory are separate routes; use the recorded owner pointer for its own assignment.
