#!/usr/bin/env bash
# Composite local-resource and configured-API headroom contract.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

HEADROOM="$ROOT/bin/mx-headroom.sh"
TMP_ROOT=$(mx_test_tmproot mx-headroom)
unset MX_HEADROOM_SKIP_QUEUE

headroom_json() {
  MX_HOME="$TMP_ROOT/home" \
  MX_HEADROOM_CPU_COUNT="${MX_HEADROOM_CPU_COUNT_TEST:-8}" \
  MX_HEADROOM_LOAD1="${MX_HEADROOM_LOAD1_TEST:-2}" \
  MX_HEADROOM_MEM_AVAILABLE_BYTES="${MX_HEADROOM_MEM_TEST:-8589934592}" \
  MX_HEADROOM_IN_USE="${MX_HEADROOM_IN_USE_TEST:-1}" \
  MX_HEADROOM_API_CAPACITY="${MX_HEADROOM_API_TEST:-5}" \
    "$HEADROOM" --json
}

json_field() {
  printf '%s\n' "$1" | node -e '
    const field = process.argv[1];
    let input = "";
    process.stdin.on("data", value => input += value);
    process.stdin.on("end", () => {
      const data = JSON.parse(input);
      const value = field.split(".").reduce((current, key) => current[key], data);
      process.stdout.write(String(value));
    });
  ' "$2"
}

test_shape_and_internal_consistency() {
  local json capacity in_use available
  json=$(headroom_json) || fail "headroom JSON failed with readable signals"
  capacity=$(json_field "$json" capacity)
  in_use=$(json_field "$json" in_use)
  available=$(json_field "$json" available)
  [ "$capacity" -eq $((in_use + available)) ] || fail "capacity != in_use + available"
  [ "$(json_field "$json" model)" = local+api ] || fail "composite model name is wrong"
  [ "$(json_field "$json" at_limit)" = false ] || fail "healthy fixture reported at_limit"
  [ "$(json_field "$json" api.source)" = configured-budget ] \
    || fail "API input overclaimed live provider quota"
  [ "$(json_field "$json" candidates.default.window)" = configured-budget ] \
    || fail "candidate detail omitted the conservative source"

  pass "headroom emits valid, internally consistent composite JSON"
}

test_each_half_can_bound_dispatch() {
  local json
  MX_HEADROOM_CPU_COUNT_TEST=2 MX_HEADROOM_LOAD1_TEST=2 MX_HEADROOM_MEM_TEST=8589934592 \
    MX_HEADROOM_IN_USE_TEST=1 MX_HEADROOM_API_TEST=10 json=$(headroom_json)
  [ "$(json_field "$json" available)" -eq 0 ] || fail "exhausted local resources did not bound dispatch"
  [ "$(json_field "$json" at_limit)" = true ] || fail "local exhaustion did not set at_limit"

  MX_HEADROOM_CPU_COUNT_TEST=16 MX_HEADROOM_LOAD1_TEST=0 MX_HEADROOM_MEM_TEST=68719476736 \
    MX_HEADROOM_IN_USE_TEST=2 MX_HEADROOM_API_TEST=2 json=$(headroom_json)
  [ "$(json_field "$json" available)" -eq 0 ] || fail "exhausted API budget did not bound dispatch"
  [ "$(json_field "$json" at_limit)" = true ] || fail "API exhaustion did not set at_limit"

  pass "the tighter local or API signal independently stops dispatch"
}

test_candidate_budgets_are_accounted() {
  local home="$TMP_ROOT/candidates" json
  mkdir -p "$home/config"
  printf '%s\n' '{"rules":[{"when":"large","use":[{"harness":"claude"},{"harness":"codex"}]}]}' \
    > "$home/config/actor-dispatch.json"
  printf '%s\n' 2 > "$home/config/api-capacity"
  printf '%s\n' 0 > "$home/config/api-capacity-claude"
  printf '%s\n' 2 > "$home/config/api-capacity-codex"
  json=$(MX_HOME="$home" MX_HEADROOM_CPU_COUNT=16 MX_HEADROOM_LOAD1=0 \
    MX_HEADROOM_MEM_AVAILABLE_BYTES=68719476736 MX_HEADROOM_IN_USE=0 \
    "$HEADROOM" --json) || fail "candidate-aware headroom failed"
  [ "$(json_field "$json" candidates.claude.available)" -eq 0 ] \
    || fail "claude candidate budget was not applied"
  [ "$(json_field "$json" candidates.codex.available)" -eq 2 ] \
    || fail "codex candidate budget was not applied"
  [ "$(json_field "$json" available)" -eq 2 ] \
    || fail "aggregate headroom did not retain the candidate with real capacity"

  pass "headroom accounts every configured dispatch candidate"
}

test_unreadable_signals_refuse() {
  local out rc=0
  out=$(MX_HOME="$TMP_ROOT/unreadable" MX_HEADROOM_PLATFORM=Unknown \
    MX_HEADROOM_API_CAPACITY=1 "$HEADROOM" --json 2>&1) || rc=$?
  [ "$rc" -ne 0 ] || fail "unreadable local signals produced fabricated capacity"
  assert_contains "$out" 'CPU capacity signal is unreadable' "unreadable signal diagnostic was vague"
  assert_not_contains "$out" '"capacity"' "unreadable signal emitted a capacity object"

  pass "unreadable signals refuse instead of guessing"
}

test_shape_and_internal_consistency
test_each_half_can_bound_dispatch
test_candidate_budgets_are_accounted
test_unreadable_signals_refuse

echo "ALL TESTS PASSED"
