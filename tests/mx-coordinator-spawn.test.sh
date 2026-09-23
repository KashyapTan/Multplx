#!/usr/bin/env bash
# Isolated black-box coverage for the public scoped coordinator spawn form.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_BASE=$(mx_test_tmproot mx-coordinator-spawn)
TMP_ROOT="$TMP_BASE/fixture with spaces"
RUNTIME="$TMP_ROOT/runtime root"
HOME_DIR="$TMP_ROOT/main home"
FAKEBIN=$(mx_fakebin "$TMP_ROOT/fake")
TMUX_LOG="$TMP_ROOT/tmux.log"
TASK_TMP="$TMP_ROOT/task-tmp"

mkdir -p "$RUNTIME/bin" "$HOME_DIR/data" "$HOME_DIR/state" "$HOME_DIR/config" "$HOME_DIR/projects" "$TASK_TMP"
cp "$ROOT/AGENTS_E.md" "$RUNTIME/AGENTS.md"
printf '%s\n' codex > "$HOME_DIR/config/daemon-harness"

cat > "$FAKEBIN/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "${1:-}" in
  -V) printf '%s\n' 'tmux 3.4'; exit 0 ;;
  has-session|new-session|kill-window) exit 0 ;;
  new-window)
    printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
    [ "${MX_FAKE_TMUX_FAIL_CREATE:-0}" = 1 ] && exit 1
    printf '@1\n'
    exit 0
    ;;
  display-message)
    case "$*" in
      *'#S'*) printf 'broker\n' ;;
      *'#{pane_id}'*) printf '%%1\n' ;;
      *) printf 'broker\n' ;;
    esac
    exit 0
    ;;
  send-keys) printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"; exit 0 ;;
esac
exit 0
SH
chmod +x "$FAKEBIN/tmux"
: > "$TMUX_LOG"

cat > "$FAKEBIN/herdr" <<'SH'
#!/usr/bin/env bash
set -u
case "${1:-} ${2:-}" in
  '--version ')
    printf '%s\n' 'herdr 0.7.1'
    ;;
  'status --json')
    printf '%s\n' '{"client":{"version":"0.7.1","protocol":14},"server":{"running":true}}'
    ;;
esac
exit 0
SH
chmod +x "$FAKEBIN/herdr"

run_mx() {
  MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$HOME_DIR" \
    MX_STATE_OVERRIDE="$HOME_DIR/state" MX_DATA_OVERRIDE="$HOME_DIR/data" \
    MX_PROJECTS_OVERRIDE="$HOME_DIR/projects" MX_CONFIG_OVERRIDE="$HOME_DIR/config" \
    MX_SPAWN_NO_GUARD=1 MX_FAKE_TMUX_LOG="$TMUX_LOG" \
    TMPDIR="$TASK_TMP" \
    PATH="$FAKEBIN:$PATH" "$MX_RUST_BIN" "$@"
}

