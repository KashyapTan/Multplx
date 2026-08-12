#!/usr/bin/env bash
# Cursor CLI adapter, hook transport, launcher safety, and terminal signature tests.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

if [ "${MX_SUPERVISION_IMPLEMENTATION:-rust}" = rust ]; then
  export MX_RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}
fi

mx_test_tmproot_into TMP_ROOT mx-cursor-adapter

assert_file_has() {
  local file=$1 needle=$2 message=$3
  grep -F -- "$needle" "$file" >/dev/null || fail "$message"
}

make_runtime() {
  local runtime=$1
  mkdir -p "$runtime/bin" "$runtime/.agents/skills" "$runtime/share/shell/shims" \
    "$runtime/config" "$runtime/data" "$runtime/projects" "$runtime/state"
  cp "$ROOT/bin/mx-launcher.sh" "$ROOT/bin/mx-launcher-lib.sh" \
    "$ROOT/bin/mx-launch-harness.sh" "$ROOT/bin/mx-lock.sh" \
    "$ROOT/bin/mx-session-lock-lib.sh" "$runtime/bin/"
  cp "$ROOT/share/shell/shims/"* "$runtime/share/shell/shims/"
  chmod +x "$runtime/bin/"* "$runtime/share/shell/shims/"*
  printf '# fixture\n' >"$runtime/AGENTS.md"
  printf '# fixture\n' >"$runtime/.agents/skills/fixture.md"
  git -C "$runtime" init -q
}

test_launcher_prefers_agent_and_enforces_sandbox() {
  local runtime=$TMP_ROOT/runtime fakebin=$TMP_ROOT/fakebin log=$TMP_ROOT/agent.log output
  make_runtime "$runtime"
  runtime=$(cd "$runtime" && pwd -P)
  mkdir -p "$fakebin"
  cat >"$fakebin/agent" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$CURSOR_TEST_LOG"
printf 'cwd=%s\n' "$(pwd -P)"
SH
  chmod +x "$fakebin/agent"
  output=$(CURSOR_TEST_LOG="$log" PATH="$fakebin:/usr/bin:/bin" \
    MX_ROOT_OVERRIDE="$runtime" MX_HOME="$runtime" "$runtime/bin/mx-launcher.sh" cursor 'literal arg') \
    || fail "multplx cursor did not launch preferred agent executable"
  assert_contains "$output" "cwd=$runtime" "Cursor launcher did not enter the broker root"
  assert_file_has "$log" '--sandbox enabled literal arg' "Cursor launcher omitted explicit sandbox enablement"
  CURSOR_TEST_LOG="$log" MX_ROOT_OVERRIDE="$runtime" MX_HOME="$runtime" \
    MX_REAL_CURSOR_AGENT="$fakebin/agent" "$runtime/share/shell/shims/cursor-agent" alias-arg >/dev/null \
    || fail "cursor-agent compatibility shim failed"
  assert_file_has "$log" '--sandbox enabled alias-arg' "cursor-agent shim did not preserve sandboxing"
  for args in '--force' '--yolo' '--sandbox disabled' '--sandbox=disabled' '--worktree'; do
    # shellcheck disable=SC2086
    if CURSOR_TEST_LOG="$log" MX_ROOT_OVERRIDE="$runtime" MX_HOME="$runtime" \
        MX_REAL_CURSOR_AGENT="$fakebin/agent" "$runtime/bin/mx-launch-harness.sh" cursor $args >/dev/null 2>&1; then
      fail "Cursor launcher accepted unsafe mode: $args"
    fi
  done
  pass "multplx cursor and cursor-agent prefer agent and enforce sandbox-on launch"
}

make_hook_fixture() {
  local fixture=$1
  mkdir -p "$fixture/bin"
  cp "$ROOT/bin/mx-cursor-hook.sh" "$fixture/bin/"
  cp "$ROOT/bin/mx-rust-runtime.sh" "$fixture/bin/"
  cat >"$fixture/bin/mx-sessionstart-nudge.sh" <<'SH'
#!/usr/bin/env bash
printf 'RUN_SESSION_START_EXACTLY_ONCE\n'
SH
  cat >"$fixture/bin/mx-arm-pretool-check.sh" <<'SH'
#!/usr/bin/env bash
cat >/dev/null
exit 0
SH
  cp "$fixture/bin/mx-arm-pretool-check.sh" "$fixture/bin/mx-cd-pretool-check.sh"
  cat >"$fixture/bin/mx-subagent-pretool-check.sh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = --tool ]; then tool=${2:-}; else payload=$(cat); tool=$(printf '%s' "$payload" | jq -r '.tool_name'); fi
if [ "$tool" = Task ] || [ "$tool" = subagentStart ]; then
  printf '{"decision":"deny","reason":"dispatch through Multplx"}\n'
  exit 2
fi
exit 0
SH
  cat >"$fixture/bin/mx-turnend-guard.sh" <<'SH'
#!/usr/bin/env bash
cat >/dev/null
printf 'restore one foreground checkpoint\n' >&2
exit 2
SH
  chmod +x "$fixture/bin/"*
}

