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

## Repo conventions

- This repo is a template for running the Multplx multi-agent orchestrator.
  `AGENTS.md` is the root broker contract, `CLAUDE.md` contains contributor context, and `.claude/skills` is a symlink to `.agents/skills`.
- Only shared material is tracked: `AGENTS.md`, `README.md`, `CONTRIBUTING.md`, `.github/workflows/`, `bin/`, `.agents/skills/`, and `skills/`.
  `.agents/skills/` holds agent-loaded skills that assume a live Multplx home and carry `metadata.internal: true` so installers such as [skills.sh](https://skills.sh) hide them from discovery; `skills/` holds standalone, installer-facing public skills with no Multplx dependency.
  Everything personal to one maintainer's system (`.env`, `data/`, `state/`, `config/`, `projects/`) is gitignored; never commit it.
  The in-repo backlog library owns `data/backlog.md`, its parser, retention defaults, and routine mutations as documented in [`docs/configuration.md`](docs/configuration.md) ("Backlog backend").
  A local `config/backlog-backend=manual` opt-out forces the broker's routine backlog updates to hand-editing and stays gitignored; validated daemon handoffs still route through the owned atomic move.
  A local `config/backend` file explicitly overrides runtime auto-detection for new task endpoints and stays gitignored; spawn-supported values are `tmux` plus experimental `herdr` and `cmux`, while `codex-app` is documented only in `docs/codex-app-backend.md`.
  It does not make `data/` tracked.
- Public helper entry points in `bin/` are compatibility transports to the release Rust binary where a script pathname is part of the public contract.
  Each starts with a usage header comment; keep it accurate when you change behavior.
  The Rust workspace builds one `mx` multicall binary, and transferred backend, harness, lifecycle, supervision, session, authority, and workflow operations select it by default after their numbered portion.
  Test scripts and helpers in `tests/` remain plain Bash, with Rust-native fixtures and self-tests in the Rust workspace.
- Changes to harness adapters (detection in the Rust harness layer, launch and hook mechanics in the Rust lifecycle layer, busy signatures in `bin/mx-watch.sh` and the Rust backend facade, cleanup in the Rust teardown lifecycle, and facts in `.agents/skills/harness-adapters/SKILL.md`) must be verified empirically against the real harness, never written from documentation alone.
- Changes to runtime session backends (`bin/mx-backend.sh`, `bin/backends/`, and the scripts that dispatch through them) keep current setup and limits in the relevant backend guide and active empirical evidence in [`docs/verification/runtime-backends.md`](docs/verification/runtime-backends.md).
- [`docs/documentation-audiences.md`](docs/documentation-audiences.md) and its machine-consumed inventory own prose classification; run `bin/mx-doc-audience-check.sh` after documentation changes.
- In Markdown, put each full sentence on its own line.
- `README.md` stays a concise overview plus pointers: it never carries a wall of inline detail.
  Route detail to the most specific `docs/` file (architecture, configuration, or a backend guide) and link to it instead.

## Development

Tracked changes to Multplx itself - `AGENTS.md`, `README.md`, `CONTRIBUTING.md`, `.github/workflows/`, `bin/`, `.agents/skills/`, and `skills/` - run through the selected development workflow.
Before making any such change, load the agent-only `multplx-coding-guidelines` skill (`.agents/skills/multplx-coding-guidelines/SKILL.md`).
It has the knowledge-placement rules that keep the root broker contract from regrowing after each diet pass.
There is no reliable way for `bin/mx-brief.sh`'s scaffold to detect that a task's repo is Multplx itself, so the broker adds this skill's load line to Multplx-repo briefs by hand.
An actor picking up such a brief should load the skill even if the brief predates this instruction.
When monitoring live actors, keep the broker's own long validation or build commands in the background so watcher wakes can still be handled.
Multplx actors stop at the local deep-review handoff and never push a branch or open a PR.
The gate routes every `ask-user` finding through the validated status path under the authority contract maintained in `AGENTS.md`.
Its private restart-safe evidence lives under `state/<task-id>.gate/`, outside project commits.
The local gate's test step is intent-targeted and must not re-run every `tests/*.test.sh`; `.github/workflows/ci.yml` owns the broad behavior suite plus platform-specific compatibility lanes.

Check and test the toolbelt before pushing:

```sh
for script in bin/*.sh bin/backends/*.sh; do bash -n "$script"; done   # syntax-check the toolbelt
target/release/mx test-run tests/<subject>.test.sh   # one script (primary local focus path, timed)
target/release/mx test-run --family pure-contract-unit   # ordinary family-scoped local path (serial, timed)
target/release/mx test-run --changed   # conservative changed-file-informed set (never silent full suite)
target/release/mx test-run --all --jobs auto   # accelerated complete regression (also the --all default)
target/release/mx test-run --all --jobs 1   # byte-comparable serial reference and debugging path
target/release/mx test-run --list-resources --all   # audited resource declarations
target/release/mx test-run --check-coverage   # prove manifest and CI partitions equal the full inventory
target/release/mx test-isolation-proof --list-conflicts   # inspect the derived conflict matrix
target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /tmp/mx-isolation-proof.json
target/release/mx test-run --compare-json /tmp/serial.json /tmp/accelerated.json
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

The Rust `test-run` command is the single owner of behavior-suite selection, the resource-conflict manifest, resource-aware scheduling, generated portable CI lanes, timing markers, family totals, the coverage guard, assertion parity, and JSON timing artifacts.
Its `--help` output owns the flags, family labels, lanes, and changed-file map; this section only documents the entry points.
The Rust `test-isolation-proof` command consumes the runner manifest and owns repeated conflict-matrix and leak proof; see `docs/mx-test-isolation-proof.md`.
Portable shard balance evidence lives in `docs/mx-test-portable-shards.md`.
The performance baseline, current accepted proof, and local/CI targets live in `docs/mx-test-performance.md`.
The deep-review test step stays intent-targeted and must not wire `.deep-review.yaml` `commands.test` to `--all` or a `tests/*.test.sh` walk.
Family selection is the ordinary focused path; `--all` is the accelerated complete regression.
Use `--jobs 1` whenever a failure needs serial reproduction.
CI owns broad regression across required portable parallel shards, the portable serial lane, the Herdr lane, invariants, and the coverage guard in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).
Use `target/release/mx test-run --help` for lane names, resource-aware `--jobs` rules, and required gate-skip flags when reproducing a lane locally.
Discover tests by listing `tests/*.test.sh`: each is a self-contained bash script named `<subject>.test.sh`, and its header comment describes what it covers, so pass one to `target/release/mx test-run` to focus on a subject with canonical timing output.
Tests that need a real optional backend or an explicit opt-in (real herdr/cmux smoke tests, the live Pi regression) skip themselves and print the tool or environment gate needed to enable them, so the portable suite remains safe on machines without those tools.
Timeout increases, reduced fault matrices, retries, and new skips are not test-performance fixes.
The [Herdr backend guide](docs/herdr-backend.md#destructive-lab-safety) owns the lane's isolation boundary, while [runtime backend verification](docs/verification/runtime-backends.md#herdr) owns active empirical evidence; live harness credential tests remain opt-in.

## Questions

Open a [GitHub issue](https://github.com/KashyapTan/Multplx/issues) for a bug, question, or concrete proposal.