test_idea_spawn_and_repeat_adopt_one_coordinator() {
  local first second meta model private_home out status implementation_repo coordinator_launch
  local attempt_id attempt_generation brief_revision window_count root_state
  first=$(run_mx spawn research --sub-orchestrator --idea runtime-v2 \
    --scope 'assess runtime v2 before repository selection' \
    --request-id research-create --backend tmux --harness codex --json) \
    || fail "named idea coordinator spawn failed"
  printf '%s' "$first" | jq -e '
    .status == "ok" and .disposition == "running" and
    .request_id == "research-create" and .coordinator_id == "research" and
    .persistent == false and (.home | contains("fixture with spaces")) and
    (.domain_id | startswith("domain-")) and (.endpoint | length) > 0
  ' >/dev/null || fail 'JSON spawn result omitted canonical coordinator identity'
  meta="$HOME_DIR/state/research.meta"
  assert_present "$meta" 'coordinator metadata was not published'
  model=$(sed -n 's/^canonical_model=//p' "$meta")
  printf '%s' "$model" | jq -e '
    .role == "sub-orchestrator" and
    .artifact == "coordination" and
    .persistent == false and
    .private_home == true and
    .domain.idea_id == "runtime-v2" and
    .domain.projects == [] and
    .domain.scope_revision == 1 and
    .domain.assignment_generation == 1 and
    .runtime.endpoint != null
  ' >/dev/null || fail 'canonical coordinator/domain binding is incomplete'
  private_home=$(printf '%s' "$model" | jq -r .persistent_home)
  root_state=$(cd "$HOME_DIR/state" && pwd -P)
  assert_present "$private_home/data/charter.md" 'private coordinator charter is missing'
  assert_grep 'assess runtime v2 before repository selection' "$private_home/data/charter.md" \
    'private charter lost accepted scope'
  [ "$(grep -c '^new-window ' "$TMUX_LOG")" -eq 1 ] || fail 'initial spawn did not create exactly one endpoint'
  coordinator_launch=$(find "$TASK_TMP" -name launch.sh -print -quit)
  assert_present "$coordinator_launch" 'coordinator launch script was not installed'
  assert_grep "MX_STATE_OVERRIDE='$private_home/state'" "$coordinator_launch" \
    'installed coordinator launch did not bind state to its private home'
  assert_grep "MX_ROOT_OVERRIDE='$RUNTIME'" "$coordinator_launch" \
    'installed coordinator launch did not retain the runtime package root'
  cat > "$FAKEBIN/codex" <<'SH'
#!/usr/bin/env bash
set -u
env > "$MX_TEST_ENV_CAPTURE"
set +e
"$MX_RUST_BIN" request get smoke-domain-child > "$MX_TEST_REQUEST_OUTPUT" 2>&1
printf '%s\n' "$?" > "$MX_TEST_REQUEST_STATUS"
exit 0
SH
  chmod +x "$FAKEBIN/codex"
  coordinator_mcp=$(find "$TASK_TMP" -name report-mcp.json -print -quit)
  assert_present "$coordinator_mcp" 'coordinator report MCP configuration was not installed'
  jq -e --arg home "$private_home" --arg state "$root_state" \
    --arg root "$RUNTIME" --arg bin "$MX_RUST_BIN" '
    .mcpServers.multplx_status.env.MX_TASK_ID == "research" and
    .mcpServers.multplx_status.env.MX_HOME == $home and
    .mcpServers.multplx_status.env.MX_REPORT_STATE_OVERRIDE == $state and
    .mcpServers.multplx_status.env.MX_RUST_SOURCE_ROOT == $root and
    .mcpServers.multplx_status.env.MX_RUST_BIN == $bin
  ' "$coordinator_mcp" >/dev/null || {
    printf 'expected home=%s state=%s root=%s bin=%s\n' \
      "$private_home" "$HOME_DIR/state" "$RUNTIME" "$MX_RUST_BIN"
    cat "$coordinator_mcp"
    fail 'coordinator report MCP crossed private and parent identities or lost installed runtime paths'
  }

  mkdir -p "$private_home/state/request-inbox"
  printf '{' > "$private_home/state/request-inbox/smoke-domain-child.json"
  env MX_ROOT_OVERRIDE="$HOME_DIR" MX_HOME="$HOME_DIR" \
    MX_STATE_OVERRIDE="$HOME_DIR/state" MX_DATA_OVERRIDE="$HOME_DIR/data" \
    MX_PROJECTS_OVERRIDE="$HOME_DIR/projects" MX_CONFIG_OVERRIDE="$HOME_DIR/config" \
    MX_TEST_ENV_CAPTURE="$TMP_ROOT/coordinator-env" \
    MX_TEST_REQUEST_OUTPUT="$TMP_ROOT/coordinator-request.out" \
    MX_TEST_REQUEST_STATUS="$TMP_ROOT/coordinator-request.status" \
    PATH="$FAKEBIN:$PATH" "$coordinator_launch" \
    || fail 'generated coordinator launch script did not execute the fake provider'
  assert_grep "MX_HOME=$private_home" "$TMP_ROOT/coordinator-env" \
    'generated coordinator launch did not bind MX_HOME to its private home'
  assert_grep "MX_ROOT_OVERRIDE=$RUNTIME" "$TMP_ROOT/coordinator-env" \
    'generated coordinator launch did not bind MX_ROOT_OVERRIDE to the runtime package'
  assert_grep "MX_STATE_OVERRIDE=$private_home/state" "$TMP_ROOT/coordinator-env" \
    'generated coordinator launch did not bind state to its private home'
  assert_grep "MX_DATA_OVERRIDE=$private_home/data" "$TMP_ROOT/coordinator-env" \
    'generated coordinator launch did not bind data to its private home'
  assert_grep "MX_PROJECTS_OVERRIDE=$private_home/projects" "$TMP_ROOT/coordinator-env" \
    'generated coordinator launch did not bind projects to its private home'
  assert_grep "MX_CONFIG_OVERRIDE=$private_home/config" "$TMP_ROOT/coordinator-env" \
    'generated coordinator launch did not bind config to its private home'
  [ "$(cat "$TMP_ROOT/coordinator-request.status")" -ne 0 ] \
    || fail 'malformed private request receipt unexpectedly decoded'
  out=$(cat "$TMP_ROOT/coordinator-request.out")
  assert_contains "$out" 'EOF while parsing an object' \
    'coordinator request lookup did not decode the private request-inbox receipt'
  assert_not_contains "$out" 'No such file or directory' \
    'generated coordinator launch request lookup missed its private receipt under inherited parent overrides'
  rm "$private_home/state/request-inbox/smoke-domain-child.json"

  second=$(run_mx spawn research --sub-orchestrator --idea runtime-v2 \
    --scope 'assess runtime v2 before repository selection' \
    --request-id research-create --backend tmux --harness codex --json) \
    || fail 'exact coordinator spawn retry failed'
  printf '%s' "$second" | jq -e '.disposition == "recovered" and .request_id == "research-create"' \
    >/dev/null || fail 'repeat did not report adopted coordinator identity'
  [ "$(grep -c '^new-window ' "$TMUX_LOG")" -eq 1 ] || fail 'repeat spawned a duplicate endpoint'

  implementation_repo="$TMP_ROOT/unbound implementation"
  mx_git_init_commit "$implementation_repo"
  mkdir -p "$private_home/data/unbound-implementation"
  printf '%s\n' 'implement the unbound idea' \
    > "$private_home/data/unbound-implementation/brief.md"
  attempt_id=$(printf '%s' "$model" | jq -r .attempt.id)
  attempt_generation=$(printf '%s' "$model" | jq -r .attempt.generation)
  brief_revision=$(printf '%s' "$model" | jq -r .accepted_brief_revision)
  window_count=$(grep -c '^new-window ' "$TMUX_LOG")
  set +e
  out=$(MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$private_home" \
    MX_STATE_OVERRIDE="$private_home/state" MX_DATA_OVERRIDE="$private_home/data" \
    MX_PROJECTS_OVERRIDE="$private_home/projects" MX_CONFIG_OVERRIDE="$private_home/config" \
    MX_REPORT_STATE_OVERRIDE="$HOME_DIR/state" MX_TASK_ID=research \
    MX_ATTEMPT_ID="$attempt_id" MX_ATTEMPT_GENERATION="$attempt_generation" \
    MX_BRIEF_REVISION="$brief_revision" MX_SPAWN_NO_GUARD=1 MX_FAKE_TMUX_LOG="$TMUX_LOG" \
    PATH="$FAKEBIN:$PATH" "$MX_RUST_BIN" spawn unbound-implementation \
      "$implementation_repo" --role implementer --output implementation \
      --request-id unbound-implementation-create --backend tmux --harness codex 2>&1)
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'idea-only domain delegated repository implementation before binding it'
  assert_contains "$out" 'implementation project is outside the initiating coordinator domain' \
    'idea-only implementation refusal did not identify the missing domain project binding'
  assert_absent "$private_home/state/unbound-implementation.meta" \
    'refused idea-domain implementation published child task authority'
  [ "$(grep -c '^new-window ' "$TMUX_LOG")" -eq "$window_count" ] \
    || fail 'refused idea-domain implementation launched an endpoint'
  model=$(sed -n 's/^canonical_model=//p' "$meta")
  printf '%s' "$model" | jq -e '.domain.projects == [] and .domain.idea_id == "runtime-v2"' \
    >/dev/null || fail 'refused implementation silently changed the idea domain binding'
  if [ -f "$private_home/data/projects.json" ]; then
    jq -e '.projects == []' "$private_home/data/projects.json" >/dev/null \
      || fail 'refused implementation registered an unbound private-home project'
  fi

  set +e
  out=$(run_mx spawn research --sub-orchestrator --idea entirely-different \
    --scope 'assess runtime v2 before repository selection' \
    --request-id research-conflict --backend tmux --harness codex --json 2>&1)
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'conflicting idea identity recovered an unrelated domain'
  assert_contains "$out" 'conflicts with the current domain revision' \
    'conflicting idea identity did not fail closed'
  pass 'named idea coordinator spawn publishes one canonical private-home assignment and adopts it on retry'
}