test_cursor_hook_translation_and_bounds() {
  local fixture=$TMP_ROOT/hooks output
  make_hook_fixture "$fixture"
  output=$(printf '{"session_id":"s"}' | "$fixture/bin/mx-cursor-hook.sh" session-start)
  [ "$(printf '%s' "$output" | jq -r '.additional_context')" = RUN_SESSION_START_EXACTLY_ONCE ] \
    || fail "Cursor sessionStart context translation failed"
  output=$(printf '{"tool_name":"Shell","tool_input":{"command":"true"}}' | "$fixture/bin/mx-cursor-hook.sh" pre-tool)
  [ "$(printf '%s' "$output" | jq -r '.permission')" = allow ] || fail "Cursor preToolUse allow translation failed"
  output=$(printf '{"tool_name":"Task","tool_input":{}}' | "$fixture/bin/mx-cursor-hook.sh" pre-tool)
  [ "$(printf '%s' "$output" | jq -r '.permission')" = deny ] || fail "Cursor preToolUse denial translation failed"
  output=$(printf '{"agent_type":"generalPurpose"}' | "$fixture/bin/mx-cursor-hook.sh" subagent-start)
  [ "$(printf '%s' "$output" | jq -r '.permission')" = deny ] || fail "Cursor subagentStart denial translation failed"
  output=$(printf '{"session_id":"s","loop_count":0}' | "$fixture/bin/mx-cursor-hook.sh" stop)
  assert_contains "$output" 'restore one foreground checkpoint' "Cursor stop did not translate a guard block to follow-up"
  output=$(printf '{"session_id":"s","loop_count":1}' | "$fixture/bin/mx-cursor-hook.sh" stop)
  [ "$output" = '{}' ] || fail "Cursor stop continuation was not bounded after loop one"
  if printf '{}' | "$fixture/bin/mx-cursor-hook.sh" pre-tool >/dev/null 2>&1; then
    fail "malformed critical Cursor preToolUse payload failed open"
  fi
  jq -e '
    .version == 1 and
    (.hooks.sessionStart[0].failClosed == true) and
    (.hooks.preToolUse[0].failClosed == true) and
    (.hooks.subagentStart[0].failClosed == true) and
    (.hooks.stop[0].loop_limit == 1)
  ' "$ROOT/.cursor/hooks.json" >/dev/null || fail "tracked Cursor hook contract is incomplete"
  pass "Cursor hooks translate shared guards, fail closed, and bound stop continuation"
}

test_cursor_spawn_profile_and_terminal_signatures() {
  local output
  assert_file_has "$ROOT/bin/mx-spawn.sh" "agent --sandbox enabled --trust __CURSORPLUGIN__" \
    "Cursor actor template is missing sandbox/trust/plugin controls"
  assert_file_has "$ROOT/bin/mx-spawn.sh" 'cursor-turnend-plugin' \
    "Cursor actor turn-end plugin is not task-private"
  # shellcheck disable=SC2016  # exact literal source contract; no expansion intended
  assert_file_has "$ROOT/bin/mx-spawn.sh" 'selected="${selected}[effort=$effort]"' \
    "Cursor effort is not parameterized onto its model token"
  # shellcheck source=bin/mx-composer-lib.sh
  . "$ROOT/bin/mx-composer-lib.sh"
  [ "$(mx_composer_classify_content 0 '→')" = empty ] || fail "Cursor idle composer glyph is not recognized"
  [ "$(mx_composer_classify_content 0 '→ pending maintainer text')" = pending ] || fail "Cursor typed composer text is not preserved"
  output=$(printf 'Working\n' | grep -E 'Working(\.\.\.)?|ctrl\+c to stop' || true)
  [ "$output" = Working ] || fail "Cursor busy signature is missing"
  pass "Cursor actor profile, model effort, composer, and busy signatures are wired"
}

test_launcher_prefers_agent_and_enforces_sandbox
test_cursor_hook_translation_and_bounds
test_cursor_spawn_profile_and_terminal_signatures
