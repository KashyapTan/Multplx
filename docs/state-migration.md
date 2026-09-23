# Operational-home migration

`mx migrate` upgrades one existing operational home to the lean task, project, configuration and recovery contracts without replaying work.
Run it with the release binary that will own the migrated home.
Do not run an older writer against a migrated home.

## Inspect and apply

Bring the main harness and participating persistent sessions to a safe boundary first.
Inspection is read-only and reports conversions, retained compatibility records, explicit role mappings and blockers.

```sh
mx migrate inspect --home /absolute/multplx-home --operation lean-phase09-v1
mx migrate apply --home /absolute/multplx-home --operation lean-phase09-v1
mx migrate summary --home /absolute/multplx-home --limit 20
```

Use `--coordinator TASK` only when the recorded charter responsibility proves that a persistent legacy task coordinated a domain.
Persistence by itself never maps a task to the sub-orchestrator assignment.
Ambiguous roles, parents, attempts and briefs remain visibly unknown for reconciliation.

Apply refuses a live session writer, unsafe symlink, newer schema, malformed owned record or conflicting canonical and legacy alias.
It copies legacy harness and dispatch settings to their canonical names while retaining the aliases for this lean release's compatibility window.
It embeds the versioned compatibility model in legacy task metadata without inventing an attempt, accepted brief or completed result.
It assigns stable identities to available flat managed project checkouts without moving them.
Missing project locations remain recorded for repair.

Wake queues, inbox claims, waiting continuations, message and correlation IDs, pending replies, journals, status history, workflow snapshots, decisions, PR records and action receipts remain byte-for-byte with their existing owners.
Historical `yolo`, approval, review and publication fields confer no new authority.
Migration never starts an agent, publishes a branch, opens or merges a PR, resolves a decision or deletes unfinished work.

The private backup at `state/.migration-backups/OPERATION/manifest.json` contains the exact before and after bytes for every converted file.
The recoverable transition receipt resumes the same operation after interruption and rejects a changed intent.
`state/.home-schema-version` is the authoritative completion marker.
The compact summary reports current task, owner, role, attempt, brief, dependency, open-question and evidence identities without replaying transcripts.

## Roll back

Rollback is a stopped-writer recovery action, not a general downgrade converter.

```sh
mx migrate rollback --home /absolute/multplx-home --operation lean-phase09-v1
```

The command requires the matching runtime version, home filesystem identity and backup operation.
Every migrated file must still match either its exact before bytes or its recorded after digest.
A later edit blocks rollback rather than being overwritten.
Successful rollback restores prior bytes, removes only files created by that migration and retains the backup plus `rollback.json` evidence.
Restore matching code and configuration before starting an old writer.

## Legacy worktree transfer

Home migration never enumerates or mutates an external Treehouse pool.
Inspect its inert metadata and transfer only an exact task-owned path after the home is current and every process using the worktree is stopped.

```sh
mx worktree legacy-inspect /absolute/treehouse-state.json /absolute/recorded-worktree
mx migrate relocate-worktree \
  --home /absolute/multplx-home \
  --metadata /absolute/treehouse-state.json \
  --path /absolute/recorded-worktree \
  --project PROJECT_OR_CHECKOUT_ID \
  --task TASK_ID \
  --request STABLE_REQUEST_ID
```

The transfer requires a current task with exact attempt and project identity, an exact matching legacy `worktree=` reference, compatible lease-holder evidence and no observed occupants.
It records a new internal allocation and lease, uses `git worktree move`, then transactionally updates the task and any owned persistent-home receipt.
An exact retry resumes the Git move or reference publication and returns the same durable mapping under `state/.legacy-worktree-mappings/`.
The supplied metadata and unrelated pool entries remain unchanged.
Missing v2.0.1 acquisition identity is preserved as historical uncertainty rather than copied into the new lease.
Foreign, ambiguous, active or unsupported resources stay retained with a specific diagnostic.

## Compatibility window

Schema-1 and unversioned task records plus schema-2 canonical records are supported migration inputs.
Canonical and legacy configuration aliases remain readable for this lean release.
Their removal requires an announced subsequent breaking version with a migration path.
Do not rename durable records, message carriers, endpoint labels or filesystem markers merely to modernize their vocabulary.
Existing `.mx-daemon-home`, route, carrier and endpoint labels remain compatibility evidence while new task and status records use canonical assignment terminology.
Run `mx doctor --check home-migration` to verify the current marker.
