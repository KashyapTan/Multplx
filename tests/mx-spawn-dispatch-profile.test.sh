#!/usr/bin/env bash
# Behavior tests for mx-spawn.sh concrete dispatch profile flags.
#
# These tests drive mx-spawn through meta writing and launch construction with a
# fake tmux pane and a real isolated git worktree. The fake tmux captures the
# literal launch command sent with `tmux send-keys -l`, so assertions pin the
# command broker would run without starting any real harness.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SPAWN="$ROOT/bin/mx-spawn.sh"
TMP_ROOT=$(mx_test_tmproot mx-spawn-dispatch-profile)

make_spawn_fakebin() {
  local dir=$1 fakebin
  fakebin=$(mx_fakebin "$dir")
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "$*" in
  *"#{pane_current_path}"*) printf '%s\n' "${MX_FAKE_PANE_PATH:-}"; exit 0 ;;
esac
case "${1:-}" in
  display-message) printf 'broker\n'; exit 0 ;;
  list-windows) exit 0 ;;
  new-window) printf '@1\n'; exit 0 ;;
  has-session|new-session|kill-window) exit 0 ;;
  send-keys)
    if [ -n "${MX_FAKE_LAUNCH_LOG:-}" ]; then
      prev=
      for a in "$@"; do
        if [ "$prev" = "-l" ]; then
          printf '%s\n' "$a" >> "$MX_FAKE_LAUNCH_LOG"
        fi
        prev=$a
      done
    fi
    exit 0
    ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  mx_fake_exit0 "$fakebin" treehouse
  printf '%s\n' "$fakebin"
}

make_spawn_case() {
  local name=$1 harness=$2 case_dir home proj wt fakebin launchlog id
  shift 2
  case_dir="$TMP_ROOT/$name"
  home="$case_dir/home"
  proj="$case_dir/project"
  wt="$case_dir/wt"
  launchlog="$case_dir/launch.log"
  fakebin=$(make_spawn_fakebin "$case_dir/fake")
  mkdir -p "$home/data" "$home/projects" "$home/state" "$home/config"
  printf '%s\n' "$harness" > "$home/config/actor-harness"
  mx_git_worktree "$proj" "$wt" "wt-$name"
  touch "$home/state/.last-watcher-beat"
  for id in "$@"; do
    mkdir -p "$home/data/$id"
    printf 'brief for %s\n' "$id" > "$home/data/$id/brief.md"
  done
  printf '%s\n' "$case_dir|$home|$proj|$wt|$fakebin|$launchlog"
}

enable_dispatch_profile() {
  local home=$1
  printf '%s\n' '{"rules":[{"when":"current events","use":{"harness":"claude","model":"claude-opus-4-6","effort":"high"}}],"default":{"harness":"codex","model":"gpt-5","effort":"medium"}}' \
    > "$home/config/actor-dispatch.json"
}

make_seeded_daemon_home() {
  local home=$1 id=$2
  mkdir -p "$home/bin" "$home/data"
  printf '# Multplx\n' > "$home/AGENTS.md"
  printf '%s\n' "$id" > "$home/.mx-daemon-home"
  printf 'charter for %s\n' "$id" > "$home/data/charter.md"
}

run_spawn() {
  local home=$1 wt=$2 fakebin=$3 launchlog=$4
  shift 4
  : > "$launchlog"
  MX_ROOT_OVERRIDE='' MX_HOME="$home" \
    MX_STATE_OVERRIDE="$home/state" MX_DATA_OVERRIDE="$home/data" \
    MX_PROJECTS_OVERRIDE="$home/projects" MX_CONFIG_OVERRIDE="$home/config" \
    MX_SPAWN_NO_GUARD=1 MX_FAKE_PANE_PATH="$wt" TMUX="fake,1,0" \
    MX_FAKE_LAUNCH_LOG="$launchlog" PATH="$fakebin:$PATH" \
    "$SPAWN" "$@" 2>&1
}