test_overlapping_domains_share_one_project_without_exclusivity() {
  local shared="$TMP_ROOT/shared overlap repo" first second model_a model_b
  local home_a home_b project_a project_b domain_a domain_b
  mx_git_init_commit "$shared"
  run_mx project register "$shared" --alias overlap >/dev/null \
    || fail 'shared overlap project registration failed'
  first=$(run_mx spawn overlap-a --sub-orchestrator --project overlap \
    --scope 'coordinate the alpha concern in the shared repository' \
    --request-id overlap-a-create --backend tmux --harness codex --json) \
    || fail 'first overlapping coordinator spawn failed'
  second=$(run_mx spawn overlap-b --sub-orchestrator --project overlap \
    --scope 'coordinate the beta concern in the shared repository' \
    --request-id overlap-b-create --backend tmux --harness codex --json) \
    || fail 'second overlapping coordinator spawn failed'
  printf '%s' "$first" | jq -e '.status == "ok" and .task_id == "overlap-a"' >/dev/null \
    || fail 'first overlapping coordinator did not report its identity'
  printf '%s' "$second" | jq -e '.status == "ok" and .task_id == "overlap-b"' >/dev/null \
    || fail 'second overlapping coordinator did not report its identity'

  model_a=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/overlap-a.meta")
  model_b=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/overlap-b.meta")
  home_a=$(printf '%s' "$model_a" | jq -r .persistent_home)
  home_b=$(printf '%s' "$model_b" | jq -r .persistent_home)
  project_a=$(printf '%s' "$model_a" | jq -r .domain.projects[0])
  project_b=$(printf '%s' "$model_b" | jq -r .domain.projects[0])
  domain_a=$(printf '%s' "$model_a" | jq -r .domain.domain_id)
  domain_b=$(printf '%s' "$model_b" | jq -r .domain.domain_id)
  [ "$project_a" = "$project_b" ] || fail 'overlapping domains resolved different canonical projects'
  [ "$home_a" != "$home_b" ] || fail 'overlapping domains shared one private home'
  [ "$domain_a" != "$domain_b" ] || fail 'overlapping domains shared one domain identity'
  printf '%s' "$model_a" | jq -e '
    .task_id == "overlap-a" and
    .domain.scope == "coordinate the alpha concern in the shared repository" and
    (.domain.projects | length) == 1
  ' >/dev/null || fail 'first overlapping domain lost its bounded task or scope identity'
  printf '%s' "$model_b" | jq -e '
    .task_id == "overlap-b" and
    .domain.scope == "coordinate the beta concern in the shared repository" and
    (.domain.projects | length) == 1
  ' >/dev/null || fail 'second overlapping domain lost its bounded task or scope identity'
  assert_absent "$home_a/projects/overlap" 'first overlapping domain copied the shared repository'
  assert_absent "$home_b/projects/overlap" 'second overlapping domain copied the shared repository'
  jq -e --arg path "$(cd "$shared" && pwd -P)" \
    '([.projects[].checkouts[].canonical_path] | index($path)) != null' \
    "$home_a/data/projects.json" >/dev/null \
    || fail 'first overlapping domain lost its borrowed canonical checkout'
  jq -e --arg path "$(cd "$shared" && pwd -P)" \
    '([.projects[].checkouts[].canonical_path] | index($path)) != null' \
    "$home_b/data/projects.json" >/dev/null \
    || fail 'second overlapping domain lost its borrowed canonical checkout'

  run_mx teardown overlap-a --stop-coordinator >/dev/null \
    || fail 'first overlapping coordinator cleanup failed'
  run_mx teardown overlap-b --stop-coordinator >/dev/null \
    || fail 'second overlapping coordinator cleanup failed'
  pass 'distinct scoped coordinators may borrow one canonical project without exclusivity'
}

