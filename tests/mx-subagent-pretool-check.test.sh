#!/usr/bin/env bash
# Native delegation freedom and the narrow remote PR merge backstop.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

export MX_RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}
CHECK="$ROOT/bin/mx-subagent-pretool-check.sh"
SETTINGS="$ROOT/.claude/settings.json"
TMP_ROOT=$(mx_test_tmproot mx-subagent-pretool-check)

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

test_stdin_transport_guards_only_remote_merge_actions() {
  local output rc=0
  output=$(printf '%s' '{"tool_name":"Agent","tool_input":{"prompt":"go"}}' | "$CHECK" --claude) || rc=$?
  [ "$rc" -eq 0 ] || fail "legacy stdin transport denied native delegation"
  [ -z "$output" ] || fail "delegation stdin transport wrote a decision"
  rc=0
  output=$(printf '%s' 'not-json' | "$CHECK") || rc=$?
  [ "$rc" -eq 0 ] && [ -z "$output" ] || fail "compatibility entry depends on JSON tooling"
  rc=0
  printf '%s' '{"tool_name":"Bash","tool_input":{"command":"gh pr merge 17 --auto"}}' \
    | "$CHECK" --claude >"$TMP_ROOT/merge.out" 2>"$TMP_ROOT/merge.err" || rc=$?
  [ "$rc" -eq 2 ] || fail "Claude stdin transport allowed remote PR auto-merge"
  assert_grep 'remote-pr-merge' "$TMP_ROOT/merge.err" "merge refusal omitted its stable reason"
  [ ! -s "$TMP_ROOT/merge.out" ] || fail "Claude denial protocol wrote to stdout"
  pass "hook stdin transports allow delegation and refuse remote PR merge actions"
}

test_compatibility_grammar_stays_bounded() {
  "$CHECK" --help | grep -F 'Native delegation is allowed' >/dev/null || fail "help does not explain delegation freedom"
  "$CHECK" --help | grep -F 'remote PR merge' >/dev/null || fail "help omits the retained merge boundary"
  if "$CHECK" --tool >/dev/null 2>&1; then fail "missing --tool value was accepted"; fi
  if "$CHECK" --command >/dev/null 2>&1; then fail "missing --command value was accepted"; fi
  if "$CHECK" --unknown >/dev/null 2>&1; then fail "unknown option was accepted"; fi
  pass "merge guard keeps a bounded compatibility grammar"
}

test_claude_settings_keep_supervision_and_merge_guard_without_delegation_fence() {
  jq -e '
    [.hooks.PreToolUse[] | select(.matcher == "Bash") | .hooks[].command]
      | map(select(contains("mx-subagent-pretool-check.sh"))) | length == 1
  ' "$SETTINGS" >/dev/null || fail "Claude settings do not register exactly one remote merge guard"
  jq -e '
    [.hooks.PreToolUse[] | select(.matcher == "Bash") | .hooks[].command]
      == [
        "\"$CLAUDE_PROJECT_DIR\"/bin/mx-arm-pretool-check.sh --claude",
        "\"$CLAUDE_PROJECT_DIR\"/bin/mx-cd-pretool-check.sh --claude",
        "\"$CLAUDE_PROJECT_DIR\"/bin/mx-subagent-pretool-check.sh --claude"
      ]
  ' "$SETTINGS" >/dev/null || fail "Claude Bash supervision hooks changed"
  jq -e '.hooks.Stop[0].hooks[0].command | contains("mx-turnend-guard.sh")' "$SETTINGS" >/dev/null \
    || fail "Claude turn-end ownership changed"
  pass "Claude keeps supervision hooks and applies the merge guard only to Bash"
}

test_remote_merge_matrix_and_local_git_freedom() {
  local command rc
  for command in \
    'bin/mx-pr-merge.sh task https://github.com/acme/app/pull/17' \
    'gh pr merge 17 --squash' \
    "gh api graphql -f 'query=mutation { enablePullRequestAutoMerge(input: {}) }'" \
    "gh api graphql -f 'query=mutation { enqueuePullRequest(input: {}) }'" \
    'gh api repos/acme/app/pulls/17/merge -X PUT' \
    'git push origin HEAD:main'; do
    rc=0
    "$CHECK" --command "$command" >"$TMP_ROOT/deny.out" 2>"$TMP_ROOT/deny.err" || rc=$?
    [ "$rc" -eq 2 ] || fail "merge guard allowed: $command"
  done
  for command in \
    'git merge feature/one' \
    'git rebase main' \
    'git push origin HEAD:refs/heads/mx/task' \
    'gh pr create --fill' \
    'gh pr view 17 --json mergeStateStatus'; do
    "$CHECK" --command "$command" >/dev/null 2>&1 || fail "merge guard denied ordinary work: $command"
  done
  "$CHECK" --command 'bin/mx-pr-merge.sh --help' >/dev/null 2>&1 \
    || fail "merge guard denied read-only helper help"
  pass "supported remote merge forms are refused while local integration and branch publication remain available"
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
test_stdin_transport_guards_only_remote_merge_actions
test_compatibility_grammar_stays_bounded
test_claude_settings_keep_supervision_and_merge_guard_without_delegation_fence
test_remote_merge_matrix_and_local_git_freedom
test_native_observers_are_non_authoritative_and_nonblocking