read_case_record() {
  IFS='|' read -r CASE_DIR HOME_DIR PROJ_DIR WT_DIR FAKEBIN_DIR LAUNCH_LOG <<EOF
$1
EOF
}

assert_meta_profile() {
  local meta=$1 harness=$2 model=$3 effort=$4
  assert_grep "harness=$harness" "$meta" "meta missing harness=$harness"
  assert_grep "model=$model" "$meta" "meta missing model=$model"
  assert_grep "effort=$effort" "$meta" "meta missing effort=$effort"
}

assert_report_binding() {
  local launch=$1 home=$2 state=$3 id=$4 state_real home_real
  state_real=$(cd "$state" && pwd -P)
  home_real=$(cd "$home" && pwd -P)
  case "$launch" in
    *"MX_HOME='$home'"*|*"MX_HOME='$home_real'"*) : ;;
    *) fail "launch missing the actor's operational home"$'\n'"$launch" ;;
  esac
  assert_contains "$launch" "MX_TASK_ID='$id'" "launch missing the immutable task binding"
  assert_contains "$launch" "MX_REPORT_STATE_OVERRIDE='$state_real'" \
    "launch missing the exact parent status-state binding"
}

assert_remote_write_credentials_removed() {
  local launch=$1
  assert_contains "$launch" 'env -u GH_TOKEN -u GITHUB_TOKEN -u GH_ENTERPRISE_TOKEN -u GITHUB_ENTERPRISE_TOKEN' \
    "agent launch does not remove ambient GitHub tokens"
  assert_contains "$launch" '-u GH_CONFIG_DIR -u SSH_AUTH_SOCK -u MX_DELIVERY_GH_TOKEN -u MX_DELIVERY_GH_CONFIG_DIR' \
    "agent launch can inherit a GitHub config, SSH agent, or delivery credential"
  assert_contains "$launch" 'GH_PROMPT_DISABLED=1 GIT_TERMINAL_PROMPT=0' \
    "agent launch permits an interactive credential fallback"
  assert_contains "$launch" 'GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0=' \
    "agent launch retains ambient git credential helpers"
  assert_contains "$launch" 'GIT_CONFIG_KEY_1=remote.origin.pushurl GIT_CONFIG_VALUE_1=/dev/null/multplx-agent-no-push' \
    "agent launch leaves origin push-capable"
  assert_contains "$launch" 'IdentityAgent=none -o IdentitiesOnly=yes -o IdentityFile=/dev/null' \
    "agent launch retains ambient SSH write identity"
}

assert_claude_report_mcp_config() {
  local launch=$1 home=$2 state=$3 id=$4 config="/tmp/mx-$4/report-mcp.json" state_real
  state_real=$(cd "$state" && pwd -P)
  assert_contains "$launch" "--mcp-config '$config'" \
    "claude launch missing the per-task MCP config"
  assert_present "$config" "claude per-task MCP config was not generated"
  jq -e \
    --arg id "$id" --arg home "$home" --arg state "$state_real" \
    '.mcpServers.multplx_status |
      .type == "stdio" and
      (.command | endswith("/bin/mx-report-mcp")) and
      (.args | length == 0) and
      .env.MX_TASK_ID == $id and
      .env.MX_HOME == $home and
      .env.MX_REPORT_STATE_OVERRIDE == $state' \
    "$config" >/dev/null || fail "claude MCP config has the wrong task-bound server entry"
}

test_no_profile_keeps_claude_profile_defaults() {
  local rec id out status launch
  id=profile-off-z1
  rec=$(make_spawn_case profile-off claude "$id")
  read_case_record "$rec"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR")
  status=$?
  expect_code 0 "$status" "claude spawn without profile flags should succeed"
  assert_contains "$out" "spawned $id harness=claude" "spawn did not report claude"
  assert_meta_profile "$HOME_DIR/state/$id.meta" claude default default

  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_remote_write_credentials_removed "$launch"
  assert_claude_report_mcp_config "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false claude --dangerously-skip-permissions" \
    "no-profile claude launch lost its harness flags"
  assert_contains "$launch" "mx-operational-input.sh' encode launch-brief" \
    "no-profile claude launch did not use the canonical launch kind"
  pass "no --model/--effort records defaults and types the claude launch instructions"
}

