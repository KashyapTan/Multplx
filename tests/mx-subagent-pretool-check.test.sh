#!/usr/bin/env bash
# Native delegation freedom and retired-hook compatibility checks.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

export MX_RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}
CHECK="$ROOT/bin/mx-subagent-pretool-check.sh"
SETTINGS="$ROOT/.claude/settings.json"

test_every_tool_shape_is_allowed() {
  local tool output error rc
  output=$(mktemp)
  error=$(mktemp)
  for tool in Agent TaskCreate SpawnWorker DelegateTask WorkflowRun RemoteExec mcp__tracker__create_task Bash; do
    rc=0
    MX_ALLOW_SUBAGENT=0 "$CHECK" --claude --tool "$tool" >"$output" 2>"$error" || rc=$?
    [ "$rc" -eq 0 ] || fail "compatibility entry denied $tool"
    [ ! -s "$output" ] && [ ! -s "$error" ] || fail "compatibility allow for $tool wrote hook output"
  done
  rm -f "$output" "$error"
  pass "native delegation tool shapes are allowed without an escape flag"
}

test_stdin_transport_is_an_allowing_no_op() {
  local output rc=0
  output=$(printf '%s' '{"tool_name":"Agent","tool_input":{"prompt":"go"}}' | "$CHECK" --claude) || rc=$?
  [ "$rc" -eq 0 ] || fail "legacy stdin transport denied native delegation"
  [ -z "$output" ] || fail "legacy stdin transport wrote a decision"
  rc=0
  output=$(printf '%s' 'not-json' | "$CHECK") || rc=$?
  [ "$rc" -eq 0 ] && [ -z "$output" ] || fail "compatibility entry depends on JSON tooling"
  pass "old hook transports remain harmless allowing no-ops"
}

test_compatibility_grammar_stays_bounded() {
  "$CHECK" --help | grep -F 'Native delegation is allowed' >/dev/null || fail "help does not explain retired behavior"
  if "$CHECK" --tool >/dev/null 2>&1; then fail "missing --tool value was accepted"; fi
  if "$CHECK" --unknown >/dev/null 2>&1; then fail "unknown option was accepted"; fi
  pass "retired entry keeps a bounded compatibility grammar"
}

test_claude_settings_keep_supervision_without_delegation_fence() {
  jq -e '
    [.hooks.PreToolUse[] | .hooks[].command]
      | all(contains("mx-subagent-pretool-check.sh") | not)
  ' "$SETTINGS" >/dev/null || fail "Claude settings still register the delegation fence"
  jq -e '
    [.hooks.PreToolUse[] | select(.matcher == "Bash") | .hooks[].command]
      == [
        "\"$CLAUDE_PROJECT_DIR\"/bin/mx-arm-pretool-check.sh --claude",
        "\"$CLAUDE_PROJECT_DIR\"/bin/mx-cd-pretool-check.sh --claude"
      ]
  ' "$SETTINGS" >/dev/null || fail "Claude Bash supervision hooks changed"
  jq -e '.hooks.Stop[0].hooks[0].command | contains("mx-turnend-guard.sh")' "$SETTINGS" >/dev/null \
    || fail "Claude turn-end ownership changed"
  pass "Claude keeps supervision hooks and ships no delegation fence"
}

test_native_observers_are_non_authoritative_and_nonblocking() {
  local pi="$ROOT/.pi/extensions/mx-native-delegation-observe.ts"
  jq -e '
    .hooks.SubagentStart[0].hooks[0]
      | .async == true and (.command | contains("mx-native-observe.sh"))
  ' "$SETTINGS" >/dev/null || fail "Claude native-start observer is missing or blocking"
  jq -e '
    .hooks.SubagentStop[0].hooks[0]
      | .async == true and (.command | contains("--event result"))
  ' "$SETTINGS" >/dev/null || fail "Claude native-result observer is missing or blocking"
  grep -F 'pi.on("tool_call"' "$pi" >/dev/null || fail "Pi delegation start event is not observed"
  grep -F 'pi.on("tool_result"' "$pi" >/dev/null || fail "Pi delegation result event is not observed"
  grep -F 'child.stdin.end(JSON.stringify(payload))' "$pi" >/dev/null \
    || fail "Pi observer does not forward structured lifecycle evidence"
  ! grep -Eq 'block:[[:space:]]*true|process\.exit' "$pi" \
    || fail "Pi delegation observer can deny a native tool call"
  pass "Claude and Pi native observers record lifecycle without becoming delegation fences"
}

test_every_tool_shape_is_allowed
test_stdin_transport_is_an_allowing_no_op
test_compatibility_grammar_stays_bounded
test_claude_settings_keep_supervision_without_delegation_fence
test_native_observers_are_non_authoritative_and_nonblocking
