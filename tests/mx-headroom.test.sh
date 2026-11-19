#!/usr/bin/env bash
# Configured concurrency limits; retired host metrics cannot constrain admission.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

HEADROOM="$ROOT/bin/mx-headroom.sh"
TMP_ROOT=$(mx_test_tmproot mx-headroom)
unset MX_HEADROOM_SKIP_QUEUE

headroom_json() {
  MX_HOME="$TMP_ROOT/home" \
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
  json=$(headroom_json) || fail "headroom JSON failed"
  capacity=$(json_field "$json" capacity)
  in_use=$(json_field "$json" in_use)
  available=$(json_field "$json" available)
  [ "$capacity" -eq $((in_use + available)) ] || fail "capacity != in_use + available"
  [ "$(json_field "$json" model)" = configured-concurrency ] || fail "concurrency model name is wrong"
  [ "$(json_field "$json" at_limit)" = false ] || fail "healthy fixture reported at_limit"
  [ "$(json_field "$json" api.source)" = configured-budget ] \
    || fail "API input overclaimed live provider quota"
  [ "$(json_field "$json" candidates.default.window)" = configured-budget ] \
    || fail "candidate detail omitted the conservative source"

  pass "headroom emits valid, internally consistent concurrency JSON"
}

test_only_configured_concurrency_bounds_dispatch() {
  local json
  MX_HEADROOM_IN_USE_TEST=2 MX_HEADROOM_API_TEST=2 json=$(headroom_json)
  [ "$(json_field "$json" available)" -eq 0 ] || fail "configured limit did not bound dispatch"
  [ "$(json_field "$json" at_limit)" = true ] || fail "concurrency exhaustion did not set at_limit"
  MX_HEADROOM_IN_USE_TEST=3 MX_HEADROOM_API_TEST=2 json=$(headroom_json)
  [ "$(json_field "$json" capacity)" -eq 2 ] || fail "configured cap changed when usage exceeded it"
  [ "$(json_field "$json" available)" -eq 0 ] || fail "over-limit usage underflowed"
  MX_HEADROOM_IN_USE_TEST=0 MX_HEADROOM_API_TEST=0 json=$(headroom_json)
  [ "$(json_field "$json" at_limit)" = true ] || fail "zero capacity did not pause admission"
  pass "configured concurrency alone bounds new launches, including zero and reduced limits"
}

test_candidate_budgets_are_accounted() {
  local home="$TMP_ROOT/candidates" json
  mkdir -p "$home/config"
  printf '%s\n' '{"rules":[{"when":"large","use":[{"harness":"claude"},{"harness":"codex"},{"harness":"cursor"}]}]}' \
    > "$home/config/actor-dispatch.json"
  printf '%s\n' 2 > "$home/config/api-capacity"
  printf '%s\n' 0 > "$home/config/api-capacity-claude"
  printf '%s\n' 2 > "$home/config/api-capacity-codex"
  printf '%s\n' 1 > "$home/config/api-capacity-cursor"
  json=$(MX_HOME="$home" MX_HEADROOM_IN_USE=0 \
    "$HEADROOM" --json) || fail "candidate-aware headroom failed"
  [ "$(json_field "$json" candidates.claude.available)" -eq 0 ] \
    || fail "claude candidate budget was not applied"
  [ "$(json_field "$json" candidates.codex.available)" -eq 2 ] \
    || fail "codex candidate budget was not applied"
  [ "$(json_field "$json" candidates.cursor.available)" -eq 1 ] \
    || fail "cursor candidate budget was not applied"
  [ "$(json_field "$json" available)" -eq 2 ] \
    || fail "aggregate headroom did not retain the candidate with real capacity"

  pass "headroom accounts every configured dispatch candidate"
}

test_unconfigured_api_capacity_defaults_to_twenty() {
  local home="$TMP_ROOT/default-capacity" json
  mkdir -p "$home/config"
  json=$(MX_HOME="$home" MX_HEADROOM_IN_USE=0 \
    MX_HEADROOM_API_CAPACITY='' "$HEADROOM" --json) \
    || fail "unconfigured default headroom failed"
  [ "$(json_field "$json" api.capacity)" -eq 20 ] \
    || fail "unconfigured API capacity did not default to 20"
  [ "$(json_field "$json" available)" -eq 20 ] \
    || fail "unconfigured headroom did not expose 20 dispatch slots without host metrics"
  [ "$(json_field "$json" candidates.default.capacity)" -eq 20 ] \
    || fail "default candidate did not inherit the 20-actor budget"

  pass "unconfigured API capacity defaults to twenty actors"
}

