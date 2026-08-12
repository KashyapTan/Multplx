# Contributing

Thanks for wanting to contribute.
Multplx owns its full local validation gate in this repository.
It does not require an external git proxy or a signed pull-request body.
The [documentation index](docs/README.md) is the entry point for current operator, architecture, safety, and verification material.

## Workflow

1. Fork the repo, then clone the parent repo or set your local `origin` back to the parent (`git@github.com:KashyapTan/Multplx.git`).
2. Create a branch and make your changes.
3. Run focused tests for the behavior you changed, then run the complete behavior suite before opening a PR.
4. Commit your changes, push the branch to your fork, and open the PR against `main`.

Multplx-managed delivery tasks use a stricter automated path.
The actor runs `bin/mx-deep-review.sh <task-id> --intent-file <brief>` from its assigned `mx/<task-id>` worktree.
That intent-targeted gate performs rebase, review, focused test, documentation, and lint locally, then writes a pending exact-SHA handoff without pushing.
Only the separately approved, credentialed delivery service may consume that handoff, push its exact SHA, and open the PR.

The Rust port is the deliberate exception to using Multplx to develop Multplx.
Open a fresh ordinary coding-agent session directly in this checkout, not through the global `multplx` launcher, and do not run session start, dispatch actors or daemons, or use Multplx supervision for port work.
Treat `AGENTS-PORTING.md` as dormant product source rather than active agent instructions.
Close any session that already loaded the former root `AGENTS.md` before continuing the port.

## Repo conventions

- This repo is a template for running the Multplx multi-agent orchestrator.
  During the Rust port, `AGENTS-PORTING.md` is the root broker-contract editing target and is intentionally not auto-loaded; `CLAUDE.md` contains contributor context, and `.claude/skills` is a symlink to `.agents/skills`.
