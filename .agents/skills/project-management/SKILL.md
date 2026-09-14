---
name: project-management
description: Short reference for project lookup, local checkout selection and project lifecycle commands.
user-invocable: false
metadata:
  internal: true
---

# Project operations

[A9](../../../porting.md#a9-launch-anywhere-project-discovery-and-one-shared-chat) defines one chat across repositories, launch from any directory and optional recursive discovery roots.
Select an explicit local path or unambiguous known project before considering a URL or clone.
A task keeps its project, checkout and starting revision when the selected context changes.
Keep each repository's instructions with its task; do not load every discovered repository's contract.
Discovery and registration do not authorize changes to a borrowed checkout or its removal.
Remote-free repositories can support local tasks.

`bin/mx-project-mode.sh --help` describes the existing flat registry and path resolver.
`bin/mx-brief.sh --help` accepts project references for scaffolding.
The legacy runtime still has flat managed-clone and mode fields; Phase 02 adds project/checkout identity and Phase 11 implements local registration, discovery and workspace entry.
Do not treat legacy mode or yolo values as a request for a review tool, a project-intake interview or merge authority.
Use a URL for intentional cloning when no selected local checkout supplies the work.
A request to unregister a borrowed repository does not authorize deleting its files.
Exact new registration, discovery and unregister syntax belongs in the implementing command's help.
