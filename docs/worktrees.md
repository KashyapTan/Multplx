# Built-in Git worktree lifecycle

The domain lifecycle owns Git allocations; session backends consume their exact paths.
The [sub-agent model](subagent-model.md) owns task, attempt and selected-project identity.
[A10](../porting.md#a10-built-in-git-worktree-lifecycle) owns the accepted contract.

## Commands

`mx worktree --help` publishes the implemented grammar.
Commands emit JSON on stdout and diagnostics on stderr.

```sh
mx worktree acquire PROJECT --request REQUEST --task TASK --attempt ATTEMPT --base COMMIT
mx worktree list PROJECT
mx worktree inspect PROJECT ALLOCATION
mx worktree retain PROJECT ALLOCATION --lease LEASE --generation 1 --attempt ATTEMPT --reason 'Keep unfinished work'
mx worktree release PROJECT ALLOCATION --lease LEASE --generation 1 --attempt ATTEMPT
mx worktree prune PROJECT ALLOCATION --lease LEASE --generation 1 --attempt ATTEMPT
mx worktree prune PROJECT ALLOCATION --lease LEASE --generation 1 --attempt ATTEMPT --apply
```

A project is an existing checkout path or registered selector.
Base must be the exact accepted commit; acquisition never fetches a newer branch or modifies the source checkout's branch, index or working files.
Remote-free repositories work without a fabricated remote.
Linked checkouts share the allocation store of their common Git directory.
An unavailable source, unborn repository or invalid base refuses acquisition.

Ordinary spawn acquires before creating the endpoint and writes its allocation into the durable launch intent and canonical task metadata.
The backend receives that exact directory without shell-driven acquisition or cwd polling.
Spawn and teardown share a per-task lifecycle lock through endpoint and allocation publication, so replacement cannot race a stale endpoint stop.
Recursive cleanup takes each child's matching lock and rereads its authoritative metadata before acting.
These task guards do not serialize unrelated tasks or hold a home-wide lock across Git work.
An uncertain launch retains its allocation and launch intent for reconciliation.
The worktree command reconciles repeated acquisition requests; broader uncertain endpoint recovery remains with the delegation lifecycle.

## Records and disposition

Version 1 allocation records live in `COMMON_GIT_DIR/multplx-worktrees/records/ALLOCATION.json`.
Isolated paths live in a private namespace beside the primary repository, keyed by common Git identity.
Independent homes share the repository reservation and cannot acquire the same request twice.
The record includes request, owner home, project/checkout, exact base, task attempt, persistence, path, directory identity, lease identity and generation.
A short filesystem lock publishes a process-identity-bound repository operation reservation; Git runs after that lock is released.
A live or uncertain reservation owner cannot be displaced merely because a contender's bounded wait expires.
Git mutations and cleanup probes have bounded execution and output; a timeout stops their owned process group and retains unresolved resources.
Bootstrap and doctor probe actual Git worktree and merge-tree capabilities without requiring the runtime installation to be a Git checkout.

| State | Meaning |
| --- | --- |
| `reserved` | Durable acquisition intent exists; Git creation may need reconciliation. |
| `active` | Git identity and base were verified and ownership published. |
| `retained` | Work or uncertainty is preserved with a reason. |
| `disposed` | Safe release checks passed; files remain until explicit prune. |
| `removing` | Exact-target Git removal is in progress; retry reconciles Git inventory. |
| `removed` | Git inventory and path removal have been reconciled. |

Acquisition currently chooses a fresh path; it does not maintain a warm pool or reset disposed worktrees for another task.
The same request returns its existing active allocation, while a request with conflicting identity is refused.
An explicitly reconciled replacement attempt continues its existing worktree through an owner-controlled transfer that issues a new lease and generation.
The prior token remains historical evidence and cannot release the replacement.
Herdr projection recovery quiesces its exact restored shell through a recorded holding pane in the owning home before the allocation owner checks occupants and transfers the worker lease.
The session owner then replaces that holding pane at the exact worker path; uncertain intermediate topology retains both receipts for recovery.
Projected teardown holds the same session presentation lock as spawn and recovery through endpoint proof, pane close, focus restoration and journal retirement.
No ignored content is classified as disposable cache.
Unknown content is preserved, and later tasks receive separate worktrees.

Release and prune require the exact recorded lease, generation and attempt.
Dirty, untracked, unclassified ignored, unlanded or occupied worktrees remain retained.
Unavailable occupant observation also retains work.
A persistent allocation does not expire when its process exits.
Prune previews one disposed allocation and repeats safety checks when `--apply` is supplied.
It uses Git-supported exact-target removal, without force, reset, clean or repository-wide prune.
Corrupt records and unexplained paths never become free resources.

Doctor and task snapshots consume canonical allocation observations rather than external pool status text.
Unknown and foreign worktrees remain visible in inventory without becoming cleanup candidates.

## Persistent private homes

`mx home-seed ID - --no-projects` provisions a private directory and durable home reservation.
An explicit new home path uses the same private-directory model.
Runtime assets stay at their installed owner; the home receives links to assets and private coordination/configuration state.
Home seeding records project references without cloning the application or every project.
It works with packaged runtime assets that have no Git repository.

`data/.home-allocation-ID.json` owns the private-home reservation, including path, lease, generation, directory identity and owner home.
Canonical persistent task metadata binds that lease independently of the current model process.
Interrupted seeding preserves the reserved home and restores recorded parent artifacts.
Retirement checks the exact home lease and occupant state, then preserves operational material in a recorded archive beside the owning home.
The receipt records the retained path; retirement never implies that the archived material is disposable.
A later provisioning at the original path gets a new lease and generation.
For a deliberate Git-backed home, acquire with `mx worktree acquire PROJECT --request REQUEST --task ID --attempt ATTEMPT --base COMMIT --persistent`, then seed its exact path with `mx home-seed ID PATH --no-projects --git-allocation PROJECT ALLOCATION`.
The home receipt binds the persistent Git allocation; arbitrary existing checkouts are never adopted by this option.
Retirement of a Git-backed home retains it at its original path with its private files and Git reservation, preserving inventory integrity.
Legacy homes with unproven ownership remain retained for explicit migration.

## Migration boundary

`mx worktree legacy-inspect METADATA RECORDED_PATH...` emits retained migration observations.
The inert migration reader accepts explicitly supplied metadata and recorded paths only.
It preserves v2.0.1-style records without inventing a missing per-acquisition lease ID, and retains known later fields as source evidence.
A missing leased observation remains unknown (`null`), rather than becoming proof that the resource is unleased.
Unknown metadata versions, duplicate paths, missing recorded entries and corrupt records refuse interpretation.
No read executes the external provider, scans its global pools or adopts a worktree.

Phase 09 owns executable transfer: prove ownership, quiesce old endpoints and wrapper shells, persist a relocation intent, move a verified resource out of the external pool using Git, and update each reference through its owner.
The relocation intent carries source digest, old/new paths, common Git identity, process-quiescence evidence, reference receipts and progress.
Its structural validator rejects malformed or inconsistent receipts; Phase 09 must still prove live ownership, wrapper shutdown and reference completion before executing any transfer.
Unsupported moves and uncertain ownership retain the original work and its evidence.
The [research record](../plans/lean_redesign/treehouse-replacement-assessment.md) links the pinned upstream schema and explains the transfer risk.
