#!/usr/bin/env bash
# Proves every pre-split top-level case appears exactly once in the new groups.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

MAP="$ROOT/docs/mx-test-split-assertions.json"
BASELINE="$ROOT/docs/mx-test-performance-baseline.json"
assert_present "$MAP" "split assertion map is missing"
assert_present "$BASELINE" "Plan-06 assertion baseline is missing"

python3 - "$ROOT" "$MAP" "$BASELINE" <<'PY' || fail "split assertion map does not match the Plan-06 baseline and helper dispatch"
import collections
import json
import re
import sys

root, map_path, baseline_path = sys.argv[1:4]
doc = json.load(open(map_path, encoding="utf-8"))
baseline = json.load(open(baseline_path, encoding="utf-8"))
baseline_by_path = {row["path"]: row for row in baseline["scripts"]}
assert doc["kind"] == "plan-6.5-split-assertion-map"
assert doc["baseline_inventory_scripts"] == 96
assert baseline["scripts_complete"] is True
expected_total = 0
for split in doc["splits"]:
    text = open(f'{root}/{split["helper"]}', encoding="utf-8").read()
    definitions = re.findall(r"^(test_[A-Za-z0-9_]+)\(\)", text, re.MULTILINE)
    current_assertions = re.findall(r'^\s*pass "([^"]+)"\s*$', text, re.MULTILINE)
    baseline_assertions = baseline_by_path[split["before"]]["assertions"]
    mapped = [
        case
        for cases in split["after_groups"].values()
        for case in cases
    ]
    counts = collections.Counter(mapped)
    assert len(mapped) == split["case_count"]
    assert all(count == 1 for count in counts.values())
    assert set(mapped) == set(definitions)
    assert collections.Counter(current_assertions) == collections.Counter(baseline_assertions)
    expected_total += split["case_count"]
assert expected_total == 140
PY

pass "all 140 Plan-06 cases and named assertions map exactly once to the split inventory"

tmp=
mx_test_tmproot_into tmp mx-test-helper-contract
(
  sleep 0.05
  : >"$tmp/ready"
) &
producer=$!
mx_test_wait_until 1000 "helper contract ready file" test -e "$tmp/ready" \
  || fail "condition wait did not observe completion"
wait "$producer"
pass "bounded condition wait observes completion without a fixed settle delay"

timed_output=$(mx_test_timed_case helper-contract printf 'ok - timed body\n')
assert_contains "$timed_output" "MX_TEST_CASE_BEGIN label=helper-contract" "timed-case begin marker"
assert_contains "$timed_output" "MX_TEST_CASE_END label=helper-contract exit=0 duration_ms=" "timed-case end marker"
pass "per-case timing preserves output and emits machine markers"

template="$tmp/git-template"
clone_a="$tmp/clone-a"
clone_b="$tmp/clone-b"
mx_test_make_git_template "$template"
mx_test_clone_git_template "$template" "$clone_a"
mx_test_clone_git_template "$template" "$clone_b"
printf 'mutated\n' >"$clone_a/README.md"
[ "$(cat "$clone_b/README.md")" != mutated ] || fail "git template clones shared mutable bytes"
[ "$(git -C "$clone_b" status --short)" = "" ] || fail "independent git clone started dirty"
pass "immutable git fixture templates produce independent writable clones"
