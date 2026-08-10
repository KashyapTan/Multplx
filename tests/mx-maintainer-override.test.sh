#!/usr/bin/env bash
# Exact, single-use maintainer override state-machine behavior tests.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mx_test_tmproot_into TMP_ROOT mx-maintainer-override
STATE=$TMP_ROOT/state
export MX_STATE_OVERRIDE=$STATE
# shellcheck source=bin/mx-maintainer-override-lib.sh
. "$ROOT/bin/mx-maintainer-override-lib.sh"

# Tests exercise transitions without impersonating a harness process.
# The production CLI restriction is tested separately below.
mx_override_require_primary_lock() { return 0; }

digest() { mx_override_sha256_text "$1"; }

request_one() {
  local boundary=$1 operation=$2 target=$3 ttl=${4:-300}
  mx_override_request "$boundary" task-18 multplx "$operation" "$target" \
    "$(digest state-v1)" "The named safeguard will be skipped for this exact action." "$ttl"
}

grant_one() {
  local request=$1
  local file boundary operation target
  file=$(mx_override_find_record "$request") || return 1
  boundary=$(jq -r '.boundary_id' "$file")
  operation=$(jq -r '.action_argv_or_operation' "$file")
  target=$(jq -r '.target_identity' "$file")
  mx_override_grant "$request" "Grant $boundary for exact operation $operation on exact target $target."
}

request_binding() {
  local bindings=$1
  mx_override_request \
    "$(printf '%s' "$bindings" | jq -r '.boundary')" \
    "$(printf '%s' "$bindings" | jq -r '.task')" \
    "$(printf '%s' "$bindings" | jq -r '.project')" \
    "$(printf '%s' "$bindings" | jq -r '.operation')" \
    "$(printf '%s' "$bindings" | jq -r '.target')" \
    "$(printf '%s' "$bindings" | jq -r '.expected_state_digest')" \
    "$(printf '%s' "$bindings" | jq -r '.consequence')"
}

test_platform_stat_dispatch() {
  (
    uname() { printf '%s\n' Linux; }
    stat() {
      case "$1:$2" in
        '-c:%a') printf '%s\n' 600 ;;
        '-c:%h') printf '%s\n' 1 ;;
        '-f:'*) printf '%s\n' 'GNU filesystem output' ;;
        *) return 1 ;;
      esac
    }
    [ "$(mx_override_file_mode ignored)" = 600 ] \
      && [ "$(mx_override_link_count ignored)" = 1 ]
  ) || fail "Linux mode/link checks used BSD stat syntax"

  (
    uname() { printf '%s\n' Darwin; }
    stat() {
      case "$1:$2" in
        '-f:%Lp') printf '%s\n' 600 ;;
        '-f:%l') printf '%s\n' 1 ;;
        '-c:'*) printf '%s\n' 'BSD invalid option output' ;;
        *) return 1 ;;
      esac
    }
    [ "$(mx_override_file_mode ignored)" = 600 ] \
      && [ "$(mx_override_link_count ignored)" = 1 ]
  ) || fail "Darwin mode/link checks used GNU stat syntax"
  pass "override record security selects the native stat interface explicitly"
}

test_schema_permissions_and_literal_transport() {
  local sentinel=$TMP_ROOT/must-not-run operation request file root
  operation="printf '%s' '\$(touch $sentinel)' && literal ; newline
second-line"
  request=$(request_one dependency.install "$operation" host-tooling)
  file=$(mx_override_find_record "$request") || fail "request record was not published"
  root=$(mx_override_state_root)
  [ "$(mx_override_file_mode "$root")" = 700 ] || fail "override root mode is not 0700"
  [ "$(mx_override_file_mode "$file")" = 600 ] || fail "override record mode is not 0600"
  [ "$(jq -r '.action_argv_or_operation' "$file")" = "$operation" ] || fail "literal operation changed"
  [ ! -e "$sentinel" ] || fail "literal operation was executed while recording"
  mx_override_record_validate "$file" pending || fail "published request does not validate"
  pass "schema, permissions, and literal operation transport are closed and inert"
}

