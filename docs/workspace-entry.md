# Workspace entry and local projects

One configured operational home and one main orchestrator conversation coordinate every selected repository.
The installed runtime supplies code and instructions, the operational home holds durable work, and optional discovery roots identify places to search.
These paths are independent of the directory from which `multplx` is launched.

[Getting started](getting-started.md) owns installation and first entry; [configuration](configuration.md) owns persistent settings.
The exact command grammar is available through `multplx --help`, `multplx projects --help` and `multplx task --help`.

## Local checkout lifecycle

Remember an existing checkout without cloning or moving it:

```sh
multplx projects register ~/dev/my-app --alias my-app
multplx projects list
multplx projects resolve my-app
```

Exact aliases, project and checkout identifiers, and validated paths select a known checkout.
Ambiguous names report candidate paths instead of choosing a clone by basename or remote URL.
Distinct clones retain distinct checkout identity; linked worktrees share repository identity while retaining separate checkout locations.
The registry revalidates location identity before dispatch and retains history when a checkout is missing or replaced.
A project context applies to the next request; changing it never rebinds accepted work.
Outside a repository, the launcher shows the workspace overview without silently selecting the last project.
Discovery does not execute repository instructions or register every result automatically.

Registration defaults to user-owned status.
Refresh does not fetch into, reset, stash or clean a borrowed checkout, and unregister removes only the remembered location.
Implementation uses an isolated allocation from an explicit starting commit through the [built-in worktree owner](worktrees.md).
Dirty working files are excluded from that committed base and remain untouched; discuss working changes explicitly rather than assuming they entered the task.
Remote-free repositories support local work, and unversioned folders do not gain Git, worktree or forge capabilities merely by appearing in discovery.

## Configured discovery

Roots are optional and persist under the operational home's `config/project-discovery.json`.
The derived cache at `data/project-discovery-cache.json` contains results, scan progress, observation time and errors; it is not the project registry.
For example:

```sh
multplx projects roots add ~/dev --depth 6
multplx projects roots list
multplx projects discover --refresh
multplx projects configure --no-follow-symlinks --exclude node_modules --exclude target
```

Depth is bounded, overlapping roots and symlink aliases are deduplicated, and Git internals are excluded.
Supplying exclusions replaces the configured list; follow-symlink traversal is explicit and detects cycles.
Cached results remain available while a refresh runs.
Missing roots and folders without a usable Git commit retain visible capability limits.
Use `multplx projects roots remove PATH` to stop discovering a root.
A moved checkout can be repaired with `multplx projects repair CHECKOUT_ID PATH` only when its recorded identity matches; a replacement repository is not a move.

## One conversation and durable requests

An ordinary chat request can include independent work in several repositories:

```text
Implement feature A in repo-one, investigate issue B in repo-two, and research feature C in repo-three.
```

Each accepted item retains its own project, checkout, starting revision, scope, dependencies, evidence and delivery outcome.
An ambiguous item needs clarification without blocking independent items.
The [durable coordination owner](durable-coordination.md) distinguishes accepted, delivered, acknowledged, answered and completed requests.
A terminal receipt establishes durable intake, not a running implementation or a completed task.

```sh
multplx task --project my-app --request-id login-1 --batch-id release-1 "Fix login"
multplx chat codex
```

Repeat the same `--request-id` and immutable request fields after an uncertain submission.
Use separate request IDs with one `--batch-id` for independent items; changed input under an accepted identity refuses.
`--start REVISION` selects a different committed base, and `--domain COORDINATOR` routes to an explicitly recorded domain that contains the project.

The launcher remembers an explicitly selected supported harness.
A second terminal connects through the recorded route when supported; otherwise it shows the existing conversation route and leaves durable submission available.
It never replaces a live owner merely because attachment is unavailable.
Dead-owner reconciliation preserves queued requests and independent children.
State recovery does not promise restoration of a provider transcript that the provider cannot resume.

## Terminal workspace

Bare `multplx` opens the terminal workspace, while `multplx workspace --plain` provides a noninteractive view.
The workspace displays projects, cross-repository tasks, pending decisions, current context and connection state.
Its task and domain facts come from the same canonical projection as CLI status and [MX Viz](viz.md).
Search and selection are presentation state; the terminal interface has no separate model, task store or workflow engine.
Closing it leaves independent work running.
Use `multplx chat codex` to go straight to the orchestrator conversation without opening the dashboard.

Use `Tab` to switch between projects, tasks, decisions and domains.
Arrow keys or `j`/`k` move within a view, `/` searches projects, `t` enters task text for a selected project, `r` refreshes discovery and task state, `c` enters chat, `v` opens MX Viz and `q` leaves.
The mouse wheel moves through the current list; `Ctrl+C` also exits while editing text.
The screen adapts to terminal size and preserves selection as cached discovery results change.
Interactive rendering uses [Ratatui](https://ratatui.rs/) with Crossterm for terminal events.
It uses the terminal's alternate screen, keeping dashboard redraws out of normal shell scrollback, and restores the shell screen when leaving the workspace.
Task observation age stays visible between explicit refreshes.
Discovery progress and unavailable observations remain visible while known projects and chat entry stay usable.
`multplx shell` retains explicit shell activation, including the existing named-harness shims.

## Scoped coordinators

Project selection never creates a coordinator.
Use the explicit [scoped coordinator](scoped-coordinators.md) commands when a bounded repository or idea benefits from delegation.
The global launcher exposes the existing command owners without selecting a different home:

```sh
multplx spawn --help
multplx domain inspect app-coordinator
multplx task --project my-app --domain app-coordinator --request-id login-2 "Investigate login failures"
```
The same main chat can coordinate direct tasks alongside domain work and inspect child outcomes through the shared projection.
The existing provider and persistent-home support limits still apply.