test_stopped_idea_domain_binds_project_and_restarts_new_generation() {
  local selected="$TMP_ROOT/selected repo" model private_home restarted child
  local attempt_id attempt_generation brief_revision child_attempt child_generation child_revision
  mx_git_init_commit "$selected"
  run_mx project register "$selected" --alias selected >/dev/null || fail 'selected registration failed'
  run_mx teardown research --stop-coordinator >/dev/null || fail 'coordinator stop failed'
  run_mx domain bind-project research --expected-revision 1 --project selected \
    --reason 'repository selected after research' >/dev/null || fail 'idea project binding failed'
  model=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/research.meta")
  printf '%s' "$model" | jq -e '
    .domain.scope_revision == 2 and .domain.assignment_generation == 2 and
    (.domain.projects | length) == 1 and .accepted_brief_revision == 2 and
    .runtime.endpoint == null and
    (.briefs[1].source_artifacts | length) > 0
  ' >/dev/null || fail 'project binding did not publish the fenced accepted revision'
  private_home=$(printf '%s' "$model" | jq -r .persistent_home)
  jq -e '(.projects | length) == 1' "$private_home/data/projects.json" >/dev/null \
    || fail 'project binding did not update the private-home project catalog'
  assert_grep 'repository selected after research' "$HOME_DIR/state/research.meta" \
    'project binding reason was not preserved'
  assert_grep 'assess runtime v2 before repository selection' "$private_home/data/charter.md" \
    'revised private charter lost accepted scope'

  restarted=$(MX_SPAWN_RECOVERY=1 run_mx spawn research --sub-orchestrator \
    --project selected --scope 'assess runtime v2 before repository selection' \
    --request-id research-restart --backend tmux --harness codex --json) \
    || fail 'stopped coordinator restart failed'
  printf '%s' "$restarted" | jq -e '
    .disposition == "running" and .scope_revision == 2 and
    .assignment_generation == 2 and .request_id == "research-restart"
  ' >/dev/null || fail 'restart output lost revised domain generation'
  model=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/research.meta")
  printf '%s' "$model" | jq -e '
    .attempt.generation == 2 and .runtime.endpoint != null and
    (.retained_executions | length) == 1 and .domain.scope_revision == 2
  ' >/dev/null || fail 'restart did not retain the prior attempt and fence it with a new generation'

  attempt_id=$(printf '%s' "$model" | jq -r .attempt.id)
  attempt_generation=$(printf '%s' "$model" | jq -r .attempt.generation)
  brief_revision=$(printf '%s' "$model" | jq -r .accepted_brief_revision)
  mkdir -p "$private_home/data/bound-implementation"
  printf '%s\n' 'implement only the explicitly bound repository' \
    > "$private_home/data/bound-implementation/brief.md"
  child=$(MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$private_home" \
    MX_STATE_OVERRIDE="$private_home/state" MX_DATA_OVERRIDE="$private_home/data" \
    MX_PROJECTS_OVERRIDE="$private_home/projects" MX_CONFIG_OVERRIDE="$private_home/config" \
    MX_REPORT_STATE_OVERRIDE="$HOME_DIR/state" MX_TASK_ID=research \
    MX_ATTEMPT_ID="$attempt_id" MX_ATTEMPT_GENERATION="$attempt_generation" \
    MX_BRIEF_REVISION="$brief_revision" MX_SPAWN_NO_GUARD=1 MX_FAKE_TMUX_LOG="$TMUX_LOG" \
    TMPDIR="$TASK_TMP" \
    PATH="$FAKEBIN:$PATH" "$MX_RUST_BIN" spawn bound-implementation "$selected" \
      --role implementer --output implementation --request-id bound-implementation-create \
      --backend tmux --harness codex) || fail 'bound domain implementation delegation failed'
  assert_contains "$child" 'spawned bound-implementation' \
    'bound implementation did not report a launched endpoint'
  worker_mcp=$(find "$TASK_TMP" -name report-mcp.json -print | while IFS= read -r path; do
    jq -e '.mcpServers.multplx_status.env.MX_TASK_ID == "bound-implementation"' "$path" >/dev/null 2>&1 && { printf '%s\n' "$path"; break; }
  done)
  assert_present "$worker_mcp" 'nested worker report MCP configuration was not installed'
  model=$(sed -n 's/^canonical_model=//p' "$private_home/state/bound-implementation.meta")
  child_attempt=$(printf '%s' "$model" | jq -r .attempt.id)
  child_generation=$(printf '%s' "$model" | jq -r .attempt.generation)
  child_revision=$(printf '%s' "$model" | jq -r .accepted_brief_revision)
  printf '%s' "$model" | jq -e --arg parent research '
    .parent_id == $parent and .artifact == "implementation" and
    .project.project_id != null and .runtime.endpoint != null
  ' >/dev/null || fail 'bound implementation lost its parent, project, or attempt authority'
  jq -e --arg home "$private_home" --arg state "$private_home/state" \
    --arg root "$RUNTIME" --arg bin "$MX_RUST_BIN" --arg attempt "$child_attempt" \
    --arg generation "$child_generation" --arg revision "$child_revision" '
    .mcpServers.multplx_status.env.MX_HOME == $home and
    .mcpServers.multplx_status.env.MX_REPORT_STATE_OVERRIDE == $state and
    .mcpServers.multplx_status.env.MX_RUST_SOURCE_ROOT == $root and
    .mcpServers.multplx_status.env.MX_RUST_BIN == $bin and
    .mcpServers.multplx_status.env.MX_ATTEMPT_ID == $attempt and
    .mcpServers.multplx_status.env.MX_ATTEMPT_GENERATION == $generation and
    .mcpServers.multplx_status.env.MX_BRIEF_REVISION == $revision
  ' "$worker_mcp" >/dev/null || {
    cat "$worker_mcp"
    fail 'nested worker report MCP crossed its coordinator parent or lost installed runtime identity'
  }
  pass 'stopped idea domain binds a canonical project and restarts at the revised generation'
}

