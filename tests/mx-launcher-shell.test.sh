#!/usr/bin/env bash
# Bash/Zsh activation, prompt, startup-file, shim, and stream tests.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mx_test_tmproot_into TMP_ROOT mx-launcher-shell
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
RUNTIME="$TMP_ROOT/runtime"
mkdir -p "$RUNTIME/bin" "$RUNTIME/.agents/skills" "$RUNTIME/share/shell/shims" \
  "$RUNTIME/config" "$RUNTIME/data" "$RUNTIME/projects" "$RUNTIME/state" "$RUNTIME/target/release"
for file in mx-launcher.sh mx-launch-harness.sh mx-rust-runtime.sh mx-lock.sh mx-session-lock-lib.sh \
  mx-maintainer-override-lib.sh mx-override-bindings.sh mx-wake-lib.sh; do
  cp "$ROOT/bin/$file" "$RUNTIME/bin/$file"
done
cp "$ROOT/target/release/mx" "$RUNTIME/target/release/mx"
cp "$ROOT/share/shell/multplx.bash" "$RUNTIME/share/shell/multplx.bash"
cp "$ROOT/share/shell/multplx.zsh" "$RUNTIME/share/shell/multplx.zsh"
cp "$ROOT/share/shell/shims/"* "$RUNTIME/share/shell/shims/"
chmod +x "$RUNTIME/bin/"*.sh "$RUNTIME/share/shell/shims/"* "$RUNTIME/target/release/mx"
printf '# fixture\n' >"$RUNTIME/AGENTS.md"
printf '# fixture\n' >"$RUNTIME/.agents/skills/fixture.md"
git -C "$RUNTIME" init -q
git -C "$RUNTIME" add -A
git -C "$RUNTIME" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm initial

make_fake_codex() {
  local path=$1
  mkdir -p "${path%/*}"
  cat >"$path" <<'SH'
#!/usr/bin/env bash
printf 'HARNESS_CWD=%s\n' "$(pwd -P)"
printf 'HARNESS_ARG=%s\n' "${1:-}"
printf '\033[?1049h\033[31mraw-unicode-λ\033[0m\r\n\033[?1049l'
SH
  chmod +x "$path"
}

test_bash_adapter_preserves_user_configuration() {
  local home="$TMP_ROOT/bash-home" fakebin="$TMP_ROOT/bash-fakebin" output marker_count shim_count
  mkdir -p "$home" "$fakebin"
  make_fake_codex "$fakebin/codex"
  cat >"$home/.bashrc" <<'SH'
PS1='user-prompt$ '
alias user_alias='printf alias-ok'
shopt -s histappend
PATH="/user/custom::$PATH:"
export USER_RC_COUNT=$(( ${USER_RC_COUNT:-0} + 1 ))
SH
  output=$(HOME="$home" TERM=dumb PATH="$fakebin:/usr/bin:/bin" \
    MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$RUNTIME" MX_SHIM_DIR="$RUNTIME/share/shell/shims" \
    MX_REAL_CLAUDE= MX_REAL_CODEX="$fakebin/codex" MX_REAL_PI= MULTPLX_ACTIVE=1 \
    /bin/bash --noprofile --rcfile "$RUNTIME/share/shell/multplx.bash" -i 2>/dev/null <<'SH'
printf 'COUNT=%s\n' "$USER_RC_COUNT"
printf 'PROMPT=%s\n' "$PS1"
alias user_alias
shopt -q histappend && printf 'HISTAPPEND=yes\n'
printf 'PATH=%s\n' "$PATH"
exit
SH
  )
  assert_contains "$output" 'COUNT=1' "Bash user rc sourced more than once"
  assert_contains "$output" 'PROMPT=user-prompt$  multplx ' "Bash prompt was replaced instead of appended"
  assert_contains "$output" "alias user_alias='printf alias-ok'" "Bash alias was not preserved"
  assert_contains "$output" 'HISTAPPEND=yes' "Bash option was not preserved"
  assert_contains "$output" "PATH=$RUNTIME/share/shell/shims:/user/custom::$fakebin:/usr/bin:/bin:" "Bash PATH bytes were not preserved after shim prepend"
  marker_count=$(printf '%s\n' "$output" | grep -o 'multplx' | wc -l | tr -d ' ')
  [ "$marker_count" -ge 1 ] || fail "Bash marker missing"
  shim_count=$(printf '%s\n' "$output" | sed -n 's/^PATH=//p' | tr ':' '\n' | grep -Fx "$RUNTIME/share/shell/shims" | wc -l | tr -d ' ')
  [ "$shim_count" -eq 1 ] || fail "Bash shim PATH appeared $shim_count times"
  pass "Bash sources user rc once and preserves prompt, alias, option, and PATH"
}

