---
name: project-management
description: >-
  Agent-only procedure for Multplx project management.
  Use before adding, creating, or removing a project.
  Owns project add, create, clone, remove, registry, delivery-mode, autonomy, and outward-consent decisions.
user-invocable: false
metadata:
  internal: true
---

# project-management

Use this procedure before adding, creating, or removing a project.
This skill is the single owner of Multplx's project-management procedure.
It does not replace `daemon-provisioning`, which owns project clones inside persistent daemon homes.

## Preconditions and registry

Projects live flat under `projects/`, and `data/projects.md` is the private system registry.
Use the registry format and parser contract owned by the header of `bin/mx-project-mode.sh`.
Keep each registry description useful for identifying the project, but keep delivery posture, maintainer-private state, and detailed project knowledge in their existing designated homes.
Do not turn the registry into project documentation.

Resolve the project name, destination, delivery mode, and autonomy posture before changing local or remote state.
Keep a newly added clone and its registry entry consistent, and roll back only artifacts created by the incomplete operation when a later setup step fails and that rollback is safe.
Do not overwrite or repurpose an existing path.

## Delivery posture

Choose the delivery mode when adding or creating the project:

- `deep-review` runs the full local validation pipeline before credentialed PR delivery and is the default when the maintainer does not specify a mode.
- `direct-PR` skips the deep-review pipeline but still uses credentialed PR delivery.
- `local-only` has no required remote or PR and lands only through the approved local fast-forward path.

The optional `+yolo` posture changes routine approval authority but does not change the delivery mode.
Default it off, and enable it only on the maintainer's explicit instruction.
`AGENTS.md` section 7 owns the complete authority boundary and exceptions when it is on.

## Add or clone an existing project

Confirm the source URL, local project name, delivery mode, and autonomy posture.
Clone into `projects/<name>` and add the registry entry only after the destination is known to be unused.
A `deep-review` project must have an `origin` remote.
A `direct-PR` project also needs an `origin` remote.
A `local-only` project may have no remote.

## Create a project

Creating a GitHub repository is outward-facing.
Before making that remote change, propose the repository name, owner or organization, visibility, and delivery mode, defaulting visibility to private and delivery mode to `deep-review`, then obtain the maintainer's explicit consent for those values.
Use official `gh` for the approved GitHub operation and consult its current help rather than relying on remembered flags.
After remote creation succeeds, clone it locally and add the registry entry with its approved delivery mode.

For a purely `local-only` project, create a local Git repository under its unused `projects/<name>` path, add the registry entry, and make no GitHub call.
The maintainer's request to create that local project authorizes this local initialization, but it does not authorize an unmentioned remote repository.

## Validation readiness

The `deep-review` gate is part of Multplx and needs no per-project initialization.
Before dispatching work, confirm the project has a valid default branch and that its tracked `.deep-review.yaml`, when present, parses under the in-repo gate.

## Remove

Project removal is destructive and is not one of Multplx's current direct-write exceptions under `projects/`.
Never issue a raw removal command from Multplx.
First obtain the maintainer's explicit removal decision, then inspect the current digest and authoritative repositories for in-flight or queued work, registered daemon clones, linked worktrees, dirty files, unpushed commits, and any other unlanded work.
If any dependency or unlanded work exists, stop and report it before changing the registry.
Until a guarded removal helper and corresponding prime-directive exception exist, report that implementation gap instead of bypassing the project-write boundary.
When a clone has already been removed through an approved guarded path, or the registry is provably stale because no clone exists, remove its registry line so navigation matches reality.