test_queued_request_replay_keeps_one_domain_and_home() {
  local first second before after
  first=$(MX_HEADROOM_API_CAPACITY=0 MX_HEADROOM_SKIP_QUEUE=0 \
    run_mx spawn queued-domain --sub-orchestrator --idea queued-idea \
      --scope 'wait for shared capacity' --request-id queued-domain-create \
      --backend tmux --harness codex --json) || fail 'queued coordinator request failed'
  printf '%s' "$first" | jq -e '.disposition == "queued" and .endpoint == null' >/dev/null \
    || fail 'capacity-limited coordinator did not report a queued result'
  before=$(find "$TMP_ROOT" -name '.mx-daemon-home' -print | sort)
  second=$(MX_HEADROOM_API_CAPACITY=0 MX_HEADROOM_SKIP_QUEUE=0 \
    run_mx spawn queued-domain --sub-orchestrator --idea queued-idea \
      --scope 'wait for shared capacity' --request-id queued-domain-create \
      --backend tmux --harness codex --json) || fail 'queued coordinator replay failed'
  printf '%s' "$second" | jq -e '.disposition == "queued" and .request_id == "queued-domain-create"' \
    >/dev/null || fail 'queued replay did not recover the same request'
  after=$(find "$TMP_ROOT" -name '.mx-daemon-home' -print | sort)
  [ "$before" = "$after" ] || fail 'queued replay created a duplicate private home'
  assert_absent "$HOME_DIR/state/queued-domain.meta" 'queued coordinator published a running task record'
  # This case deliberately leaves a durable pending request. Settle it before
  # later cases share the same root admission queue; once it ages, coordinator
  # fairness correctly reserves the next slot and would make an unrelated
  # standing-domain assertion timing-dependent under coverage instrumentation.
  run_mx headroom --queue-cancel queued-domain-create >/dev/null \
    || fail 'queued coordinator fixture cleanup could not cancel its own request'
  assert_absent "$HOME_DIR/state/.dispatch-queue/queued-domain-create.request" \
    'queued coordinator fixture cleanup retained its dispatch request'
  pass 'queued coordinator replay preserves one request and one private home'
}

