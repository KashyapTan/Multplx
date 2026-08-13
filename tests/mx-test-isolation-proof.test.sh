#!/usr/bin/env bash
# Contracts for the runner-owned resource manifest and repeatable isolation proof.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

PROOF="$ROOT/bin/mx-test-isolation-proof.sh"
RUNNER="$ROOT/bin/mx-test-run.sh"
CI="$ROOT/.github/workflows/ci.yml"
CONTRIB="$ROOT/CONTRIBUTING.md"
PROOF_DOC="$ROOT/docs/mx-test-isolation-proof.md"
PROOF_JSON="$ROOT/docs/mx-test-isolation-proof.json"

assert_present "$PROOF" "bin/mx-test-isolation-proof.sh is missing"
[ -x "$PROOF" ] || fail "bin/mx-test-isolation-proof.sh must be executable"

test_proof_consumes_exact_runner_manifest() {
  local proof_manifest runner_manifest
  proof_manifest=$("$PROOF" --list-resources)
  runner_manifest=$("$RUNNER" --list-resources --all)
  [ "$proof_manifest" = "$runner_manifest" ] \
    || fail "isolation proof duplicated or diverged from the runner resource manifest"
  [ "$(printf '%s\n' "$proof_manifest" | wc -l | tr -d ' ')" = \
    "$("$RUNNER" --list --all | wc -l | tr -d ' ')" ] \
    || fail "resource manifest does not cover the exact inventory"
  pass "isolation proof consumes the exact runner-owned manifest"
}

test_candidate_and_exclusion_partition() {
  local candidates exclusions all union
  candidates=$("$PROOF" --list)
  exclusions=$("$PROOF" --list-exclusions | cut -f1)
  all=$("$RUNNER" --list --all | LC_ALL=C sort)
  union=$(printf '%s\n' "$candidates" "$exclusions" | LC_ALL=C sort)
  [ "$union" = "$all" ] || fail "proof candidates plus explicit exclusions must equal inventory"
  printf '%s\n' "$candidates" | grep -Fq 'mx-backend-herdr-presentation-e2e' \
    && fail "real Herdr must remain outside portable stress rounds"
  printf '%s\n' "$candidates" | grep -Fq 'mx-test-run.test.sh' \
    && fail "global runner self-contract must not recurse into the proof"
  printf '%s\n' "$exclusions" | grep -Fq 'tests/mx-test-run.test.sh' \
    || fail "global runner exclusion is not documented"
  pass "proof candidates and resource exclusions partition the inventory"
}

test_conflict_matrix_is_complete_for_shared_resources() {
  local conflicts
  conflicts=$("$PROOF" --list-conflicts)
  [ -n "$conflicts" ] || fail "conflict matrix is empty"
  printf '%s\n' "$conflicts" \
    | awk -F '\t' '
        ($1 == "tests/mx-watch-triage.test.sh" && $2 == "tests/mx-watcher-lock.test.sh" && $3 ~ /watcher-process/) ||
        ($2 == "tests/mx-watch-triage.test.sh" && $1 == "tests/mx-watcher-lock.test.sh" && $3 ~ /watcher-process/) { found=1 }
        END { exit(found ? 0 : 1) }
      ' || fail "shared watcher-process pair missing from conflict matrix"
  printf '%s\n' "$conflicts" \
    | awk -F '\t' '
        ($1 == "tests/mx-test-run.test.sh" || $2 == "tests/mx-test-run.test.sh") && $3 ~ /global/ { found=1 }
        END { exit(found ? 0 : 1) }
      ' || fail "global resource does not conflict with the inventory"
  printf '%s\n' "$conflicts" \
    | awk -F '\t' '
        ($1 ~ /mx-pr-check-security-fault-quarantine/ && $2 ~ /mx-pr-check-security-retirement-teardown/ && $3 ~ /pr-security-process/) ||
        ($2 ~ /mx-pr-check-security-fault-quarantine/ && $1 ~ /mx-pr-check-security-retirement-teardown/ && $3 ~ /pr-security-process/) { found=1 }
        END { exit(found ? 0 : 1) }
      ' || fail "PR security process owners do not serialize"
  pass "conflict matrix covers shared and global resources"
}

test_archived_proof_matches_current_manifest() {
  assert_present "$PROOF_JSON" "docs/mx-test-isolation-proof.json missing"
  python3 - "$PROOF_JSON" "$RUNNER" <<'PY' \
    || fail "archived proof manifest or result shape is stale"
import hashlib
import json
import subprocess
import sys

proof = json.load(open(sys.argv[1], encoding="utf-8"))
manifest = subprocess.check_output(
    [sys.argv[2], "--list-resources", "--all"], text=True
)
assert proof["kind"] == "resource-isolation-proof"
assert proof["manifest_sha256"] == hashlib.sha256(manifest.encode()).hexdigest()
assert proof["summary"]["failed_rounds"] == 0
assert proof["summary"]["leaks"] == 0
assert proof["repeats"] >= 1
assert len(proof["resource_manifest"]) == len(manifest.splitlines())
PY
  pass "archived proof matches the current manifest and is leak-free"
}

test_docs_and_ci_describe_the_resource_scheduler() {
  assert_present "$PROOF_DOC" "docs/mx-test-isolation-proof.md missing"
  grep -Fq 'resource manifest' "$PROOF_DOC" \
    || fail "proof documentation must describe the resource manifest"
  grep -Fq 'target/release/mx test-isolation-proof --list-conflicts' "$PROOF_DOC" \
    || fail "proof documentation must show conflict-matrix inspection"
  grep -Fq 'target/release/mx test-run --all --jobs auto' "$CONTRIB" \
    || fail "CONTRIBUTING does not show the accelerated full run"
  grep -Fq 'target/release/mx test-run --all --jobs 1' "$CONTRIB" \
    || fail "CONTRIBUTING does not show the serial reference"
  grep -Fq 'target/release/mx test-run --lane portable-parallel-1' "$CI" \
    || fail "CI shard 1 is not runner-owned"
  grep -Fq -- '--jobs auto' "$CI" \
    || fail "CI portable lanes do not use the resource scheduler"
  pass "docs and CI route through the resource-aware owners"
}

test_proof_consumes_exact_runner_manifest
test_candidate_and_exclusion_partition
test_conflict_matrix_is_complete_for_shared_resources
test_archived_proof_matches_current_manifest
test_docs_and_ci_describe_the_resource_scheduler