test_zsh_adapter_preserves_user_configuration_and_cleans_temp() {
  command -v zsh >/dev/null 2>&1 || { printf 'skip: zsh not found\n'; return; }
  local home="$TMP_ROOT/zsh-home" fakebin="$TMP_ROOT/zsh-fakebin" adapter output shim_count
  mkdir -p "$home" "$fakebin"
  make_fake_codex "$fakebin/codex"
  cat >"$home/.zshrc" <<'SH'
PROMPT='user-zsh%# '
RPROMPT='right-user'
alias user_alias='print alias-ok'
setopt appendhistory
PATH="/user/zcustom::$PATH:"
export USER_RC_COUNT=$(( ${USER_RC_COUNT:-0} + 1 ))
SH
  adapter=$(mktemp -d "$TMP_ROOT/zsh-adapter.XXXXXX")
  cp "$RUNTIME/share/shell/multplx.zsh" "$adapter/.zshrc"
  output=$(HOME="$home" TERM=dumb PATH="$fakebin:/usr/bin:/bin" ZDOTDIR="$adapter" \
    MX_ZSH_ADAPTER_DIR="$adapter" MX_ORIGINAL_ZDOTDIR_SET=0 MX_ORIGINAL_ZDOTDIR= \
    MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$RUNTIME" MX_SHIM_DIR="$RUNTIME/share/shell/shims" \
    MX_REAL_CLAUDE= MX_REAL_CODEX="$fakebin/codex" MX_REAL_PI= MULTPLX_ACTIVE=1 \
    /bin/zsh -d -i 2>/dev/null <<'SH'
print -r -- "COUNT=$USER_RC_COUNT"
print -r -- "PROMPT=$PROMPT"
print -r -- "RPROMPT=$RPROMPT"
alias user_alias
[[ -o appendhistory ]] && print 'APPENDHISTORY=yes'
print -r -- "PATH=$PATH"
exit
SH
  )
  assert_contains "$output" 'COUNT=1' "Zsh user rc sourced more than once"
  assert_contains "$output" 'PROMPT=user-zsh%# ' "Zsh left prompt changed"
  assert_contains "$output" 'RPROMPT=right-user multplx' "Zsh right marker was not appended"
  assert_contains "$output" "user_alias='print alias-ok'" "Zsh alias was not preserved"
  assert_contains "$output" 'APPENDHISTORY=yes' "Zsh option was not preserved"
  assert_contains "$output" "PATH=$RUNTIME/share/shell/shims:/user/zcustom::$fakebin:/usr/bin:/bin:" "Zsh PATH bytes were not preserved after shim prepend"
  [ ! -e "$adapter" ] || fail "one-shot Zsh adapter directory was not cleaned"
  shim_count=$(printf '%s\n' "$output" | sed -n 's/^PATH=//p' | tr ':' '\n' | grep -Fx "$RUNTIME/share/shell/shims" | wc -l | tr -d ' ')
  [ "$shim_count" -eq 1 ] || fail "Zsh shim PATH appeared $shim_count times"
  pass "Zsh sources user rc once, appends RPROMPT, and cleans its adapter"
}

