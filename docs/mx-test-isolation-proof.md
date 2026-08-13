# Multplx resource isolation proof

This document is the human-readable owner for the resource-scheduler proof archived in `docs/mx-test-isolation-proof.json`.
The Rust `test-run` command owns the resource manifest and production scheduling.
The Rust `test-isolation-proof` command consumes that manifest and owns conflict-matrix inspection, repeated portable stress rounds, and leak checks.

## Proof contract

The proof checks these guarantees:

- Every portable candidate runs in a mode-`0700` worker root with a private `TMPDIR`.
- Ambient Multplx home and override variables are cleared for each worker.
- Disjoint resources may overlap up to the worker cap.
- Shared resources never overlap.
- `global` overlaps nothing.
- Unknown ad-hoc scripts fail closed to `global`.
- A worker failure remains visible in the aggregate result.
- No retry converts a failure into green.
- Global git configuration is byte-identical before and after.
- No process referring to the proof-owned root survives completion.

Real Herdr, cmux, live harnesses, the load-sensitive PR publication race, and the global runner/proof self-contracts remain explicit exclusions from portable stress.
The dedicated backend and harness lanes retain their real environment coverage.
The Plan-06 feature-branch merge-base conformance exit is read from the committed performance baseline and counted separately when it appears.
No other nonzero script exit is accepted.

## Inspect and reproduce

```sh
target/release/mx test-run --list-resources --all
target/release/mx test-isolation-proof --list-conflicts
target/release/mx test-isolation-proof --list-exclusions
target/release/mx test-isolation-proof --jobs 4 --repeats 2 --json /tmp/mx-isolation-proof.json
bash tests/mx-test-isolation-proof.test.sh
```

The conflict matrix is derived from the runner manifest at execution time.
It is not copied into this document.

## Archived proof

The committed JSON records the exact manifest hash, resource declarations, conflict-pair count, concurrency, repeat count, per-round result, per-script timing, failure count, and leak count.
The contract test rejects the archive if its manifest hash differs from the current runner output.
Any resource declaration change requires a fresh proof archive.

The current accepted proof date, duration, and command are also recorded in [mx-test-performance.md](mx-test-performance.md).