test_active_dispatch_profile_requires_explicit_harness_for_ship() {
  local rec id out status
  id=profile-required-delivery-z11
  rec=$(make_spawn_case profile-required-delivery claude "$id")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR")
  status=$?
  expect_code 1 "$status" "delivery spawn without explicit harness should fail when dispatch profiles are active"
  assert_contains "$out" "config/actor-dispatch.json is active - pass an explicit harness resolved from the dispatch rules" \
    "spawn did not explain the dispatch-profile backstop"
  assert_absent "$HOME_DIR/state/$id.meta" "delivery refusal should happen before meta is written"
  pass "active actor-dispatch profile requires an explicit harness for delivery spawns"
}

test_active_dispatch_profile_requires_explicit_harness_for_scout() {
  local rec id out status
  id=profile-required-scout-z12
  rec=$(make_spawn_case profile-required-scout claude "$id")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR" --scout)
  status=$?
  expect_code 1 "$status" "scout spawn without explicit harness should fail when dispatch profiles are active"
  assert_contains "$out" "config/actor-dispatch.json is active - pass an explicit harness resolved from the dispatch rules" \
    "scout refusal did not explain the dispatch-profile backstop"
  assert_absent "$HOME_DIR/state/$id.meta" "scout refusal should happen before meta is written"
  pass "active actor-dispatch profile requires an explicit harness for scout spawns"
}

test_active_dispatch_profile_allows_explicit_harness() {
  local rec id out status launch
  id=profile-explicit-z13
  rec=$(make_spawn_case profile-explicit claude "$id")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" \
    "$id" "$PROJ_DIR" --harness codex --model gpt-5 --effort high)
  status=$?
  expect_code 0 "$status" "explicit harness should satisfy active dispatch-profile requirement"
  assert_contains "$out" "spawned $id harness=codex" "spawn did not report explicit codex harness"
  assert_meta_profile "$HOME_DIR/state/$id.meta" codex gpt-5 high
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "codex -c 'mcp_servers.multplx_status=" \
    "explicit codex launch did not receive the report_status MCP server"
  assert_contains "$launch" "--model 'gpt-5' -c 'model_reasoning_effort=\"high\"' --dangerously-bypass-approvals-and-sandbox" \
    "explicit harness launch did not thread model and effort"
  pass "active actor-dispatch profile allows an explicit resolved harness"
}

test_active_dispatch_profile_allows_positional_harness() {
  local rec id out status
  id=profile-positional-z14
  rec=$(make_spawn_case profile-positional claude "$id")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" \
    "$id" "$PROJ_DIR" codex --model gpt-5 --effort high)
  status=$?
  expect_code 0 "$status" "positional harness should satisfy active dispatch-profile requirement"
  assert_contains "$out" "spawned $id harness=codex" "spawn did not report positional codex harness"
  assert_meta_profile "$HOME_DIR/state/$id.meta" codex gpt-5 high
  pass "active actor-dispatch profile allows the legacy positional harness form"
}

test_active_dispatch_profile_allows_raw_launch_command() {
  local rec id out status launch
  id=profile-raw-z15
  rec=$(make_spawn_case profile-raw claude "$id")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" \
    "$id" "$PROJ_DIR" "custom-agent --flag")
  status=$?
  expect_code 0 "$status" "raw launch command should satisfy active dispatch-profile requirement"
  assert_contains "$out" "spawned $id harness=custom-agent" "spawn did not report raw command harness"
  assert_meta_profile "$HOME_DIR/state/$id.meta" custom-agent default default
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "custom-agent --flag" \
    "raw launch command body changed"$'\n'"actual: $launch"
  pass "active actor-dispatch profile allows the raw launch-command escape hatch"
}