test_host_metrics_are_ignored_for_all_harnesses() {
  local home="$TMP_ROOT/retired-metrics" harness metrics json
  mkdir -p "$home/config"
  for harness in codex cursor pi claude; do
    printf '%s\n' "$harness" > "$home/config/subagent-harness"
    for metrics in exhausted unreadable; do
      if [ "$metrics" = exhausted ]; then
        export MX_HEADROOM_CPU_COUNT=1 MX_HEADROOM_LOAD1=99999 MX_HEADROOM_MEM_AVAILABLE_BYTES=0
      else
        export MX_HEADROOM_CPU_COUNT=invalid MX_HEADROOM_LOAD1=invalid MX_HEADROOM_MEM_AVAILABLE_BYTES=invalid
      fi
      json=$(MX_HOME="$home" MX_HEADROOM_IN_USE=0 MX_HEADROOM_API_CAPACITY='' \
        MX_HEADROOM_CPU_PER_ACTOR=0 MX_HEADROOM_MEM_PER_ACTOR_BYTES=0 \
        MX_HEADROOM_PLATFORM=Unknown MX_HEADROOM_PROC_ROOT=/nonexistent "$HEADROOM" --json) \
        || fail "$harness admission read retired $metrics metrics"
      [ "$(json_field "$json" available)" -eq 20 ] || fail "$harness was resource throttled"
      assert_not_contains "$json" '"local"' "retired local capacity remains exposed"
    done
  done
  unset MX_HEADROOM_CPU_COUNT MX_HEADROOM_LOAD1 MX_HEADROOM_MEM_AVAILABLE_BYTES
  pass "all four harnesses ignore exhausted, unreadable and invalid host metrics"
}

test_limit_can_be_changed_above_twenty() {
  local home="$TMP_ROOT/configurable" json
  mkdir -p "$home/config"
  printf '%s\n' 32 > "$home/config/api-capacity"
  json=$(MX_HOME="$home" MX_HEADROOM_IN_USE=20 MX_HEADROOM_API_CAPACITY='' "$HEADROOM" --json) || fail "configured headroom failed"
  [ "$(json_field "$json" capacity)" -eq 32 ] || fail "configured cap was not reported"
  [ "$(json_field "$json" available)" -eq 12 ] || fail "limit could not be increased above twenty"
  json=$(MX_HOME="$home" MX_HEADROOM_IN_USE=20 MX_HEADROOM_API_CAPACITY=40 "$HEADROOM" --json) || fail "override failed"
  [ "$(json_field "$json" available)" -eq 20 ] || fail "environment override lost precedence"
  pass "the configured limit can be raised above twenty without restarting"
}

test_invalid_counts_and_limits_refuse() {
  local out rc=0
  out=$(MX_HEADROOM_IN_USE_TEST=bad headroom_json 2>&1) || rc=$?
  [ "$rc" -ne 0 ] || fail "invalid actor count was accepted"
  assert_contains "$out" 'live actor count is unreadable' "count diagnostic was vague"
  rc=0
  out=$(MX_HEADROOM_API_TEST=bad headroom_json 2>&1) || rc=$?
  [ "$rc" -ne 0 ] || fail "invalid configured limit was accepted"
  assert_contains "$out" 'configured API capacity is invalid' "limit diagnostic was vague"
  pass "invalid running counts and configured limits still refuse"
}

test_malformed_candidate_configuration_refuses() {
  local home="$TMP_ROOT/malformed-candidates" out rc=0
  mkdir -p "$home/config"
  printf '%s\n' '{"rules":[{"use":[{"model":"missing-harness"}]}]}' \
    > "$home/config/actor-dispatch.json"
  out=$(MX_HOME="$home" MX_HEADROOM_IN_USE=0 \
    MX_HEADROOM_API_CAPACITY=5 "$HEADROOM" --json 2>&1) || rc=$?
  [ "$rc" -ne 0 ] || fail "malformed candidate configuration produced capacity"
  assert_contains "$out" 'configured dispatch candidates are unreadable' \
    "malformed candidate diagnostic was vague"
  assert_not_contains "$out" '"capacity"' \
    "malformed candidate configuration emitted a partial capacity object"

  pass "malformed dispatch candidates refuse instead of being omitted"
}

test_shape_and_internal_consistency
test_only_configured_concurrency_bounds_dispatch
test_candidate_budgets_are_accounted
test_unconfigured_api_capacity_defaults_to_twenty
test_host_metrics_are_ignored_for_all_harnesses
test_limit_can_be_changed_above_twenty
test_invalid_counts_and_limits_refuse
test_malformed_candidate_configuration_refuses

echo "ALL TESTS PASSED"
