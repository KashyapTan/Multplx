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

`mx project --help` and `multplx projects --help` describe local registration, exact resolution, discovery configuration, location repair and metadata-only forgetting.
`mx project register PATH --alias NAME` remembers a user-owned checkout without changing its files.
`bin/mx-project-mode.sh --help` retains the legacy flat-registry compatibility view.
`bin/mx-brief.sh --help` accepts project references for scaffolding.
The [sub-agent model](../../../docs/subagent-model.md) owns project/checkout identity and frozen task bindings.
The [workspace guide](../../../docs/workspace-entry.md) describes launch from any directory, terminal entry and one-chat connection.
`multplx task --help` owns durable terminal intake and retry identity; a receipt does not imply implementation has started.
`multplx shell` retains explicit shell activation.
Do not treat legacy mode or yolo values as a request for a review tool, a project-intake interview or merge authority.
Use a URL for intentional cloning when no selected local checkout supplies the work.
A request to unregister a borrowed repository does not authorize deleting its files.
Exact new registration, discovery and unregister syntax belongs in the implementing command's help.
