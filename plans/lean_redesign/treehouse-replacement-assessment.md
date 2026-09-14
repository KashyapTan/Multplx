# Replacing Treehouse with the built-in Multplx worktree lifecycle

## Decision and research boundary

Launch from any directory and multi-task delegation across repositories are standard product behavior.
A dev folder is only an example of an optional discovery root.
The user requested complete removal of Treehouse as a runtime dependency while retaining the worktree behavior Multplx needs.
[A10 in porting.md](../../porting.md#a10-built-in-git-worktree-lifecycle) owns that accepted contract, and [Phase 03](03-built-in-worktree-lifecycle.html) owns implementation.
This record supplies research and source mapping; it does not claim the replacement is implemented.

Research on 2026-09-14 covered Kun Chen's [kunchenguid/treehouse](https://github.com/kunchenguid/treehouse), its current README, and the pinned v2.0.1 command/pool/state code used by this repository's dependency contract.
The pinned source tree identifier returned by GitHub was `5b8ecdec49034fe6861d63b8ea331490bb14c946`.
Upstream source was read in a temporary research directory; no upstream code was executed, installed or vendored and no private pools were inspected.
The excluded firstmate tree was not used.

## What the upstream product provides

Treehouse is a Git worktree pool manager with shell entry, reuse, leases, inspection and cleanup.
Its current documentation includes features beyond Multplx's needs, including configurable base selection and identity-conditioned returns.
Its broader product is not the replacement's scope. [Current upstream documentation](https://github.com/kunchenguid/treehouse)

At v2.0.1, the ordinary get command acquires a path and opens a shell; lease mode instead persists a reservation and prints a path.
The shell-exit path may detach, prompt over dirty work, terminate resident processes and return the slot.
That coupling is unnecessary when Multplx already owns task endpoints and lifecycle. [Pinned get implementation](https://github.com/kunchenguid/treehouse/blob/v2.0.1/cmd/get.go)

The pinned pool implementation chooses a default branch, fetches origin when present, reuses an available slot or creates another, and resets a slot on return.
We preserve useful isolated acquisition but use the task's recorded base and Multplx's disposition checks instead of adopting those policies wholesale. [Pinned pool implementation](https://github.com/kunchenguid/treehouse/blob/v2.0.1/internal/pool/pool.go)

The pinned state file is `treehouse-state.json`, containing worktree entries with path, short-lived process ownership and durable lease facts.
It has no per-acquisition lease ID; corrupt-state recovery conservatively reserves discovered worktrees.
Migration must not infer newer identity guarantees from these records. [Pinned state implementation](https://github.com/kunchenguid/treehouse/blob/v2.0.1/internal/pool/state.go)

Git already supplies worktree creation, machine-readable inventory, movement and removal.
The replacement therefore needs a small ownership and recovery layer around Git, not a second implementation of Git itself. [Git worktree documentation](https://git-scm.com/docs/git-worktree)

## What Multplx actually calls today

| Source | Observed integration | Replacement work |
| --- | --- | --- |
| [CLI spawn](../../crates/multplx-cli/src/lib.rs), ordinary spawn near `actor_worktree` | Sends literal `treehouse get` into tmux/Herdr/cmux, polls cwd up to sixty times and checks top-level isolation before writing metadata | Acquire through a typed internal call before endpoint launch and pass the exact path. |
| [Home seed](../../crates/multplx-domain/src/lifecycle/home_seed.rs), `seed` and rollback | Uses `get --lease --lease-holder <id>` when the home argument is `-`, captures a path, and invokes forced return on rollback | Durable internal allocation/receipt or private-directory home for packaged non-Git runtime assets. |
| [Teardown](../../crates/multplx-domain/src/lifecycle/teardown.rs), `return_worktree` and `remove_home_with` | Validates task work and invokes `treehouse return --force`; retries some Git index-lock failures | Keep proven work-retention checks, but use exact allocation generation and Git-supported cleanup. |
| [Probe](../../crates/multplx-core/src/probe.rs) and [bootstrap](../../crates/multplx-cli/src/bootstrap.rs) | Require the executable and its lease flag, and offer an upstream installer | Remove external readiness/download checks; validate the internal capability. |
| [Doctor](../../crates/multplx-cli/src/doctor.rs), `orphan-worktrees` | Checks missing task paths; the extra Treehouse orphan comparison uses an optional fixture file in this code path | Reconcile actual canonical allocation and Git inventory, not a fictional live pool observation. |
| [Installer](../../crates/multplx-backend/src/treehouse_tools.rs) and [CI](../../.github/workflows/ci.yml) | Pin v2.0.1 with platform checksums and install it for integration lanes | Retire the dependency and prove integration with Treehouse absent. |
| [Briefs](../../crates/multplx-domain/src/lifecycle/brief.rs), [backend adapters](../../bin/backends/) and tests | Assume pooled detached worktrees, shell entry and fake provider commands | Keep isolation/outcome checks while replacing provider-specific mechanics and fixtures. |

The worktree source is the assigned project, not the launch directory or the orchestrator's runtime directory.
This matters for ordinary multi-repository delegation and for launch-anywhere behavior.
The existing shared Git checks and session identities remain useful; removing Treehouse must not remove them.

## Small compatibility surface

| Existing use | Internal equivalent outcome | Deliberate simplification |
| --- | --- | --- |
| Ordinary get plus cwd discovery | Return a validated isolated worktree for the task's exact base and start its endpoint there | No intermediate interactive shell and no cwd polling. |
| Durable home acquisition | Persist ownership while no process is running | One internal allocation model; packaged directory homes do not require a runtime Git checkout. |
| Return after teardown checks | Release the exact allocation only after disposition and process reconciliation | No unconditional path-only forced reset. |
| Pool status used for reconciliation | Canonical worktree/allocation observation | No second provider database or text-scraped status authority. |
| Reusable idle worktrees | Reuse only verified safe internal allocations, otherwise create fresh or queue | No prewarming, heuristic eviction or arbitrary lifecycle hooks. |
| Old task/home paths | Explicit versioned transfer with preserved evidence | No global takeover of Treehouse installation or pools. |

The internal CLI needs only acquire, inspect/list, release/retain and scoped prune.
A10 defines their invariants; exact syntax is published with the implementation.
Task launch uses the typed operation result directly, so agents do not need to reason through shell prompts or manually manage pool slots.

## Migration concerns

A missing process does not make an old persistent lease disposable.
A later task reusing a path must not be vulnerable to a delayed release from its predecessor.
The new allocation identity solves that only after ownership has actually transferred.

An old Treehouse wrapper can run cleanup when its shell exits.
Simply importing its path into new metadata creates two managers of the same files.
The migration must quiesce that wrapper and isolate transferred resources from future external-pool reuse.
A10 specifies recoverable Git-supported moves and retained legacy handling when movement or ownership is uncertain.

Do not remove the user's global Treehouse executable or alter unrelated pool entries merely because Multplx no longer needs the tool.
All new normal operations must work without it; migration must preserve unknown historical work rather than manufacture safe ownership.

## Verification emphasis

Use real temporary Git repositories for allocation, reuse, removal and branch/dirty-state assertions, with controlled fault injection around records and endpoint operations.
Exercise concurrency, persistent zero-process leases, duplicate requests, stale release generations, ignored/untracked data, open PRs, uncertain occupants and corrupt allocation records.
Exercise every supported session backend without widening the current persistent-home support matrix.

Record fresh/reused checkout setup latency and disk/cache retention in the existing workload trials.
Removing an external process and cwd polling should simplify launch, but it does not establish a measured speedup by itself.
The worktree manager is a required dependency replacement, and unmet migration or safety checks remain open release acceptance.

## Scoped coordinator integration

[A11](../../porting.md#a11-scoped-sub-orchestrators) adds domain sub-orchestrators through [Phase 05](05-scoped-sub-orchestrators.html).
Their private homes use the same A10 lifecycle, and worker allocations share canonical project ownership across homes without cloning every repository or restoring Treehouse.
