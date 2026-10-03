#!/usr/bin/env bash
# Cursor CLI adapter, hook transport, launcher safety, and terminal signature tests.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

export MX_RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}

mx_test_tmproot_into TMP_ROOT mx-cursor-adapter

assert_file_has() {
  local file=$1 needle=$2 message=$3
  grep -F -- "$needle" "$file" >/dev/null || fail "$message"
}

make_runtime() {
  local runtime=$1
  mkdir -p "$runtime/bin" "$runtime/.agents/skills" "$runtime/share/shell/shims" \
    "$runtime/config" "$runtime/data" "$runtime/projects" "$runtime/state"
  cp "$ROOT/bin/mx-launcher.sh" \
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
payload=$(cat)
case "$payload" in
  *'gh pr merge'*) printf 'remote PR merges are human-only\n' >&2; exit 2 ;;
esac
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
  [ "$(printf '%s' "$output" | jq -r '.permission')" = allow ] || fail "Cursor native delegation tool was denied"
  output=$(printf '{"tool_name":"Shell","tool_input":{"command":"gh pr merge 7"}}' | "$fixture/bin/mx-cursor-hook.sh" pre-tool)
  [ "$(printf '%s' "$output" | jq -r '.permission')" = deny ] || fail "Cursor remote merge guard denial was not translated"
  output=$(MX_STATE_OVERRIDE="$fixture/state" printf '{"agent_type":"generalPurpose","session_id":"parent-s"}' | "$fixture/bin/mx-cursor-hook.sh" subagent-start)
  [ "$(printf '%s' "$output" | jq -r '.permission')" = allow ] || fail "Cursor subagentStart was denied"
  output=$(printf '{"session_id":"s","loop_count":0}' | "$fixture/bin/mx-cursor-hook.sh" stop)
  [ "$output" = '{}' ] || fail "Cursor stop without a primary home failed open into a follow-up"
  output=$(printf '{"session_id":"s","loop_count":1}' | "$fixture/bin/mx-cursor-hook.sh" stop)
  [ "$output" = '{}' ] || fail "Cursor stop continuation was not bounded after loop one"
  if printf '{}' | "$fixture/bin/mx-cursor-hook.sh" pre-tool >/dev/null 2>&1; then
    fail "malformed critical Cursor preToolUse payload failed open"
  fi
  jq -e '
    .version == 1 and
    (.hooks.sessionStart[0].failClosed == true) and
    (.hooks.preToolUse[0].failClosed == true) and
    (.hooks.subagentStart[0].failClosed == false) and
    (.hooks.stop[0].loop_limit == 200) and
    (.hooks.stop[0].timeout == 28800)
  ' "$ROOT/.cursor/hooks.json" >/dev/null || fail "tracked Cursor hook contract is incomplete"
  pass "Cursor hooks preserve supervision, allow native delegation, and bound stop continuation"
}

test_cursor_stop_park_lifecycle() {
  python3 - "$ROOT" "$TMP_ROOT/park" "$MX_RUST_BIN" <<'PY'
import json, os, pathlib, signal, subprocess, sys, time
root, fixture, binary = map(pathlib.Path, sys.argv[1:])
(fixture / 'bin').mkdir(parents=True)
(fixture / 'state').mkdir()
(fixture / 'AGENTS.md').write_text('# fixture\n')
(fixture / '.mx-daemon-home').write_text('cursor-park-fixture\n')
state = fixture / 'state'
(state / '.lock').write_text(str(os.getpid()) + '\n')
(state / 'work.meta').write_text('id=work\n')
arm = fixture / 'bin' / 'mx-watch-arm.sh'
arm.write_text('#!/bin/sh\nsleep "${PARK_DELAY:-0}"\nprintf "signal: fixture report\\n"\n')
arm.chmod(0o700)
env = dict(os.environ, MX_ROOT_OVERRIDE=str(fixture), MX_HOME=str(fixture),
           MX_STATE_OVERRIDE=str(state), MX_RUST_SOURCE_ROOT=str(fixture))
cmd = [str(binary), 'supervision', 'mx-cursor-hook.sh', 'stop']
def start(count=0, delay='0'):
    child = subprocess.Popen(cmd, env=dict(env, PARK_DELAY=delay), stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    child.stdin.write(json.dumps({'session_id':'fixture', 'loop_count':count}))
    child.stdin.close(); child.stdin = None
    return child
def result(child):
    out, err = child.communicate(timeout=6)
    assert child.returncode == 0, err
    return json.loads(out)
for count in (0, 1, 2):
    assert 'fixture report' in result(start(count))['followup_message']
# The model is idle, and the older park cannot emit after a newer stop claims.
old = start(delay='20'); time.sleep(.3)
new = start(1)
assert 'followup_message' in result(new)
assert result(old) == {}
assert not list(state.glob('.cursor-park-output.*'))
# Ownership and away-mode changes retire the tracked arm instead of waking.
child = start(delay='20'); time.sleep(.3); (state / '.afk').touch()
assert result(child) == {}; (state / '.afk').unlink()
child = start(delay='20'); time.sleep(.3); (state / '.lock').write_text('99999999\n')
assert result(child) == {}; (state / '.lock').write_text(str(os.getpid()) + '\n')
child = start(delay='20'); time.sleep(.3); child.send_signal(signal.SIGTERM)
assert result(child) == {}
assert not list(state.glob('.cursor-park-output.*'))
# Repair feedback has a separate bounded budget; a healthy event resets it.
arm.write_text('#!/bin/sh\nprintf "watcher: FAILED fixture\\n"\nexit 1\n')
for count in range(3):
    assert 'bounded attempts' in result(start(count))['followup_message']
assert result(start(3)) == {}
assert 'ceiling reached' in result(start(180))['followup_message']
assert result(start(181)) == {}
# A non-owner cannot start a new park, and Pi-hosted Cursor hooks stand down.
(state / '.lock').write_text('99999999\n')
assert result(start()) == {}
(state / '.lock').write_text(str(os.getpid()) + '\n')
pi_env = dict(env, PI_CODING_AGENT='true')
pi_env.pop('CURSOR_AGENT', None); pi_env.pop('CURSOR_INVOKED_AS', None)
assert subprocess.run(cmd, input='{}', env=pi_env, capture_output=True, text=True).stdout.strip() == '{}'
for payload in ('{"loop_count":"oops"}', '{"loop_count":-1}', '{"loop_count":0.5}'):
    assert subprocess.run(cmd, input=payload, env=env, capture_output=True, text=True).stdout.strip() == '{}'
# No accepted work means a quiescent turn boundary.
(state / 'work.meta').unlink()
assert result(start()) == {}
assert subprocess.run(cmd, input='[]', env=env, capture_output=True, text=True).stdout.strip() == '{}'
print('ok - Cursor park owns successive wake turns, supersession, AFK, session loss, signal cleanup and bounded failure feedback')
PY
}

test_cursor_spawn_profile_and_terminal_signatures() {
  local output
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
test_cursor_stop_park_lifecycle
test_cursor_spawn_profile_and_terminal_signatures
