#!/usr/bin/env bash
# Translate Cursor's lower-camel hook transport to the shared Multplx guards.
set -u

# Portion 08 Rust-default adapter. Keep the body below as the explicit bounded
# rollback path and as the sourced-function ABI where this file is sourceable.
MX_SUPERVISION_ADAPTER_DIR=${BASH_SOURCE[0]%/*}
[ "$MX_SUPERVISION_ADAPTER_DIR" != "${BASH_SOURCE[0]}" ] || MX_SUPERVISION_ADAPTER_DIR=.
MX_SUPERVISION_ADAPTER_DIR="$(CDPATH='' cd -- "$MX_SUPERVISION_ADAPTER_DIR" 2>/dev/null && pwd)" || exit 1
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
  # shellcheck source=bin/mx-rust-runtime.sh
  . "$MX_SUPERVISION_ADAPTER_DIR/mx-rust-runtime.sh"
  mx_supervision_adapter_implementation=$(mx_supervision_implementation) || exit $?
  if [ "$mx_supervision_adapter_implementation" = rust ]; then
    MX_RUST_SOURCE_ROOT="$(cd "$MX_SUPERVISION_ADAPTER_DIR/.." && pwd)"; export MX_RUST_SOURCE_ROOT
    mx_supervision_adapter_bin=$(mx_rust_runtime_bin) || exit $?
    exec "$mx_supervision_adapter_bin" supervision mx-cursor-hook.sh "$@"
  fi
fi

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P) || exit 1
MODE=${1:-}
PAYLOAD=$(cat 2>/dev/null || true)

deny() {
  local message=${1:-"Multplx guard refused this operation."}
  command -v jq >/dev/null 2>&1 || exit 1
  jq -cn --arg message "$message" '{permission:"deny",user_message:$message,agent_message:$message}'
}

guard_pretool() {
  local guard=$1 output status
  output=$(printf '%s' "$PAYLOAD" | "$SCRIPT_DIR/$guard" 2>&1)
  status=$?
  if [ "$status" -eq 2 ]; then
    deny "$output"
    return 2
  fi
  [ "$status" -eq 0 ] || return 1
  return 0
}

case "$MODE" in
  session-start)
    [ -n "$PAYLOAD" ] || exit 1
    nudge=$(printf '%s' "$PAYLOAD" | "$SCRIPT_DIR/mx-sessionstart-nudge.sh" 2>/dev/null || true)
    [ -n "$nudge" ] || { printf '%s\n' '{}'; exit 0; }
    command -v jq >/dev/null 2>&1 || exit 1
    jq -cn --arg context "$nudge" '{additional_context:$context}'
    ;;
  pre-tool)
    [ -n "$PAYLOAD" ] || exit 1
    command -v jq >/dev/null 2>&1 || exit 1
    printf '%s' "$PAYLOAD" | jq -e 'type == "object" and (.tool_name | type == "string") and (.tool_input | type == "object")' >/dev/null 2>&1 || exit 1
    guard_pretool mx-arm-pretool-check.sh || { [ "$?" -eq 2 ] && exit 0; exit 1; }
    guard_pretool mx-cd-pretool-check.sh || { [ "$?" -eq 2 ] && exit 0; exit 1; }
    guard_pretool mx-subagent-pretool-check.sh || { [ "$?" -eq 2 ] && exit 0; exit 1; }
    printf '%s\n' '{"permission":"allow"}'
    ;;
  subagent-start)
    [ -n "$PAYLOAD" ] || exit 1
    output=$("$SCRIPT_DIR/mx-subagent-pretool-check.sh" --tool subagentStart 2>&1)
    status=$?
    if [ "$status" -eq 2 ]; then deny "$output"; else [ "$status" -eq 0 ] || exit 1; printf '%s\n' '{"permission":"allow"}'; fi
    ;;
  stop)
    [ -n "$PAYLOAD" ] || { printf '%s\n' '{}'; exit 0; }
    command -v jq >/dev/null 2>&1 || { printf '%s\n' '{}'; exit 0; }
    loop_count=$(printf '%s' "$PAYLOAD" | jq -r '.loop_count // 0' 2>/dev/null) || { printf '%s\n' '{}'; exit 0; }
    [ "$loop_count" -eq 0 ] || { printf '%s\n' '{}'; exit 0; }
    guard_payload=$(printf '%s' "$PAYLOAD" | jq -c '. + {stop_hook_active:false}') || { printf '%s\n' '{}'; exit 0; }
    output=$(printf '%s' "$guard_payload" | "$SCRIPT_DIR/mx-turnend-guard.sh" 2>&1)
    status=$?
    if [ "$status" -eq 2 ]; then
      jq -cn --arg followup "$output" '{followup_message:$followup}'
    else
      printf '%s\n' '{}'
    fi
    ;;
  *)
    printf 'usage: %s session-start|pre-tool|subagent-start|stop\n' "${0##*/}" >&2
    exit 2
    ;;
esac