test_primary_only_and_proactive_grant() {
  local request file operation target boundary
  request=$(request_one cleanup.discard-unlanded "discard task resources for task-18" /tmp/task-18)
  if MX_STATE_OVERRIDE="$STATE" "$ROOT/bin/mx-maintainer-override.sh" grant "$request" \
      --maintainer-words "generic yes" >/dev/null 2>&1; then
    fail "CLI granted without lock-owning primary ancestry"
  fi
  if mx_override_grant "$request" "finish it" >/dev/null 2>&1; then
    fail "generic maintainer language qualified as a grant"
  fi
  file=$(mx_override_find_record "$request")
  boundary=$(jq -r '.boundary_id' "$file")
  operation=$(jq -r '.action_argv_or_operation' "$file")
  target=$(jq -r '.target_identity' "$file")
  mx_override_grant "$request" "I grant $boundary for $operation on $target only." \
    || fail "exact proactive grant was refused"
  [ -f "$(mx_override_state_root)/granted/$request.json" ] || fail "grant did not transition atomically"
  pass "only the primary can decide and generic language cannot widen authority"
}

test_atomic_consume_replay_and_result() {
  local request operation target state_digest one=$TMP_ROOT/one two=$TMP_ROOT/two successes
  operation="discard task resources for atomic-task"
  target=/tmp/atomic-task
  state_digest=$(digest state-v1)
  request=$(request_one cleanup.discard-unlanded "$operation" "$target")
  grant_one "$request" || fail "could not grant concurrency fixture"
  (
    MX_STATE_OVERRIDE="$STATE" "$ROOT/bin/mx-maintainer-override.sh" consume "$request" \
      --boundary cleanup.discard-unlanded --task task-18 --project multplx \
      --operation "$operation" --target "$target" --expected-state "$state_digest"
  ) >"$one" 2>/dev/null & first=$!
  (
    MX_STATE_OVERRIDE="$STATE" "$ROOT/bin/mx-maintainer-override.sh" consume "$request" \
      --boundary cleanup.discard-unlanded --task task-18 --project multplx \
      --operation "$operation" --target "$target" --expected-state "$state_digest"
  ) >"$two" 2>/dev/null & second=$!
  wait "$first"; first_rc=$?
  wait "$second"; second_rc=$?
  successes=0
  [ "$first_rc" -ne 0 ] || successes=$((successes + 1))
  [ "$second_rc" -ne 0 ] || successes=$((successes + 1))
  [ "$successes" -eq 1 ] || fail "concurrent grant was consumed $successes times"
  if mx_override_consume "$request" cleanup.discard-unlanded task-18 multplx \
      "$operation" "$target" "$state_digest" >/dev/null 2>&1; then
    fail "consumed grant replay succeeded"
  fi
  mx_override_result "$request" succeeded "exact destructive action completed"
  [ "$(jq -r '.outcome' "$(mx_override_state_root)/consumed/$request.json")" = succeeded ] \
    || fail "truthful exceptional outcome was not recorded"
  if mx_override_result "$request" failed "rewrite outcome" >/dev/null 2>&1; then
    fail "recorded outcome was rewritten"
  fi
  pass "grant consumption is atomic, single-use, and outcome-final"
}

test_changed_binding_stales_grant() {
  local request operation target
  operation='merge exact red PR'
  target='https://github.com/acme/repo/pull/7@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
  request=$(request_one delivery.merge-red "$operation" "$target")
  grant_one "$request" || fail "could not grant stale fixture"
  if mx_override_consume "$request" delivery.merge-red task-18 multplx "$operation" "$target" \
      "$(digest changed-state)" >/dev/null 2>&1; then
    fail "changed expected state consumed a grant"
  fi
  [ -f "$(mx_override_state_root)/stale/$request.json" ] || fail "changed binding was not made stale"
  [ "$(jq -r '.outcome' "$(mx_override_state_root)/stale/$request.json")" = state-changed ] \
    || fail "changed binding outcome is not truthful"
  pass "changed target state requires a new maintainer decision"
}

