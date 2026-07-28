#!/usr/bin/env bash
# Raw JSON-RPC contract tests for the stdio report_status MCP adapter.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SERVER="$ROOT/bin/mx-report-mcp.mjs"
REPORT="$ROOT/bin/mx-report"
TMP_ROOT=$(mx_test_tmproot mx-report-mcp)

run_rpc() {
  local home=$1 task_id=$2 input=$3
  MX_HOME="$home" MX_REPORT_STATE_OVERRIDE="$home/state" MX_TASK_ID="$task_id" \
    node "$SERVER" <<EOF
$input
EOF
}

test_schema_matches_wrapper() {
  local home id output schema_states wrapper_states
  home="$TMP_ROOT/schema-home"
  id=mcp-schema-a1
  mkdir -p "$home/state"
  output=$(run_rpc "$home" "$id" \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}') \
    || fail "MCP initialize/tools-list exchange failed"
  [ "$(printf '%s\n' "$output" | wc -l | tr -d ' ')" = 2 ] \
    || fail "MCP server responded to a notification or omitted a request response"
  printf '%s\n' "$output" | jq -e \
    'select(.id == 2) | .result.tools == [{
      name:"report_status",
      description:(.result.tools[0].description),
      inputSchema:{
        type:"object",
        properties:{
          state:{type:"string",enum:["working","paused","blocked","needs-decision","done","failed","resolved"]},
          message:{type:"string",maxLength:300},
          key:{type:"string",pattern:"^[A-Za-z0-9._-]+$"}
        },
        required:["state","message"],
        additionalProperties:false
      }
    }]' >/dev/null || fail "tools/list exposed the wrong report_status schema"
  schema_states=$(printf '%s\n' "$output" | jq -r \
    'select(.id == 2) | .result.tools[0].inputSchema.properties.state.enum[]')
  wrapper_states=$("$REPORT" --list-states)
  [ "$schema_states" = "$wrapper_states" ] \
    || fail "MCP state enum diverges from mx-report --list-states"
  pass "mx-report MCP: tools/list schema reuses the wrapper's exact closed enum"
}

test_valid_call_appends_and_stays_bound() {
  local home id output
  home="$TMP_ROOT/call-home"
  id=mcp-call-b2
  mkdir -p "$home/state"
  output=$(run_rpc "$home" "$id" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"report_status","arguments":{"state":"needs-decision","message":"choose API","key":"api-shape"}}}') \
    || fail "valid tools/call exchange failed"
  printf '%s\n' "$output" | jq -e \
    'select(.id == 1) | (.result.isError // false) == false' >/dev/null \
    || fail "valid tools/call returned an error result"
  [ "$(cat "$home/state/$id.status")" = "needs-decision [key=api-shape]: choose API" ] \
    || fail "valid tools/call did not append through mx-report"
  assert_absent "$home/state/another-task.status" \
    "bound MCP call wrote another task's status file"
  pass "mx-report MCP: valid calls append only to the launch-bound task"
}

test_schema_rejections_write_nothing() {
  local home id output
  home="$TMP_ROOT/reject-home"
  id=mcp-reject-c3
  mkdir -p "$home/state"
  output=$(run_rpc "$home" "$id" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"report_status","arguments":{"state":"blocekd","message":"typo"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"report_status","arguments":{"state":"done","message":"extra","task_id":"another"}}}') \
    || fail "invalid tools/call exchange crashed the MCP server"
  [ "$(printf '%s\n' "$output" | jq -s '[.[] | select(.error.code == -32602)] | length')" = 2 ] \
    || fail "invalid state and extra property were not schema errors"
  assert_absent "$home/state/$id.status" \
    "schema-invalid calls created the bound status file"
  assert_absent "$home/state/another.status" \
    "schema-invalid calls created another status file"
  pass "mx-report MCP: invalid enum values and extra properties are rejected before writes"
}

test_missing_binding_fails_closed() {
  local home output
  home="$TMP_ROOT/unbound-home"
  mkdir -p "$home/state"
  output=$(run_rpc "$home" "" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"report_status","arguments":{"state":"done","message":"no binding"}}}') \
    || fail "unbound tools/call crashed the MCP server"
  printf '%s\n' "$output" | jq -e \
    'select(.id == 1) | .result.isError == true and
     (.result.content[0].text | contains("no task binding found"))' >/dev/null \
    || fail "unbound server did not return a distinct fail-closed tool result"
  if find "$home/state" -name '*.status' -print -quit | grep . >/dev/null; then
    fail "unbound MCP server wrote a status file"
  fi
  pass "mx-report MCP: a server without launch-time MX_TASK_ID refuses writes"
}

node --check "$SERVER" || fail "mx-report-mcp.mjs does not parse"
test_schema_matches_wrapper
test_valid_call_appends_and_stays_bound
test_schema_rejections_write_nothing
test_missing_binding_fails_closed