test_launch_failure_reports_retained_identity_and_exact_retry_recovers() {
  local out status error_file="$TMP_ROOT/failed-spawn.err" model retry
  set +e
  out=$(MX_FAKE_TMUX_FAIL_CREATE=1 run_mx spawn failed-domain --sub-orchestrator \
    --idea failure-idea --scope 'retain identity across launch failure' \
    --request-id failed-domain-create --backend tmux --harness codex --json \
    2>"$error_file")
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'injected coordinator launch failure unexpectedly succeeded'
  printf '%s' "$out" | jq -e '
    .status == "error" and .disposition == "retained" and
    .request_id == "failed-domain-create" and .coordinator_id == "failed-domain" and
    (.domain_id | startswith("domain-")) and (.home | length) > 0 and
    .endpoint == null and (.error | length) > 0
  ' >/dev/null || fail 'failed launch did not emit retained canonical identity'
  assert_grep 'error:' "$error_file" 'failed launch diagnostic was not separated onto stderr'
  model=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/failed-domain.meta")
  printf '%s' "$model" | jq -e '.runtime.endpoint == null and .domain.coordinator_id == "failed-domain"' \
    >/dev/null || fail 'failed launch lost its prelaunch canonical binding'

  retry=$(run_mx spawn failed-domain --sub-orchestrator --idea failure-idea \
    --scope 'retain identity across launch failure' --request-id failed-domain-create \
    --backend tmux --harness codex --json) || fail 'exact failed coordinator retry did not recover'
  printf '%s' "$retry" | jq -e '.status == "ok" and .disposition == "running" and .endpoint != null' \
    >/dev/null || fail 'exact failed coordinator retry did not report the recovered endpoint'
  pass 'launch failure reports retained identity and exact request retry recovers it'
}

test_unsupported_harness_preflight_has_no_home_or_registry_mutation() {
  local before after out status
  before=$(mx_test_filesystem_manifest "$HOME_DIR/data")
  set +e
  out=$(run_mx spawn unsupported --sub-orchestrator --idea future \
    --scope 'bounded research' --request-id unsupported-create \
    --backend tmux --harness unsupported 2>&1)
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'unsupported coordinator harness unexpectedly succeeded'
  assert_contains "$out" "no launch template for harness 'unsupported'" \
    'unsupported harness did not fail at provider preflight'
  after=$(mx_test_filesystem_manifest "$HOME_DIR/data")
  [ "$before" = "$after" ] || fail 'unsupported provider mutated coordinator home or registry data'
  assert_absent "$HOME_DIR/state/unsupported.meta" 'unsupported provider published task metadata'
  pass 'unsupported provider fails before coordinator home and registry mutation'
}

test_named_herdr_preflight_is_supported_and_failure_precedes_home_mutation() {
  local before after out status
  before=$(mx_test_filesystem_manifest "$HOME_DIR/data")
  set +e
  out=$(MX_HERDR_BIN="$TMP_ROOT/missing-herdr" run_mx spawn missing-herdr-domain \
    --sub-orchestrator --idea herdr-research --scope 'research with Herdr' \
    --request-id missing-herdr-domain-create --backend herdr --harness codex --json 2>&1)
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'missing Herdr coordinator backend unexpectedly succeeded'
  assert_contains "$out" 'coordinator backend preflight failed before home creation' \
    'missing Herdr backend did not fail at the coordinator preflight boundary'
  after=$(mx_test_filesystem_manifest "$HOME_DIR/data")
  [ "$before" = "$after" ] || fail 'failed Herdr preflight mutated coordinator home or registry data'
  assert_absent "$HOME_DIR/state/missing-herdr-domain.meta" \
    'failed Herdr preflight published coordinator metadata'

  out=$(MX_HERDR_BIN="$FAKEBIN/herdr" MX_HEADROOM_API_CAPACITY=0 MX_HEADROOM_SKIP_QUEUE=0 \
    run_mx spawn queued-herdr-domain --sub-orchestrator --idea herdr-research \
      --scope 'research with a supported Herdr provider' \
      --request-id queued-herdr-domain-create --backend herdr --harness codex --json) \
    || fail 'supported named Herdr coordinator was rejected'
  printf '%s' "$out" | jq -e '
    .status == "ok" and .disposition == "queued" and
    .coordinator_id == "queued-herdr-domain" and .endpoint == null
  ' >/dev/null || fail 'supported Herdr coordinator did not reach bounded admission'
  run_mx headroom --queue-cancel queued-herdr-domain-create >/dev/null \
    || fail 'queued Herdr coordinator fixture cleanup failed'
  pass 'named Herdr preflight supports the verified provider and refuses missing tooling before mutation'
}