test_claude_threads_model_and_effort() {
  local rec id out status launch
  id=profile-claude-z2
  rec=$(make_spawn_case profile-claude claude "$id")
  read_case_record "$rec"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR" --model sonnet --effort high)
  status=$?
  expect_code 0 "$status" "claude spawn with profile flags should succeed"
  assert_meta_profile "$HOME_DIR/state/$id.meta" claude sonnet high
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_claude_report_mcp_config "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "--model 'sonnet' --effort 'high'" \
    "claude launch did not thread model and effort flags"
  pass "claude receives --model and --effort profile flags"
}

test_codex_threads_model_and_effort() {
  local rec id out status launch
  id=profile-codex-z3
  rec=$(make_spawn_case profile-codex codex "$id")
  read_case_record "$rec"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR" --model gpt-5 --effort high)
  status=$?
  expect_code 0 "$status" "codex spawn with profile flags should succeed"
  assert_meta_profile "$HOME_DIR/state/$id.meta" codex gpt-5 high
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "codex -c 'mcp_servers.multplx_status=" \
    "codex launch did not receive the report_status MCP server"
  assert_contains "$launch" "--model 'gpt-5' -c 'model_reasoning_effort=\"high\"' --dangerously-bypass-approvals-and-sandbox" \
    "codex launch did not thread model and reasoning effort config"
  pass "codex receives --model and model_reasoning_effort profile flags"
}

test_codex_omits_invalid_max_effort() {
  local rec id out status launch
  id=profile-codex-max-z4
  rec=$(make_spawn_case profile-codex-max codex "$id")
  read_case_record "$rec"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR" --model gpt-5 --effort max)
  status=$?
  expect_code 0 "$status" "codex spawn with unsupported max effort should omit the effort flag"
  assert_meta_profile "$HOME_DIR/state/$id.meta" codex gpt-5 max
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "codex -c 'mcp_servers.multplx_status=" \
    "codex max-effort launch lost the report_status MCP server"
  assert_contains "$launch" "--model 'gpt-5' --dangerously-bypass-approvals-and-sandbox" \
    "codex launch did not preserve the model flag when max effort was omitted"
  assert_not_contains "$launch" "model_reasoning_effort" "codex launch must omit unsupported max reasoning effort"
  pass "codex omits unsupported max effort instead of passing a bad config value"
}

test_pi_threads_model_and_max_effort() {
  local rec id out status launch
  id=profile-pi-z8
  rec=$(make_spawn_case profile-pi pi "$id")
  read_case_record "$rec"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR" \
    --model openai-codex/gpt-5.6-sol --effort max)
  status=$?
  expect_code 0 "$status" "pi spawn with max effort should succeed"
  assert_meta_profile "$HOME_DIR/state/$id.meta" pi openai-codex/gpt-5.6-sol max
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$HOME_DIR" "$HOME_DIR/state" "$id"
  assert_contains "$launch" "pi --model 'openai-codex/gpt-5.6-sol' --thinking 'max' -e" \
    "pi launch did not thread the requested model and max thinking level"
  assert_not_contains "$launch" "MX_MULTPLX_PI_LAUNCH_BRIEF=" \
    "pi launch still exports the removed Calm input-reroute binding"
  assert_contains "$launch" "mx-operational-input.sh' encode launch-brief" \
    "pi launch lost the canonical typed launch-brief envelope"
  pass "pi receives --model and --thinking max profile flags"
}

