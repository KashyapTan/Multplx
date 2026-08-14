#!/usr/bin/env bash
# Opt-in real-binary smoke for the global harness path.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

if [ "${MX_LAUNCHER_LIVE_E2E:-0}" != 1 ]; then
  printf 'skip: set MX_LAUNCHER_LIVE_E2E=1 to run real launcher harness smokes\n'
  exit 0
fi

mx_test_tmproot_into TMP_ROOT mx-launcher-live
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
RUNTIME="$TMP_ROOT/runtime"
mkdir -p "$RUNTIME/bin" "$RUNTIME/.agents/skills" "$RUNTIME/share/shell/shims" \
  "$RUNTIME/config" "$RUNTIME/data" "$RUNTIME/projects" "$RUNTIME/state" "$RUNTIME/target/release"
for file in mx-launcher.sh mx-launch-harness.sh mx-rust-runtime.sh mx-lock.sh mx-session-lock-lib.sh; do
  cp "$ROOT/bin/$file" "$RUNTIME/bin/$file"
done
cp "$ROOT/target/release/mx" "$RUNTIME/target/release/mx"
cp "$ROOT/share/shell/multplx.bash" "$RUNTIME/share/shell/multplx.bash"
cp "$ROOT/share/shell/multplx.zsh" "$RUNTIME/share/shell/multplx.zsh"
cp "$ROOT/share/shell/shims/"* "$RUNTIME/share/shell/shims/"
chmod +x "$RUNTIME/bin/"*.sh "$RUNTIME/share/shell/shims/"* "$RUNTIME/target/release/mx"
printf '# live launcher fixture\n' >"$RUNTIME/AGENTS.md"
printf '# fixture\n' >"$RUNTIME/.agents/skills/fixture.md"
git -C "$RUNTIME" init -q
git -C "$RUNTIME" add -A
git -C "$RUNTIME" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm initial

ran=0
for harness in claude codex cursor pi; do
  executable=$harness
  [ "$harness" = cursor ] && executable=cursor-agent
  real=$(command -v "$executable" 2>/dev/null || true)
  [ -n "$real" ] || continue
  case "$harness" in
    claude) variable=MX_REAL_CLAUDE ;;
    codex) variable=MX_REAL_CODEX ;;
    cursor) variable=MX_REAL_CURSOR_AGENT ;;
    pi) variable=MX_REAL_PI ;;
  esac
  output=$(env MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$RUNTIME" "$variable=$real" \
    "$RUNTIME/bin/mx-launch-harness.sh" "$harness" --version 2>&1) \
    || fail "$harness --version failed through the real launcher: $output"
  [ -n "$output" ] || fail "$harness --version returned no version text"
  printf 'ok - live %s launcher smoke: %s\n' "$harness" "$(printf '%s' "$output" | head -1)"
  ran=$((ran + 1))
done
[ "$ran" -gt 0 ] || fail "no verified harness binary is installed"
pass "real harness binaries execute through the child-root launcher"