test_denial_expiry_copy_and_audit() {
  local denied expired copied original_now audit
  denied=$(request_one workflow.skip-stage 'skip workflow stage build in run release' 'run#build')
  mx_override_deny "$denied" "No, preserve the workflow stage." || fail "denial failed"
  [ -f "$(mx_override_state_root)/denied/$denied.json" ] || fail "denial did not preserve ordinary path"

  expired=$(request_one dependency.install 'install exact package tool@1' host-tooling 1)
  original_now=$(mx_override_now)
  # shellcheck disable=SC2329  # indirect override consumed by the library under test
  mx_override_now() { printf '%s\n' "$((original_now + 2))"; }
  if grant_one "$expired" >/dev/null 2>&1; then fail "expired request was granted"; fi
  unset -f mx_override_now
  mx_override_now() { date +%s; }
  [ "$(jq -r '.outcome' "$(mx_override_state_root)/stale/$expired.json")" = expired ] \
    || fail "expired request was not recorded stale"

  copied=$(request_one security.one-action-elevation '["tool","exact"]' sandbox-action)
  grant_one "$copied" || fail "copy fixture grant failed"
  cp "$(mx_override_state_root)/granted/$copied.json" "$(mx_override_state_root)/pending/$copied.json"
  chmod 600 "$(mx_override_state_root)/pending/$copied.json"
  if mx_override_consume "$copied" security.one-action-elevation task-18 multplx \
      '["tool","exact"]' sandbox-action "$(digest state-v1)" >/dev/null 2>&1; then
    fail "duplicated authority record was consumed"
  fi
  if audit=$(MX_STATE_OVERRIDE="$STATE" "$ROOT/bin/mx-maintainer-override.sh" audit --json 2>/dev/null); then
    fail "audit accepted duplicated or misplaced authority"
  fi
  printf '%s' "$audit" | jq -e 'type == "array"' >/dev/null || fail "JSON audit output is malformed"
  pass "denial, expiry, copied-record refusal, and invalid audit remain deterministic"
}

test_registry_policy_requests_are_distinct() {
  local line boundary class request count=0
  while IFS=$'\t' read -r boundary class _; do
    [ "$class" = policy ] || continue
    request=$(request_one "$boundary" "exercise exact alternate $boundary" "target-$count") \
      || fail "registered policy boundary could not create its own request: $boundary"
    [ -f "$(mx_override_state_root)/pending/$request.json" ] || fail "policy request missing: $boundary"
    count=$((count + 1))
  done <<EOF
$(mx_override_registry)
EOF
  [ "$count" -ge 12 ] || fail "policy registry is unexpectedly incomplete"
  pass "every registered policy boundary receives a distinct exact request"
}