- Only shared material is tracked: `AGENTS-PORTING.md`, `README.md`, `CONTRIBUTING.md`, `.github/workflows/`, `bin/`, `.agents/skills/`, and `skills/`.
  `.agents/skills/` holds agent-loaded skills that assume a live Multplx home and carry `metadata.internal: true` so installers such as [skills.sh](https://skills.sh) hide them from discovery; `skills/` holds standalone, installer-facing public skills with no Multplx dependency.
  Everything personal to one maintainer's system (`.env`, `data/`, `state/`, `config/`, `projects/`) is gitignored; never commit it.
  The in-repo backlog library owns `data/backlog.md`, its parser, retention defaults, and routine mutations as documented in [`docs/configuration.md`](docs/configuration.md) ("Backlog backend").
  A local `config/backlog-backend=manual` opt-out forces the broker's routine backlog updates to hand-editing and stays gitignored; validated daemon handoffs still route through the owned atomic move.
  A local `config/backend` file explicitly overrides runtime auto-detection for new task endpoints and stays gitignored; spawn-supported values are `tmux` plus experimental `herdr` and `cmux`, while `codex-app` is documented only in `docs/codex-app-backend.md`.
  It does not make `data/` tracked.
- Public helper entry points in `bin/` remain plain Bash adapters during the Rust port.
  Each starts with a usage header comment; keep it accurate when you change behavior.
  The Rust workspace builds one `mx` multicall binary, and the transferred backend, harness, headroom, and Treehouse operations select it by default after Portion 06.
  Hidden compatibility commands preserve exact legacy-versus-Rust verification without changing public callers.
  Test scripts and helpers in `tests/` remain plain Bash, with Rust-native fixtures and differential self-tests in the Rust workspace.
- Changes to harness adapters (detection in the Rust harness layer, launch and hook mechanics in the Rust lifecycle layer, busy signatures in `bin/mx-watch.sh` and the Rust backend facade, cleanup in the Rust teardown lifecycle, and facts in `.agents/skills/harness-adapters/SKILL.md`) must be verified empirically against the real harness, never written from documentation alone.
- Changes to runtime session backends (`bin/mx-backend.sh`, `bin/backends/`, and the scripts that dispatch through them) keep current setup and limits in the relevant backend guide and active empirical evidence in [`docs/verification/runtime-backends.md`](docs/verification/runtime-backends.md).
- [`docs/documentation-audiences.md`](docs/documentation-audiences.md) and its machine-consumed inventory own prose classification; run `bin/mx-doc-audience-check.sh` after documentation changes.
- In Markdown, put each full sentence on its own line.
- `README.md` stays a concise overview plus pointers: it never carries a wall of inline detail.
  Route detail to the most specific `docs/` file (architecture, configuration, or a backend guide) and link to it instead.

## Development

Tracked changes to Multplx itself - `AGENTS-PORTING.md`, `README.md`, `CONTRIBUTING.md`, `.github/workflows/`, `bin/`, `.agents/skills/`, and `skills/` - run through the selected port-development workflow.
Before making any such change, load the agent-only `multplx-coding-guidelines` skill (`.agents/skills/multplx-coding-guidelines/SKILL.md`).
It has the knowledge-placement rules that keep the root broker contract from regrowing after each diet pass.
Every root contract edit during the port belongs in `AGENTS-PORTING.md`; do not create root `AGENTS.md` or update hooks and active-home detection to accept the temporary name.
This does not alter managed projects' own `AGENTS.md` files.
Port portion 13 restores `AGENTS-PORTING.md` to `AGENTS.md` only after all portions and the full Rust-default validation, documentation, packaging, and release gates pass.
There is no reliable way for `bin/mx-brief.sh`'s scaffold to detect that a task's repo is Multplx itself, so the broker adds this skill's load line to Multplx-repo briefs by hand.
An actor picking up such a brief should load the skill even if the brief predates this instruction.
When monitoring live actors, keep the broker's own long validation or build commands in the background so watcher wakes can still be handled.
Multplx actors stop at the local deep-review handoff and never push a branch or open a PR.
The gate routes every `ask-user` finding through the validated status path under the authority contract currently maintained in `AGENTS-PORTING.md`.
Its private restart-safe evidence lives under `state/<task-id>.gate/`, outside project commits.
The local gate's test step is intent-targeted and must not re-run every `tests/*.test.sh`; `.github/workflows/ci.yml` owns the broad behavior suite plus platform-specific compatibility lanes.

Check and test the toolbelt before pushing:

```sh
for script in bin/*.sh bin/backends/*.sh; do bash -n "$script"; done   # syntax-check the toolbelt
bin/mx-test-run.sh tests/<subject>.test.sh   # one script (primary local focus path, timed)
bin/mx-test-run.sh --family pure-contract-unit   # ordinary family-scoped local path (serial, timed)
bin/mx-test-run.sh --changed   # conservative changed-file-informed set (never silent full suite)
bin/mx-test-run.sh --all --jobs auto   # accelerated complete regression (also the --all default)
bin/mx-test-run.sh --all --jobs 1   # byte-comparable serial reference and debugging path
bin/mx-test-run.sh --list-resources --all   # audited resource declarations
bin/mx-test-run.sh --check-coverage   # prove manifest and CI partitions equal the full inventory
bin/mx-test-isolation-proof.sh --list-conflicts   # inspect the derived conflict matrix
bin/mx-test-isolation-proof.sh --jobs 4 --repeats 2 --json /tmp/mx-isolation-proof.json
bin/mx-test-run.sh --compare-json /tmp/serial.json /tmp/accelerated.json
[ "$(readlink .claude/skills)" = "../.agents/skills" ]
tmp=$(mktemp -d) && printf 'done: smoke\n' > "$tmp/smoke.status" && MX_STATE_OVERRIDE="$tmp" MX_SIGNAL_GRACE=1 MX_POLL=1 MX_HEARTBEAT=999999 bin/mx-watch-arm.sh  # watcher re-arm smoke test (prints arm status, then an actionable signal)
```

Check the Rust workspace independently:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo build --release --workspace --locked
target/release/mx shadow-diagnostic
```

`tests/lib.sh` owns the test-only `MX_TEST_IMPLEMENTATION=legacy|rust` selector and the differential capture helpers.
The test-only local-state selector remains legacy by default, while production backend, harness, headroom, lifecycle, supervision, session-start, health, snapshot, system-view, and timeline entry points default to Rust and retain explicit legacy selectors for differential coverage.
Differential capture compares exit status, standard output, standard error, relative filesystem paths, exact file bytes, file modes, and surviving child processes.
Only temporary roots, PIDs, ports, timestamps, and random tokens may be normalized by a focused test, and no normalization is automatic.
The hidden `shadow-diagnostic` command verifies the Portion 01 binary and crate graph only, while hidden compatibility subcommands exercise transferred contracts for differential tests.
Neither surface is an operator command or a production cutover.

`bin/mx-test-run.sh` is the single owner of behavior-suite selection, the resource-conflict manifest, resource-aware scheduling, generated portable CI lanes, timing markers, family totals, the coverage guard, assertion parity, and JSON timing artifacts.
Its header and `--help` own the flags, family labels, lanes, and changed-file map; this section only documents the entry points.
`bin/mx-test-isolation-proof.sh` consumes the runner manifest and owns repeated conflict-matrix and leak proof; see `docs/mx-test-isolation-proof.md`.
Portable shard balance evidence lives in `docs/mx-test-portable-shards.md`.
The performance baseline, current accepted proof, and local/CI targets live in `docs/mx-test-performance.md`.
The deep-review test step stays intent-targeted and must not wire `.deep-review.yaml` `commands.test` to `--all` or a `tests/*.test.sh` walk.
Family selection is the ordinary focused path; `--all` is the accelerated complete regression.
Use `--jobs 1` whenever a failure needs serial reproduction.
CI owns broad regression across required portable parallel shards, the portable serial lane, the Herdr lane, invariants, the coverage guard, and macOS snapshot compatibility in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).
Use `bin/mx-test-run.sh --help` for lane names, resource-aware `--jobs` rules, and required gate-skip flags when reproducing a lane locally.
Discover tests by listing `tests/*.test.sh`: each is a self-contained bash script named `<subject>.test.sh`, and its header comment describes what it covers, so pass one to `bin/mx-test-run.sh` to focus on a subject with canonical timing output.
Tests that need a real optional backend or an explicit opt-in (real herdr/cmux smoke tests, the live Pi regression) skip themselves and print the tool or environment gate needed to enable them, so the portable suite remains safe on machines without those tools.
Timeout increases, reduced fault matrices, retries, and new skips are not test-performance fixes.
The [Herdr backend guide](docs/herdr-backend.md#destructive-lab-safety) owns the lane's isolation boundary, while [runtime backend verification](docs/verification/runtime-backends.md#herdr) owns active empirical evidence; live harness credential tests remain opt-in.

## Questions

Open a [GitHub issue](https://github.com/KashyapTan/Multplx/issues) for a bug, question, or concrete proposal.
