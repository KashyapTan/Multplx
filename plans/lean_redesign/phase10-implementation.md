# Phase 10 implementation evidence

## Status and source boundary

Status: complete; assigned implementation and acceptance checks passed on 2026-09-17 Eastern time, with automated timestamps recorded on 2026-09-18 UTC.
Work started on branch `lean-redesign-phase10` from merged Phase 09 commit `fa60d1c`.
The [phase plan](10-mx-viz-and-scale.html) allocates this work and [porting.md](../../porting.md#accepted-architecture-contract) owns the shared contracts.
Inspection covered the implemented task/attempt/brief model, project identities, allocation observations, durable coordination, domain projections, revision-bound delivery and workflow records, and migration evidence before editing.
Root `AGENTS.md` remains absent and `AGENTS_E.md` remains dormant.
The excluded `firstmate/` directory was not an implementation input.
During browser validation, a sub-agent accidentally invoked the default checkout snapshot and exposed private backlog metadata in a transient view.
No operational state was mutated, the temporary snapshot was discarded, and that check is excluded from acceptance evidence.
Retained validation evidence uses isolated fixture homes.

## Implementation and acceptance ledger

| Assigned requirement | Implemented surface and verification scope |
| --- | --- |
| Shared task projection | `mx-portfolio.v1` inside the compatible system snapshot carries home-qualified identity, project/checkout binding, ownership, roles, attempts, native observations, brief/research pointers, workflows, dependencies, decisions, delivery and allocation facts. |
| Shared human summaries | Typed system view and bounded JSON/text catchup summaries consume the canonical portfolio rather than introducing dashboard-owned task state. |
| Truthful counts and partial homes | Useful tasks, coordinators, sessions and attempts are distinct; accepted backlog items without execution metadata and unavailable child homes remain visible. |
| Human attention and evidence | The dashboard exposes revision-bound decisions, dependency blockers and the existing delivery owner's review queue, with current evidence separated from history. |
| Bounded observation | Task observation concurrency and subprocess deadlines preserve partial results; process-group cleanup and bounded pipe draining prevent descendants from defeating collection deadlines. |
| Shared service cache | One refresh runs outside the runtime mutex, while concurrent readers receive cached state and explicit age, refresh state and errors. |
| Stable interaction | Search and project/state/role/priority filters, home-qualified links, expandable details, keyboard navigation and focus survive updates; polling does not overlap and hidden tabs back off. |
| Resource and domain visibility | Actual allocation bindings and observations expose retained work, leases and recovery uncertainty; domain views expose lineage and undelivered outcomes. |
| Read-only boundary | GET-only loopback service, contained artifact paths, scriptless rendering and identity-bound lifecycle controls remain covered by service tests. |

The projection extends the existing filesystem owners instead of adding dashboard-owned task state.
Both JSON and TOON catchup forms carry the same bounded portfolio summary; the full system snapshot and typed system view retain the canonical detail for Phase 11 clients.
Native provider child identifiers deduplicate session observations across events, while unavailable identifiers remain unknown.
Decisions obtain waiting time only from matching accepted, revision-bound durable evidence; older records without that evidence retain an unknown timestamp.
Nested coordinator homes pass the existing home validator before their state or artifact roots are exposed.
Archived briefs, original research and reports receive exact cached artifact-reference routes within their validated owning home; unknown homes, cross-owner paths and escaping symlinks remain refused.

Collection defaults to four task observers, bounded between one and sixteen, with per-task subprocess deadlines and bounded output.
Child-home reads use bounded concurrency, per-home and overall time budgets, and disclose omitted or unavailable results.
Subprocess groups and bounded pipe draining prevent a descendant holding stdout open from defeating a deadline.
The UI retains the existing read-only service and artifact viewer and adds no push, merge, approval or execution endpoint.

The combined-tree checks below verify the final implementation; the later shell portability correction was also rerun on both platforms.

## Measurement boundary

Phase 10 owns dashboard collection, cache and browser acceptance, including concurrent viewers and a stalled observation source.
The shared ownership table assigns combined live-model 1/5/10/20-task velocity, quality and hierarchical workload trials to Phase 12.
Deterministic backend fixtures and real loopback HTTP or browser execution will be identified separately from live model-provider evidence.

## Viz service cache and bounded refresh

The Rust loopback service now permits one snapshot refresh across every viewer without holding its runtime mutex during the external collector command.
An expired last-good snapshot is served immediately while a background refresh runs and remains available after refresh failure.
Initial unavailability, fresh and stale cache service, refresh state, observation age, raw snapshot identity and meaningful content identity are explicit response facts.
The meaningful ETag excludes collection timestamps and freshness ages while preserving identified event, observation, decision and delivery timestamps.
Snapshot, doctor and timeline readers use the configurable `MX_VIZ_COMMAND_TIMEOUT_MS` deadline, which defaults to 10 seconds, and failed initial refreshes respect the cache retry interval.
The outer deadline allows the collector's shorter provider deadlines to publish healthy partial results instead of racing them; the healthy-refresh acceptance target remains below 2 seconds.
The existing GET-only loopback surface, canonical artifact containment, scriptless rendering policy, read-only operational boundary, singleton lifecycle and identity-bound shutdown remain covered.

The reproducible [service measurement](phase10-viz-service-performance.json) ran on macOS 26.6.2 arm64, Apple M5 Pro, 15 logical CPUs and 24 GiB RAM, after repository validation load finished.
The archive identifies the exact source, release binary and measurement script by SHA-256 and retains every sample, cache counter and short-run CPU observation.
There were zero sampled-request errors and zero transient condition-poll errors in the final run.

| Workload | Measured result | Accepted target or interpretation |
| --- | --- | --- |
| Deterministic 1/5/10/20-task service fixtures | 50 cached conditional samples with eight callers and ten healthy refresh samples per scale. At 20 tasks, cached API p95 16.240 ms and refresh p95 43 ms. | Cached API below 250 ms and healthy 20-task refresh below 2,000 ms: passed. |
| Shipping filesystem collector, 20 task records | Initial refresh 338.740 ms, cached API p95 15.708 ms and healthy refresh p95 177 ms. | Passed; zero invented sessions, no live provider endpoints. |
| Whole-reader failure with twelve concurrent clients | Cached API p95 14.657 ms; one deliberately stalled refresh failed at its configured 2,000 ms deadline in 2,003 ms. | Last-good state remained available; this is a synthetic collector failure. |
| Shipping collector with one synthetic stalled actor and a healthy sibling update | Twelve viewers received cached responses at p95 10.503 ms; the partial refresh took 2,073 ms and exposed the exact healthy event in 2,226.565 ms. | The stalled task remained an explicit timeout; exactly two probes and refresh attempts total covered initial collection plus one shared refresh, with no failures. |
| Pending wake during the stalled collection | One real isolated pending wake, oldest age two seconds when the event became visible; service CPU delta 0.05 seconds. | Queue-age observation only; no consumer, durable-disposition or live-model throughput claim. |
| Historical serial versus current bounded task collection | Same 20-record home and synthetic 50 ms actor cost, ten samples each: serial p50/p95 1,410.319/1,502.628 ms, bounded p50/p95 388.662/535.570 ms. Child CPU p50 184.206 versus 134.802 ms. | Measures the actual pre-phase serial collector against the current four-worker collector. |
| Historical service mutex during a synchronized 500 ms reader stall | State API p95 546.742 ms, metadata 507.288 ms and batch 549.117 ms; observed reader stall 506.893 ms. | Confirms the old mutex blocked unrelated readers while the new service serves cached state. |

The stalled-actor check explicitly unsets deadline and concurrency overrides, using the shipping 10-second outer reader deadline, 2-second actor deadline and four task observers.
It asserts both the timed-out actor's canonical source and the healthy sibling's exact canonical event, rather than accepting an arbitrary occurrence of the event text elsewhere in the response.
The reference collector workload has no live provider sessions, so these results establish local collection and dashboard behavior rather than model execution quality or end-to-end coordination throughput.
CPU observations retain operating-system reporting granularity and distinguish service CPU from collector child CPU.

The historical source is merged Phase 09 commit `fa60d1ca46c3834f4b582adf7a5c7c7fb3dbabc3` in an isolated temporary checkout.
An initial reused baseline target contained newer service code and produced an impossible low-latency mutex result; those samples were discarded.
A fresh target rebuilt the historical binary, verified the absence of Phase 10 cache metrics, and produced SHA-256 `c6d1b1704fad0589df51fa078f9ff543c76eefa476e75a21eb868f9f3ec79e0a`.
The benchmark now rejects that provenance mismatch or a triggering request that does not span the injected stall.

Exact baseline build and final measurement commands:

```sh
baseline_target=$(mktemp -d /tmp/mx-viz-baseline-clean-target.XXXXXX)
printf '%s\n' "$baseline_target" >/tmp/mx-viz-baseline-clean-target.path
CARGO_TARGET_DIR="$baseline_target" cargo build --release --manifest-path /tmp/mx-viz-baseline.HHt1vR/Cargo.toml -p multplx-cli
baseline_binary="$(cat /tmp/mx-viz-baseline-clean-target.path)/release/mx"
python3 -m py_compile tests/fixtures/measure-viz-service.py
python3 tests/fixtures/measure-viz-service.py --baseline-root /tmp/mx-viz-baseline.HHt1vR --baseline-binary "$baseline_binary" --output plans/lean_redesign/phase10-viz-service-performance.json >/tmp/mx-viz-measure-final.json
```

## Repository validation

The reference macOS arm64 environment uses Rust 1.97.1, Node 24.14.1, Git 2.55.0 and tmux 3.7c.
The isolated Linux arm64 container uses Rust 1.97.1, Node 20.19.2, Git 2.47.3 and tmux 3.5a as ordinary user `mxvalidate` with `LANG=C.UTF-8`.
Its disposable source copy excludes private operational state, `firstmate/`, the host Git metadata and host build tree.
A temporary local Git repository enabled the fixture runner; validation logs were retained outside the checkout and the container was removed after checks.

| Exact check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed on macOS and Linux. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed on macOS and Linux. |
| `cargo test --locked --workspace` | Passed across the complete workspace on macOS and Linux. |
| `cargo build --release --workspace --locked` | Passed on macOS and Linux. |
| `target/release/mx test-run --check-coverage` | Passed: 130 fixtures, comprising 109 accelerated, 11 serial and ten Herdr fixtures. |
| `target/release/mx test-run --all --jobs auto` | Passed all 130 macOS fixtures: zero failures, eight declared environment skips, 386,419 ms. |
| `target/release/mx test-run --family snapshot-catchup` | All four suites passed on Linux in 12,765 ms; the same family passed in the macOS aggregate. |
| `target/release/mx test-run tests/mx-viz.test.sh` | Seven scenarios passed on macOS (5,638 ms) and Linux (5,075 ms), including the real root/direct/nested collector-to-artifact route. |
| `target/release/mx doc-audience-check` | Passed after final evidence publication: 88 maintained surfaces and 456 local links. |
| `target/release/mx shadow-diagnostic` | Ready. |
| `git diff --check` | Passed after final evidence publication. |

The unchanged source-line coverage gate is run with a separate temporary target directory:

```sh
CARGO_TARGET_DIR=/tmp/mx-phase10-coverage-target cargo llvm-cov --locked --workspace --all-targets --ignore-filename-regex '(multplx-cli/src/(authority|deep_review|launcher|review|supervision|workflow_runtime)\.rs|multplx-cli/src/tooling/(documentation|runner)\.rs|multplx-domain/src/lifecycle/(home_seed|upstream_diff)\.rs|herdr_(cleanup|presentation|tools)\.rs)' --fail-under-lines 93
```

Final-source coverage passed at 93.09%: 4,661 missed of 67,462 measured lines.
The inventory check verifies fixture classification; it is separate from the 93% source-line gate.
Earlier validation found and corrected JSON/TOON portfolio parity, a naming-fixture token, split-assertion bookkeeping, refresh-backoff expectations, a Clippy lint, nested artifact linking and the outer reader deadline.
Two earlier vplan runtime tests failed transiently under load and passed their exact reruns and subsequent workspace runs without production changes.
A later Linux inherited-pointer test failed once under concurrent validation load and passed its exact retry and the complete workspace rerun.
The Linux copy initially needed a writable parent directory, UTF-8 locale and a rebuild of test-support after its path moved; these were validation-environment corrections.
An instrumented run was interrupted after source changes so it would not be represented as final-source evidence.
The Linux dashboard run exposed a GNU/BSD grep difference in a response-header assertion; the portable check strips only the HTTP framing CR and still rejects embedded controls, then passed on both platforms.
No acceptance threshold or coverage exclusion was relaxed.

## Browser acceptance

The persisted [browser evidence](phase10-ui-browser-results.json), `tests/viz-browser-check.mjs`, and the isolated fixture setup in [docs/viz.md](../../docs/viz.md) make the checks reproducible without a packaged browser dependency.
Chromium 153.0.8010.12 with temporary Playwright 1.63.0 exercised 0, 1, 5, 10 and 20 useful tasks at 1440 by 900 and 390 by 844.
Every scale rendered without horizontal overflow; wide views used two columns and narrow views one column.
Screenshots were inspected for task hierarchy, duplicate project names with distinct checkout paths, long titles, textual role labels, decisions, allocation retention and unavailable coordinators.
Project, role, state and priority filters matched exact canonical task keys; clearing filters preserved checkout and evidence bindings, including duplicate project names and repository paths at three different nesting depths.
A meaningful update preserved search, keyboard focus, expanded detail and the home-qualified deep link.
Keyboard movement reached the next task, delayed HTTP responses produced at most one simultaneous client poll, and injected `document.hidden` scheduled a 15,000 ms backoff.
That visibility check exercises the handler deterministically; it is not evidence of a physical background tab's browser throttling policy.
Conditional 304 responses advanced observation age, and a failed cached refresh retained the overview with an explicit error banner.
An actual archived brief opened through its exact artifact-reference route in the scriptless viewer.
Twenty warm-cache post-data interaction samples measured p50 15.873 ms, p95 19.555 ms and maximum 19.591 ms against the 2,000 ms target, with no console or page errors.
A separate isolated shipping collector rendered 20 canonical records and zero invented provider sessions.
These checks run the real Rust loopback service and browser, using deterministic state fixtures rather than live model providers.

## Limitations and remaining work

No assigned Phase 10 implementation or acceptance item remains unresolved.
The eight aggregate skips are declared environment gates for cmux smoke, Claude stop auto-arm, Codex continuity, Cursor, launcher live entry, Pi live entry/types and the optional Herdr marker test; they are not live-provider passes.
The new browser and service trials use isolated local homes, real filesystem collection, loopback HTTP and Chromium, with synthetic provider delays and deterministic task fixtures.
They do not claim live model throughput, human decision or review latency, review quality, merge behavior, or a physical hidden-tab throttling trial.
Unknown native children remain uncounted as sessions, missing historical decision timestamps remain unknown, and external or unvalidated artifact pointers stay visible without receiving file-serving authority.
Linux covered full Rust checks and the changed snapshot/dashboard shell surfaces; the full cross-platform combined release matrix and live-model 1/5/10/20-task trials remain Phase 12 work.
Phase 11 is ready to begin using the shared project/task portfolio for workspace entry and its terminal UI.
Release activation remains planned; root `AGENTS.md` has not been restored.