test_harness_stream_and_child_cwd_are_transparent() {
  local fakebin="$TMP_ROOT/stream-fakebin" output caller="$TMP_ROOT/caller cwd"
  mkdir -p "$fakebin" "$caller"
  make_fake_codex "$fakebin/codex"
  output=$(cd "$caller" && \
    PATH="$fakebin:/usr/bin:/bin" MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$RUNTIME" \
    MX_REAL_CODEX="$fakebin/codex" "$RUNTIME/share/shell/shims/codex" 'arg with space')
  assert_contains "$output" "HARNESS_CWD=$RUNTIME" "shim did not change child cwd"
  assert_contains "$output" 'HARNESS_ARG=arg with space' "shim changed harness argument"
  assert_contains "$output" $'\033[?1049h\033[31mraw-unicode-λ\033[0m\r' "alternate-screen/color bytes were proxied or changed"
  assert_contains "$output" $'\033[?1049l' "alternate-screen exit bytes were changed"

  make_fake_codex "$fakebin/agent"
  output=$(cd "$caller" && \
    PATH="$fakebin:/usr/bin:/bin" MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$RUNTIME" \
    MX_REAL_CURSOR_AGENT="$fakebin/agent" "$RUNTIME/share/shell/shims/agent" 'cursor arg')
  assert_contains "$output" "HARNESS_CWD=$RUNTIME" "agent shim did not change child cwd"
  assert_contains "$output" 'HARNESS_ARG=--sandbox' "agent shim did not force sandbox-enabled launch"
  pass "harness shim passes argv and terminal control bytes without a proxy"
}

test_launcher_activation_round_trip() {
  local home="$TMP_ROOT/roundtrip-home" goodbin="$TMP_ROOT/roundtrip-good" badbin="$TMP_ROOT/roundtrip-bad"
  local caller="$TMP_ROOT/roundtrip caller" output before_path
  mkdir -p "$home" "$goodbin" "$badbin" "$caller"
  make_fake_codex "$goodbin/codex"
  cat >"$badbin/codex" <<'SH'
#!/usr/bin/env bash
printf 'BAD_RECAPTURE\n'
SH
  chmod +x "$badbin/codex"
  cat >"$home/.bashrc" <<SH
PS1='roundtrip$ '
PATH="$badbin:\$PATH"
SH
  before_path=$PATH
  output=$(cd "$caller" && printf '%s\n' \
      'printf "SHELL_CWD=%s\\n" "$(pwd -P)"' \
      'printf "ACTIVE=%s\\n" "$MULTPLX_ACTIVE"' \
      'codex roundtrip-arg' \
      'printf "AFTER_CWD=%s\\n" "$(pwd -P)"' \
      'exit' \
    | HOME="$home" TERM=dumb SHELL=/bin/bash MX_LAUNCH_SHELL=/bin/bash \
      PATH="$goodbin:/usr/bin:/bin" MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$RUNTIME" \
      "$RUNTIME/bin/mx-launcher.sh" 2>/dev/null)
  assert_contains "$output" "SHELL_CWD=$caller" "activated shell did not retain caller cwd"
  assert_contains "$output" 'ACTIVE=1' "activated shell marker environment missing"
  assert_contains "$output" "HARNESS_CWD=$RUNTIME" "activated harness did not use runtime root"
  assert_contains "$output" 'HARNESS_ARG=roundtrip-arg' "activated harness argument changed"
  assert_not_contains "$output" 'BAD_RECAPTURE' "user rc replaced the pre-captured real harness"
  assert_contains "$output" "AFTER_CWD=$caller" "harness child cwd leaked into activated shell"
  [ "$PATH" = "$before_path" ] || fail "activated child changed parent PATH"
  [ ! -e "$RUNTIME/state/.lock" ] || fail "activation without a real broker acquired the session lock"
  pass "launcher activation preserves parent and shell cwd and resists rc-time harness replacement"
}

test_static_prompt_has_no_dynamic_hook() {
  ! grep -Eq 'PROMPT_COMMAND=|precmd|git |mx-(doctor|viz|snapshot)|state/' \
    "$RUNTIME/share/shell/multplx.bash" \
    || fail "Bash prompt adapter contains dynamic prompt work"
  ! grep -Eq 'add-zsh-hook|precmd|git |mx-(doctor|viz|snapshot)|state/' \
    "$RUNTIME/share/shell/multplx.zsh" \
    || fail "Zsh prompt adapter contains dynamic prompt work"
  pass "prompt presentation is static with no poller, state read, or subprocess hook"
}

test_bash_adapter_preserves_user_configuration
test_zsh_adapter_preserves_user_configuration_and_cleans_temp
test_harness_stream_and_child_cwd_are_transparent
test_launcher_activation_round_trip
test_static_prompt_has_no_dynamic_hook