test_cursor_private_plugin_and_effort_model() {
  local rec id out status launch plugin
  id=profile-cursor-z17
  rec=$(make_spawn_case profile-cursor cursor "$id")
  read_case_record "$rec"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$PROJ_DIR" \
    --model composer-2 --effort high)
  status=$?
  expect_code 0 "$status" "cursor spawn with profile flags should succeed"
  assert_meta_profile "$HOME_DIR/state/$id.meta" cursor composer-2 high
  launch=$(cat "$LAUNCH_LOG")
  plugin="/tmp/mx-$id/cursor-turnend-plugin"
  assert_contains "$launch" "agent --sandbox enabled --trust '$plugin' --model 'composer-2[effort=high]'" \
    "cursor launch did not preserve sandbox, scoped trust, and effort model token"
  assert_present "$plugin/.cursor-plugin/plugin.json" "cursor private plugin manifest missing"
  assert_present "$plugin/hooks/hooks.json" "cursor private plugin hook map missing"
  assert_present "$plugin/hooks/stop.sh" "cursor private stop hook missing"
  assert_grep "$id.turn-ended'" "$plugin/hooks/stop.sh" \
    "cursor private stop hook is not bound to the task turn-end marker"
  [ "$(stat -f '%Lp' "$plugin/hooks/stop.sh")" = 700 ] \
    || fail "cursor private stop hook is not executable owner-only"
  pass "cursor spawn enforces sandbox, scoped trust, private stop plugin, and effort model token"
}

test_batch_forwards_shared_profile_flags() {
  local rec id1 id2 out status
  id1=profile-batch-a-z9
  id2=profile-batch-b-z10
  rec=$(make_spawn_case profile-batch claude "$id1" "$id2")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" \
    "$id1=$PROJ_DIR" "$id2=$PROJ_DIR" --harness codex --model gpt-5 --effort high)
  status=$?
  expect_code 0 "$status" "batch spawn with shared profile flags should succeed"
  assert_contains "$out" "spawned $id1 harness=codex" "first batch task did not use shared harness"
  assert_contains "$out" "spawned $id2 harness=codex" "second batch task did not use shared harness"
  assert_meta_profile "$HOME_DIR/state/$id1.meta" codex gpt-5 high
  assert_meta_profile "$HOME_DIR/state/$id2.meta" codex gpt-5 high
  pass "batch dispatch forwards shared --harness, --model, and --effort to every pair"
}

test_active_dispatch_profile_does_not_block_daemon_launch() {
  local rec id sm out status launch
  id=profile-daemon-z16
  rec=$(make_spawn_case profile-daemon codex "$id")
  read_case_record "$rec"
  enable_dispatch_profile "$HOME_DIR"
  sm="$CASE_DIR/daemon-home"
  make_seeded_daemon_home "$sm" "$id"

  out=$(run_spawn "$HOME_DIR" "$WT_DIR" "$FAKEBIN_DIR" "$LAUNCH_LOG" "$id" "$sm" --daemon)
  status=$?
  expect_code 0 "$status" "daemon spawn should be exempt from the dispatch-profile explicit harness requirement"
  assert_contains "$out" "spawned $id harness=codex kind=daemon" "daemon launch did not use daemon harness resolution"
  assert_grep "kind=daemon" "$HOME_DIR/state/$id.meta" "daemon meta missing kind=daemon"
  assert_meta_profile "$HOME_DIR/state/$id.meta" codex default default
  launch=$(cat "$LAUNCH_LOG")
  assert_report_binding "$launch" "$sm" "$HOME_DIR/state" "$id"
  assert_remote_write_credentials_removed "$launch"
  assert_contains "$launch" "codex -c 'mcp_servers.multplx_status=" \
    "daemon codex launch did not receive the parent-bound report_status MCP server"
  pass "active actor-dispatch profile does not block daemon launches"
}

test_no_profile_keeps_claude_profile_defaults
test_active_dispatch_profile_requires_explicit_harness_for_ship
test_active_dispatch_profile_requires_explicit_harness_for_scout
test_active_dispatch_profile_allows_explicit_harness
test_active_dispatch_profile_allows_positional_harness
test_active_dispatch_profile_allows_raw_launch_command
test_claude_threads_model_and_effort
test_codex_threads_model_and_effort
test_codex_omits_invalid_max_effort
test_pi_threads_model_and_max_effort
test_cursor_private_plugin_and_effort_model
test_batch_forwards_shared_profile_flags
test_active_dispatch_profile_does_not_block_daemon_launch

echo "# all mx-spawn-dispatch-profile tests passed"