test_inherited_settings_text_retry_and_retained_prepare_failure_are_truthful() {
  local first retry queued conflict model brief saved out status
  local error_file="$TMP_ROOT/inherited-retained.err"
  printf '%s\n' 'codex inherited-model xhigh' > "$HOME_DIR/config/daemon-harness"
  first=$(run_mx spawn inherited-domain --sub-orchestrator --idea inherited-idea \
    --scope 'inherit the coordinator launch settings' \
    --request-id inherited-domain-create --backend tmux) \
    || fail 'coordinator with inherited model and effort failed'
  assert_contains "$first" 'harness=codex' 'human coordinator result lost inherited harness'
  model=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/inherited-domain.meta")
  printf '%s' "$model" | jq -e '
    .runtime.endpoint != null and .domain.coordinator_id == "inherited-domain"
  ' >/dev/null || fail 'inherited coordinator metadata is incomplete'
  assert_grep 'model=inherited-model' "$HOME_DIR/state/inherited-domain.meta" \
    'coordinator did not inherit the configured persistent model'
  assert_grep 'effort=xhigh' "$HOME_DIR/state/inherited-domain.meta" \
    'coordinator did not inherit the configured persistent effort'

  retry=$(run_mx spawn inherited-domain --sub-orchestrator --idea inherited-idea \
    --scope 'inherit the coordinator launch settings' \
    --request-id inherited-domain-create --backend tmux) \
    || fail 'human-format exact coordinator retry failed'
  assert_contains "$retry" 'recovered=true' 'human exact retry did not report recovery'

  set +e
  queued=$(MX_QUEUED_MODEL="$model" run_mx spawn inherited-domain --sub-orchestrator \
    --idea inherited-idea --scope 'inherit the coordinator launch settings' \
    --request-id inherited-domain-queued-replay --backend tmux --json 2>"$error_file")
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'queued-model replay created parallel authority for an active coordinator'
  assert_grep 'metadata for inherited-domain already exists' "$error_file" \
    'exact queued-model replay did not stop at the existing-authority fence'

  conflict=$(printf '%s' "$model" | jq -c '.attempt.id = "conflicting-queued-attempt"')
  set +e
  out=$(MX_QUEUED_MODEL="$conflict" run_mx spawn inherited-domain --sub-orchestrator \
    --idea inherited-idea --scope 'inherit the coordinator launch settings' \
    --request-id inherited-domain-queued-conflict --backend tmux --json 2>"$error_file")
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'conflicting queued model replaced current coordinator identity'
  printf '%s' "$out" | jq -e '
    .status == "error" and .disposition == "retained" and
    (.error | contains("queued request conflicts"))
  ' >/dev/null || fail 'queued-model conflict did not retain and report coordinator identity'
  assert_grep 'queued request conflicts with current task identity' "$error_file" \
    'queued-model conflict omitted its identity-fence diagnostic'

  brief=$(printf '%s' "$model" | jq -r .accepted_brief_path)
  saved="$TMP_ROOT/inherited-accepted-brief"
  cp "$brief" "$saved"
  printf '%s\n' 'tampered accepted charter' > "$brief"
  set +e
  out=$(run_mx spawn inherited-domain --sub-orchestrator --idea inherited-idea \
    --scope 'inherit the coordinator launch settings' \
    --request-id inherited-domain-conflict --backend tmux --json 2>"$error_file")
  status=$?
  set -e
  mv "$saved" "$brief"
  [ "$status" -ne 0 ] || fail 'changed accepted charter unexpectedly resumed a coordinator'
  printf '%s' "$out" | jq -e '
    .status == "error" and .disposition == "retained" and
    .coordinator_id == "inherited-domain" and .endpoint == null and
    (.error | contains("accepted brief changed"))
  ' >/dev/null || fail 'prepare failure did not emit retained coordinator identity'
  assert_grep 'accepted brief changed' "$error_file" \
    'prepare failure did not explain the retained accepted-charter conflict'
  printf '%s\n' codex > "$HOME_DIR/config/daemon-harness"
  pass 'coordinator settings inherit, human retries recover exactly, and prepare failures retain identity'
}