test_exact_command_alternates_and_capability_verification() {
  local repo bindings request failed_request install_bin installed install_request elevated elevation_request
  repo=$TMP_ROOT/direct-project
  mx_git_init_commit "$repo"
  bindings=$("$ROOT/bin/mx-override-run.sh" --print-bindings \
    --boundary project.direct-write --task direct-18 --project direct-project --target "$repo" \
    -- touch controlled.txt) || fail "direct-write binding failed"
  request=$(request_binding "$bindings") || fail "direct-write request failed"
  grant_one "$request" || fail "direct-write grant failed"
  "$ROOT/bin/mx-override-run.sh" "$request" --boundary project.direct-write \
    --task direct-18 --project direct-project --target "$repo" -- touch controlled.txt \
    || fail "controlled direct-write action failed"
  [ -f "$repo/controlled.txt" ] || fail "controlled direct-write did not run in the target checkout"
  [ "$(jq -r '.outcome' "$STATE/maintainer-overrides/consumed/$request.json")" = succeeded ] \
    || fail "direct-write outcome was not recorded"
  if "$ROOT/bin/mx-override-run.sh" "$request" --boundary project.direct-write \
      --task direct-18 --project direct-project --target "$repo" -- touch controlled.txt >/dev/null 2>&1; then
    fail "direct-write grant replay succeeded"
  fi

  bindings=$("$ROOT/bin/mx-override-run.sh" --print-bindings \
    --boundary project.direct-write --task direct-fail --project direct-project --target "$repo" \
    -- sh -c 'exit 7') || fail "failing direct-write binding failed"
  failed_request=$(request_binding "$bindings") || fail "failing direct-write request failed"
  grant_one "$failed_request" || fail "failing direct-write grant failed"
  if "$ROOT/bin/mx-override-run.sh" "$failed_request" --boundary project.direct-write \
      --task direct-fail --project direct-project --target "$repo" -- sh -c 'exit 7' >/dev/null 2>&1; then
    fail "failing direct-write action reported success"
  fi
  [ "$(jq -r '.outcome' "$STATE/maintainer-overrides/consumed/$failed_request.json")" = failed ] \
    || fail "exceptional command failure was not recorded"

  install_bin=$TMP_ROOT/install-bin
  installed=mx-plan18-installed
  mkdir -p "$install_bin"
  PATH="$install_bin:$PATH"
  export PATH
  # shellcheck disable=SC2016  # inner shell expands its positional parameter
  bindings=$("$ROOT/bin/mx-override-run.sh" --print-bindings \
    --boundary dependency.install --task install-18 --project multplx \
    --target "command:$installed" --verify-command "$installed" -- \
    sh -c 'printf "#!/usr/bin/env bash\nexit 0\n" > "$1"; chmod +x "$1"' sh "$install_bin/$installed") \
    || fail "dependency-install binding failed"
  install_request=$(request_binding "$bindings") || fail "dependency-install request failed"
  grant_one "$install_request" || fail "dependency-install grant failed"
  # shellcheck disable=SC2016  # inner shell expands its positional parameter
  "$ROOT/bin/mx-override-run.sh" "$install_request" --boundary dependency.install \
    --task install-18 --project multplx --target "command:$installed" --verify-command "$installed" -- \
    sh -c 'printf "#!/usr/bin/env bash\nexit 0\n" > "$1"; chmod +x "$1"' sh "$install_bin/$installed" \
    || fail "approved dependency install or capability verification failed"
  command -v "$installed" >/dev/null || fail "dependency capability was not re-verified"

  elevated=$TMP_ROOT/elevated-once
  bindings=$("$ROOT/bin/mx-override-run.sh" --print-bindings \
    --boundary security.one-action-elevation --task elevate-18 --project multplx \
    --target exact-local-action -- touch "$elevated") || fail "elevation binding failed"
  elevation_request=$(request_binding "$bindings") || fail "elevation request failed"
  grant_one "$elevation_request" || fail "elevation grant failed"
  "$ROOT/bin/mx-override-run.sh" "$elevation_request" --boundary security.one-action-elevation \
    --task elevate-18 --project multplx --target exact-local-action -- touch "$elevated" \
    || fail "one-action elevation path failed"
  [ -f "$elevated" ] || fail "one-action elevation did not run the exact argv"
  pass "exact command alternates bind state, record failures, and re-check installed capability"
}

test_operator_handoff_requires_consumption() {
  local operation target state_digest request out
  operation='perform interactive Cursor authentication for the configured local account'
  target='cursor-agent-local-login'
  state_digest=$(digest cursor-login-required)
  request=$(mx_override_request authentication.login auth-18 multplx "$operation" "$target" "$state_digest" \
    'Open only the official interactive login, then re-check authenticated status.') || fail "login request failed"
  grant_one "$request" || fail "login grant failed"
  if "$ROOT/bin/mx-maintainer-override.sh" handoff "$request" >/dev/null 2>&1; then
    fail "operator handoff printed before atomic consumption"
  fi
  mx_override_consume "$request" authentication.login auth-18 multplx "$operation" "$target" "$state_digest" >/dev/null \
    || fail "login handoff consumption failed"
  out=$("$ROOT/bin/mx-maintainer-override.sh" handoff "$request") || fail "consumed operator handoff failed"
  assert_contains "$out" 'boundary=authentication.login' "handoff omitted boundary"
  assert_contains "$out" "operation=$operation" "handoff omitted exact operation"
  [ "$(jq -r '.outcome' "$STATE/maintainer-overrides/consumed/$request.json")" = not-run ] \
    || fail "printing a handoff forged a successful login"
  mx_override_result "$request" succeeded 'operator completed login and authenticated status was re-checked'
  pass "authentication capability becomes an exact consumed operator handoff, never forged success"
}

test_platform_stat_dispatch
test_schema_permissions_and_literal_transport
test_primary_only_and_proactive_grant
test_atomic_consume_replay_and_result
test_changed_binding_stales_grant
test_denial_expiry_copy_and_audit
test_registry_policy_requests_are_distinct
test_exact_command_alternates_and_capability_verification
test_operator_handoff_requires_consumption
