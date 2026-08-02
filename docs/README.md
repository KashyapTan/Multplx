# Multplx documentation

This index is the human entry point for Multplx documentation.
Choose the path that matches what you are trying to do; agent operating contracts remain linked from their human-facing owners rather than duplicated here.

[Start with the product overview](../README.md) or continue directly to [Getting Started](getting-started.md).

## Start here

- [Getting Started](getting-started.md) takes a new operator from clone to a safe first broker request.
- [Architecture](architecture.md) explains the maintainer, broker, actor, daemon, worktree, supervision, and delivery model.
- [Configuration](configuration.md) owns `MX_HOME`, local settings, harness selection, dispatch profiles, capacity, and the universal toolchain.
- [Delivery](delivery.md) explains why agents stop at local commits and how an approved exact SHA reaches GitHub.

## Operate Multplx

- [System doctor](doctor.md) lists health checks, severities, and the two proof-bound repairs.
- [Live system dashboard](viz.md) covers the disposable read-only `mx-viz` view.
- [vplan review artifacts](vplan.md) covers one-shot annotated HTML reviews.
- [Workflow definitions and runs](workflows.md) owns the declarative workflow schema and lifecycle.
- [Pi Calm mode](calm.md) describes the optional Pi-only presentation toggle.
- [The `bin/` toolbelt](scripts.md) is a concise script index; each script header and `--help` own exact mechanics.
- [Task journal events](journal-events.md) defines the best-effort event vocabulary used by `mx-timeline`.

## Runtime backends

- [tmux](tmux-backend.md) is the verified reference backend and the baseline for daemon homes.
- [Herdr](herdr-backend.md) is an experimental agent-native backend with native state and push events.
- [cmux](cmux-backend.md) is an experimental macOS GUI backend.
- [Codex App](codex-app-backend.md) is not a selectable runtime backend; its page records the missing bridge and acceptance boundary.

## Safety and supervision

- [Native session-start nudge](sessionstart-nudge.md), [turn-end guard](turnend-guard.md), and [watcher continuity](watcher-continuity.md) explain the primary supervision chain.
- [Watcher arm guard](arm-pretool-check.md), [cd guard](cd-guard.md), and [delegation guard](subagent-guard.md) document the primary-session safety seatbelts.
- [Decision hold lifecycle](decision-hold-lifecycle.md) explains how unresolved maintainer decisions survive teardown.
- [Guard verification](verification/guards.md) holds current cross-harness empirical proof for those safety mechanisms.
- [Supervision verification](verification/supervision.md) and [runtime backend verification](verification/runtime-backends.md) hold the other active version-scoped evidence.

## Contribute and maintain

- [Contributing](../CONTRIBUTING.md) covers the contribution workflow, repository conventions, and test entry points.
- [Documentation audiences](documentation-audiences.md) owns placement policy and the machine-consumed maintained-surface inventory.
- [Upstream review](upstream.md) owns the fork point, relevance map, review cursor, and review-and-reimplement process.
- [Test performance](mx-test-performance.md), [test isolation proof](mx-test-isolation-proof.md), [portable shards](mx-test-portable-shards.md), and [Pi Calm feasibility](calm-mode-feasibility.md) are maintained verification records.

## Reading paths

| Reader | Suggested path |
| --- | --- |
| New operator | [Getting Started](getting-started.md) -> [Configuration](configuration.md) -> one backend guide -> [Delivery](delivery.md) |
| Day-to-day operator | [Doctor](doctor.md) -> [mx-viz](viz.md) -> [vplan](vplan.md) or [Workflows](workflows.md) |
| Contributor | [Contributing](../CONTRIBUTING.md) -> [Architecture](architecture.md) -> the relevant mechanism page |
| Maintainer validating a guarantee | The relevant page under [guard](verification/guards.md), [supervision](verification/supervision.md), backend, or test verification |

Maintained prose is audience-classified in [`documentation-audiences.json`](documentation-audiences.json).