test_prelaunch_publication_failure_reports_retained_identity() {
  local out status intent brief error_file="$TMP_ROOT/prelaunch-retained.err"
  set +e
  MX_SPAWN_FAULT=after-intent run_mx spawn prelaunch-domain --sub-orchestrator \
    --idea prelaunch-idea --scope 'retain a prelaunch publication failure' \
    --request-id prelaunch-domain-create --backend tmux --harness codex --json \
    >/dev/null 2>"$error_file"
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'after-intent fault did not interrupt coordinator launch'
  intent="$HOME_DIR/state/.spawn-prelaunch-domain.intent"
  assert_present "$intent" 'after-intent fault did not retain the frozen launch identity'
  brief=$(jq -r .accepted_brief_path "$intent")
  mkdir -p "$(dirname "$brief")"
  printf '%s\n' 'conflicting retained charter evidence' > "$brief"

  set +e
  out=$(MX_SPAWN_RECOVERY_REQUEST=prelaunch-domain-create \
    run_mx spawn prelaunch-domain --sub-orchestrator --idea prelaunch-idea \
      --scope 'retain a prelaunch publication failure' \
      --request-id prelaunch-domain-create --backend tmux --harness codex --json \
      2>"$error_file")
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail 'conflicting prelaunch charter unexpectedly published authority'
  printf '%s' "$out" | jq -e '
    .status == "error" and .disposition == "retained" and
    .request_id == "prelaunch-domain-create" and
    .coordinator_id == "prelaunch-domain" and .endpoint == null and
    (.error | contains("accepted coordinator charter changed"))
  ' >/dev/null || fail 'prelaunch publication failure omitted retained coordinator identity'
  assert_grep 'canonical coordinator binding could not be published' "$error_file" \
    'prelaunch publication failure omitted its canonical-binding diagnostic'
  pass 'prelaunch publication failure retains and reports the frozen coordinator identity'
}

test_repeatable_project_domain_is_canonical_and_standing() {
  local alpha="$TMP_ROOT/alpha" beta="$TMP_ROOT/beta" out retry model private_home window_count
  mx_git_init_commit "$alpha"
  mx_git_init_commit "$beta"
  run_mx project register "$alpha" --alias alpha >/dev/null || fail 'alpha registration failed'
  run_mx project register "$beta" --alias beta >/dev/null || fail 'beta registration failed'
  out=$(run_mx spawn delivery-domain --sub-orchestrator \
    --project alpha --project beta --scope 'coordinate the bounded two-repository delivery' \
    --persistent --request-id delivery-domain-create --backend tmux --harness codex) \
    || fail 'repeatable project coordinator spawn failed'
  assert_contains "$out" 'persistent=true' 'standing project domain lost persistence'
  model=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/delivery-domain.meta")
  printf '%s' "$model" | jq -e '
    .persistent == true and .private_home == true and
    (.domain.projects | length) == 2 and
    .domain.idea_id == null and
    .home_allocation.generation == 1
  ' >/dev/null || fail 'project domain lost canonical projects or private-home lease'
  private_home=$(printf '%s' "$model" | jq -r .persistent_home)
  assert_absent "$private_home/projects/alpha" 'coordinator home copied the alpha repository'
  assert_absent "$private_home/projects/beta" 'coordinator home copied the beta repository'
  jq -e --arg alpha "$(cd "$alpha" && pwd -P)" --arg beta "$(cd "$beta" && pwd -P)" '
    ([.projects[].checkouts[].canonical_path] | index($alpha)) != null and
    ([.projects[].checkouts[].canonical_path] | index($beta)) != null
  ' "$private_home/data/projects.json" >/dev/null || fail 'private home lost borrowed canonical project references'
  window_count=$(grep -c '^new-window ' "$TMUX_LOG")
  retry=$(run_mx spawn delivery-domain --sub-orchestrator \
    --project alpha --project beta --scope 'coordinate the bounded two-repository delivery' \
    --persistent --request-id delivery-domain-create --backend tmux --harness codex) \
    || fail 'human-format project coordinator retry failed'
  assert_contains "$retry" 'recovered=true' 'human project coordinator retry lost recovery disposition'
  [ "$(grep -c '^new-window ' "$TMUX_LOG")" -eq "$window_count" ] \
    || fail 'human project coordinator retry created a duplicate endpoint'
  pass 'repeatable project options bind a standing domain without copying repositories'
}

test_idea_spawn_and_repeat_adopt_one_coordinator
test_overlapping_domains_share_one_project_without_exclusivity
test_stopped_idea_domain_binds_project_and_restarts_new_generation
test_queued_request_replay_keeps_one_domain_and_home
test_launch_failure_reports_retained_identity_and_exact_retry_recovers
test_repeatable_project_domain_is_canonical_and_standing
test_unsupported_harness_preflight_has_no_home_or_registry_mutation
test_named_herdr_preflight_is_supported_and_failure_precedes_home_mutation
test_inherited_settings_text_retry_and_retained_prepare_failure_are_truthful
test_prelaunch_publication_failure_reports_retained_identity
