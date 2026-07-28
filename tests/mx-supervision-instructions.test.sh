#!/usr/bin/env bash
# Tests for harness-aware supervision instruction rendering.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mx_test_tmproot mx-supervision-instructions)
RENDER="$ROOT/bin/mx-supervision-instructions.sh"

test_selected_harness_block_only() {
  local out
  out=$("$RENDER" --harness codex)
  assert_contains "$out" "SUPERVISION OPERATING INSTRUCTIONS - primary harness: codex" "codex heading missing"
  assert_contains "$out" "Mode: Codex foreground checkpoint." "codex snippet missing"
  assert_contains "$out" "bin/mx-watch-checkpoint.sh" "codex checkpoint helper missing"
  assert_not_contains "$out" "Mode: Claude Stop-hook-owned supervision." "renderer printed the claude snippet too"
  assert_not_contains "$out" "Mode: Pi extension background wake." "renderer printed the pi snippet too"
  pass "renderer prints exactly the selected harness block"
}

test_unknown_fallback() {
  local out
  out=$("$RENDER" --harness not-real)
  assert_contains "$out" "primary harness: unknown" "unknown heading missing"
  assert_contains "$out" "Mode: Unknown harness fallback." "unknown fallback snippet missing"
  pass "renderer falls back to unknown.md for unverified harness names"
}

test_conditional_stanzas() {
  local home config out
  home="$TMP_ROOT/conditional-home"
  config="$TMP_ROOT/conditional-config"
  mkdir -p "$home/state" "$home/config" "$config"
  out=$(MX_HOME="$home" MX_CONFIG_OVERRIDE="$config" "$RENDER" --harness codex --read-only 1 --afk 1)
  assert_contains "$out" "- Lock: read-only" "read-only stanza missing"
  assert_contains "$out" "- Away mode: active" "afk stanza missing"
  assert_contains "$out" 'Mode: Codex foreground checkpoint.' "codex snippet missing"
  pass "renderer includes read-only and afk current-state stanzas"
}

test_repair_lines() {
  local home out
  home="$TMP_ROOT/repair-home"
  mkdir -p "$home/state" "$home/config"
  out=$(MX_HOME="$home" MX_CODEX_WATCH_CHECKPOINT=7 "$RENDER" --harness codex --repair-line)
  assert_contains "$out" "bin/mx-watch-checkpoint.sh --seconds 7" "codex repair line did not use checkpoint helper and env override"

  out=$(MX_HOME="$home" "$RENDER" --harness claude --queue-pending 1 --repair-line)
  assert_contains "$out" "After draining queued wakes" "queue-pending prefix missing"
  assert_contains "$out" "Claude Code background task" "claude repair line missing background-task mechanism"

  out=$(MX_HOME="$home" "$RENDER" --harness claude --read-only 1 --repair-line)
  assert_contains "$out" "session holding the system lock" "read-only repair line missing"

  out=$(MX_HOME="$home" "$RENDER" --harness pi --repair-line)
  assert_contains "$out" "Pi tool mx_watch_arm_pi" "pi repair line does not direct the model to the extension-owned tool"
  assert_not_contains "$out" "extension command /mx-watch-arm-pi" "pi repair line still directs the model to the human slash command"
  pass "renderer repair-line mode is harness-aware and honors conditional state"
}

test_cross_harness_ordinary_continuation_and_repair_matrix() {
  local ordinary out

  out=$("$RENDER" --harness pi)
  ordinary=$(printf '%s\n' "$out" | grep -F -- '- Ordinary wake:')
  assert_contains "$ordinary" "Pi extension already owns watcher continuity" "pi ordinary-wake line does not leave continuity to the extension"
  assert_not_contains "$ordinary" "mx_watch_arm_pi" "pi ordinary-wake line incorrectly calls the recovery tool"
  out=$("$RENDER" --harness pi --repair-line)
  assert_contains "$out" "mx_watch_arm_pi" "pi recovery line lost the extension-owned repair tool"

  out=$("$RENDER" --harness claude)
  ordinary=$(printf '%s\n' "$out" | grep -F -- '- Ordinary wake:')
  assert_contains "$ordinary" "Stop-owned auto-arm" "claude ordinary-wake line does not leave continuity to the Stop hook"
  assert_contains "$ordinary" "bin/mx-claude-stop-autoarm.sh" "claude ordinary-wake line lost the auto-arm script name"
  assert_contains "$ordinary" "do not arm another cycle" "claude ordinary-wake line does not forbid a model re-arm"
  assert_not_contains "$ordinary" "bin/mx-watch-arm.sh" "claude ordinary-wake line incorrectly calls the manual arm"
  out=$("$RENDER" --harness claude --repair-line)
  assert_contains "$out" "Claude Code background task" "claude recovery line lost its tracked background repair"
  assert_contains "$out" "bin/mx-watch-arm.sh" "claude recovery line lost the arm command"

  out=$("$RENDER" --harness codex)
  ordinary=$(printf '%s\n' "$out" | grep -F -- '- Ordinary wake:')
  assert_contains "$ordinary" "next foreground" "codex ordinary-wake line lost its foreground checkpoint"
  assert_contains "$ordinary" "bin/mx-watch-checkpoint.sh" "codex ordinary-wake line lost the checkpoint command"
  assert_not_contains "$ordinary" "bin/mx-watch-arm.sh" "codex ordinary-wake line incorrectly uses a background arm"
  out=$("$RENDER" --harness codex --repair-line)
  assert_contains "$out" "foreground checkpoint" "codex recovery line lost its checkpoint repair"
  assert_contains "$out" "bin/mx-watch-checkpoint.sh" "codex recovery line lost the checkpoint command"

  pass "renderer preserves every harness ordinary-continuation and missing-cycle repair path"
}

test_pi_snippet_uses_effective_extension_path() {
  local home out turnend watch
  home="$TMP_ROOT/pi-home"
  turnend="$ROOT/.pi/extensions/mx-primary-turnend-guard.ts"
  watch="$ROOT/.pi/extensions/mx-primary-pi-watch.ts"
  mkdir -p "$home/state" "$home/config"
  out=$(MX_HOME="$home" "$RENDER" --harness pi)
  assert_contains "$out" "-e $turnend -e $watch" "pi snippet did not render both effective extension launch paths"
  assert_contains "$out" "The turn-end guard extension lives at \`$turnend\`" "pi snippet did not render the turn-end guard extension path"
  assert_contains "$out" "The watcher extension lives at \`$watch\`" "pi snippet did not render the watcher extension path"
  assert_not_contains "$out" "__MX_PI_EXT__" "renderer leaked the Pi extension path placeholder"
  assert_not_contains "$out" "__MX_PI_TURNEND_EXT__" "renderer leaked the Pi turn-end extension path placeholder"
  assert_not_contains "$out" "state/mx-primary-pi-watch.ts" "pi snippet kept the old generated state-relative extension path"
  pass "pi supervision snippet renders the effective extension path"
}

test_selected_harness_block_only
test_unknown_fallback
test_conditional_stanzas
test_repair_lines
test_cross_harness_ordinary_continuation_and_repair_matrix
test_pi_snippet_uses_effective_extension_path
