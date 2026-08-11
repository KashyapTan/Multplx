#!/usr/bin/env bash
# Deterministic installer, path, harness, lock, backend, and delegation tests.
set -u

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

INSTALLER=$ROOT/bin/mx-launcher-install.sh
mx_test_tmproot_into TMP_ROOT mx-launcher
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)

make_runtime() {
  local target=$1 source_file
  mkdir -p "$target/bin" "$target/.agents/skills" "$target/share/shell/shims"
  for source_file in \
    mx-launcher-lib.sh mx-launcher.sh mx-launcher-install.sh \
    mx-launch-harness.sh mx-rust-runtime.sh mx-lock.sh mx-session-lock-lib.sh \
    mx-maintainer-override-lib.sh mx-override-bindings.sh mx-wake-lib.sh; do
    cp "$ROOT/bin/$source_file" "$target/bin/$source_file"
  done
  cp "$ROOT/share/shell/multplx.bash" "$target/share/shell/multplx.bash"
  cp "$ROOT/share/shell/multplx.zsh" "$target/share/shell/multplx.zsh"
  cp "$ROOT/share/shell/shims/claude" "$target/share/shell/shims/claude"
  cp "$ROOT/share/shell/shims/codex" "$target/share/shell/shims/codex"
  cp "$ROOT/share/shell/shims/agent" "$target/share/shell/shims/agent"
  cp "$ROOT/share/shell/shims/cursor-agent" "$target/share/shell/shims/cursor-agent"
  cp "$ROOT/share/shell/shims/pi" "$target/share/shell/shims/pi"
  chmod +x "$target/bin/"*.sh "$target/share/shell/shims/"*
  printf '# launcher fixture\n' >"$target/AGENTS.md"
  printf '# skill fixture\n' >"$target/.agents/skills/fixture.md"
  git -C "$target" init -q
  git -C "$target" add -A
  git -C "$target" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm initial
}

install_fixture() {
  local case_dir=$1 root=$2
  mkdir -p "$case_dir"
  "$INSTALLER" \
    --root "$root" \
    --bin-dir "$case_dir/bin" \
    --config-dir "$case_dir/config" \
    --data-dir "$case_dir/data" >/dev/null
}

make_fake_harnesses() {
  local fakebin=$1 harness
  mkdir -p "$fakebin"
  for harness in claude codex agent pi; do
    cat >"$fakebin/$harness" <<'SH'
#!/usr/bin/env bash
set -u
record=${MX_FAKE_HARNESS_RECORD:?}
mkdir -p "$record"
pwd -P >"$record/cwd"
printf '%s\n' "${MX_ROOT_OVERRIDE:-}" >"$record/root"
printf '%s\n' "${MX_HOME:-}" >"$record/home"
printf '%s\n' "${MX_BACKEND-unset}" >"$record/backend"
printf '%s\n' "${TMUX-unset}" >"$record/tmux"
printf '%s\n' "${HERDR_ENV-unset}" >"$record/herdr"
printf '%s\n' "${CMUX_WORKSPACE_ID-unset}" >"$record/cmux"
printf '%s\n' "$#" >"$record/argc"
i=0
for arg in "$@"; do
  printf '%s' "$arg" >"$record/arg.$i"
  i=$((i + 1))
done
SH
    chmod +x "$fakebin/$harness"
  done
}

test_existing_install_paths_and_literal_safety() {
  local case_dir="$TMP_ROOT/existing space ü;'quote;\$(safe)" root="$TMP_ROOT/runtime space ü;'quote;\$(safe)"
  local before marker output status checksum separate_case separate_home
  make_runtime "$root"
  install_fixture "$case_dir" "$root"

  [ "$(cat "$case_dir/config/root")" = "$root" ] || fail "installer did not record literal root"
  [ "$(cat "$case_dir/config/home")" = "$root" ] || fail "existing mode did not preserve root as home"
  for part in config data projects state; do
    [ -d "$root/$part" ] || fail "existing mode did not create $part"
  done
  output=$("$case_dir/bin/multplx" paths)
  assert_contains "$output" "root=$root" "paths root mismatch"
  assert_contains "$output" "home=$root" "paths home mismatch"
  assert_contains "$output" "bin=$case_dir/bin/multplx" "paths bootstrap mismatch"

  checksum=$(shasum -a 256 "$case_dir/bin/multplx" | awk '{print $1}')
  install_fixture "$case_dir" "$root"
  [ "$(shasum -a 256 "$case_dir/bin/multplx" | awk '{print $1}')" = "$checksum" ] \
    || fail "compatible reinstall changed bootstrap bytes"

  separate_case="$TMP_ROOT/adopted-separate-case"
  separate_home="$TMP_ROOT/adopted-separate-home"
  printf 'adopted checkout may be dirty\n' >"$root/untracked-development-file"
  "$INSTALLER" --root "$root" --home "$separate_home" \
    --bin-dir "$separate_case/bin" --config-dir "$separate_case/config" \
    --data-dir "$separate_case/data" >/dev/null
  [ "$(cat "$separate_case/config/home")" = "$separate_home" ] \
    || fail "adopted install did not preserve the independently selected home"
  "$separate_case/bin/multplx" paths >/dev/null \
    || fail "adopted checkout with a separate home was mistaken for a managed runtime"

  marker="$case_dir/evaluated"
  printf '\$(touch %s)\n' "$marker" >"$case_dir/config/root"
  if output=$("$case_dir/bin/multplx" paths 2>&1); then status=0; else status=$?; fi
  expect_code 2 "$status" "shell syntax in root record"
  [ ! -e "$marker" ] || fail "path file was evaluated as shell code"
  assert_contains "$output" "path is not absolute" "malformed literal path diagnostic missing"

  printf '%s\nextra\n' "$root" >"$case_dir/config/root"
  if "$case_dir/bin/multplx" paths >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "extra path-file line"

  printf '%s\0\n' "$root" >"$case_dir/config/root"
  if "$case_dir/bin/multplx" paths >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "NUL path-file byte"
  pass "existing install is literal, atomic, idempotent, and path-safe"
}

test_collisions_uninstall_and_private_preservation() {
  local root="$TMP_ROOT/collision-root" case_dir="$TMP_ROOT/collision-case" status
  make_runtime "$root"
  mkdir -p "$case_dir/bin" "$case_dir/config"
  printf 'unrelated\n' >"$case_dir/bin/multplx"
  if install_fixture "$case_dir" "$root" >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "unrelated bootstrap collision"
  [ "$(cat "$case_dir/bin/multplx")" = unrelated ] || fail "collision overwrote unrelated bootstrap"

  rm -f "$case_dir/bin/multplx"
  printf 'symlink target\n' >"$case_dir/bin/not-the-launcher"
  ln -s not-the-launcher "$case_dir/bin/multplx"
  if install_fixture "$case_dir" "$root" >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "linked bootstrap collision"
  [ -L "$case_dir/bin/multplx" ] || fail "collision replaced a linked bootstrap"
  rm -f "$case_dir/bin/multplx"
  install_fixture "$case_dir" "$root"
  printf 'private sentinel\n' >"$root/data/private-sentinel"
  "$INSTALLER" --uninstall \
    --bin-dir "$case_dir/bin" --config-dir "$case_dir/config" --data-dir "$case_dir/data" >/dev/null
  [ ! -e "$case_dir/bin/multplx" ] || fail "uninstall left bootstrap"
  [ ! -e "$case_dir/config/root" ] && [ ! -e "$case_dir/config/home" ] \
    || fail "uninstall left path records"
  [ "$(cat "$root/data/private-sentinel")" = 'private sentinel' ] \
    || fail "uninstall changed private operational data"
  pass "collisions refuse and uninstall preserves runtime and private data"
}

test_atomic_interruption_recovery() {
  local root="$TMP_ROOT/atomic-root" source="$TMP_ROOT/atomic-managed-source"
  local target case_dir status
  make_runtime "$root"
  for target in root home multplx; do
    case_dir="$TMP_ROOT/atomic-$target"
    if MX_LAUNCHER_INSTALL_FAIL_BEFORE=$target install_fixture "$case_dir" "$root" \
      >/dev/null 2>&1; then status=0; else status=$?; fi
    expect_code 1 "$status" "interruption before $target publication"
    install_fixture "$case_dir" "$root"
    "$case_dir/bin/multplx" paths >/dev/null \
      || fail "install did not converge after interruption before $target"
  done

  make_runtime "$source"
  case_dir="$TMP_ROOT/atomic-runtime"
  if MX_LAUNCHER_INSTALL_FAIL_BEFORE=runtime "$INSTALLER" --managed --source "$source" \
    --bin-dir "$case_dir/bin" --config-dir "$case_dir/config" \
    --data-dir "$case_dir/data" >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 1 "$status" "interruption before managed runtime publication"
  [ ! -e "$case_dir/data/runtime" ] || fail "interrupted managed clone was published"
  "$INSTALLER" --managed --source "$source" \
    --bin-dir "$case_dir/bin" --config-dir "$case_dir/config" \
    --data-dir "$case_dir/data" >/dev/null
  "$case_dir/bin/multplx" paths >/dev/null \
    || fail "managed install did not converge after runtime interruption"
  pass "atomic publication interruptions leave every install recoverable"
}

test_managed_clone_and_linked_worktree_refusal() {
  local source="$TMP_ROOT/managed-source" case_dir="$TMP_ROOT/managed-case" linked status
  local invalid_source="$TMP_ROOT/managed-invalid-source" invalid_case="$TMP_ROOT/managed-invalid-case"
  mkdir -p "$invalid_source"
  mx_git_init_commit "$invalid_source"
  if "$INSTALLER" --managed --source "$invalid_source" \
    --bin-dir "$invalid_case/bin" --config-dir "$invalid_case/config" \
    --data-dir "$invalid_case/data" >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "invalid managed source"
  [ ! -e "$invalid_case/data/runtime" ] || fail "invalid managed runtime was published before validation"

  make_runtime "$source"
  mkdir -p "$case_dir"
  "$INSTALLER" --managed --source "$source" \
    --bin-dir "$case_dir/bin" --config-dir "$case_dir/config" --data-dir "$case_dir/data" >/dev/null
  [ -d "$case_dir/data/runtime/.git" ] || fail "managed mode did not create a plain clone"
  [ "$(git -C "$case_dir/data/runtime" config --local --get multplx.managed)" = true ] \
    || fail "managed runtime ownership marker is missing"
  [ "$(cat "$case_dir/config/root")" = "$case_dir/data/runtime" ] || fail "managed root record mismatch"
  [ "$(cat "$case_dir/config/home")" = "$case_dir/data/home" ] || fail "managed home record mismatch"
  [ "$(git -C "$case_dir/data/runtime" status --porcelain)" = '' ] || fail "managed runtime is dirty"

  printf 'dirty\n' >>"$case_dir/data/runtime/AGENTS.md"
  if "$case_dir/bin/multplx" paths >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "dirty managed runtime"

  linked="$TMP_ROOT/linked-worktree"
  git -C "$source" worktree add -q --detach "$linked"
  if "$INSTALLER" --root "$linked" \
    --bin-dir "$TMP_ROOT/linked-bin" --config-dir "$TMP_ROOT/linked-config" \
    --data-dir "$TMP_ROOT/linked-data" >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "linked worktree root"
  pass "managed mode separates clean runtime/home and linked task roots refuse"
}

test_harness_cwd_arguments_environment_and_backend() {
  local root="$TMP_ROOT/harness-root" case_dir="$TMP_ROOT/harness-case" fakebin record caller status
  make_runtime "$root"
  install_fixture "$case_dir" "$root"
  fakebin="$case_dir/fakebin"
  make_fake_harnesses "$fakebin"
  caller="$case_dir/unrelated repo"
  mkdir -p "$caller"
  record="$case_dir/record"
  (
    cd "$caller" || exit 1
    PATH="$fakebin:/usr/bin:/bin" MX_FAKE_HARNESS_RECORD="$record" \
      TMUX='tmux bytes ;$' HERDR_ENV=1 CMUX_WORKSPACE_ID='cmux bytes' \
      "$case_dir/bin/multplx" --backend herdr codex \
        'space arg' '*?[glob]' $'line one\nline two'
    [ "$(pwd -P)" = "$caller" ] || exit 9
  ) || fail "direct harness launch changed caller cwd or failed"
  [ "$(cat "$record/cwd")" = "$root" ] || fail "harness did not start at code root"
  [ "$(cat "$record/root")" = "$root" ] || fail "harness root environment mismatch"
  [ "$(cat "$record/home")" = "$root" ] || fail "harness home environment mismatch"
  [ "$(cat "$record/backend")" = herdr ] || fail "explicit backend did not win"
  [ "$(cat "$record/tmux")" = 'tmux bytes ;$' ] || fail "TMUX bytes changed"
  [ "$(cat "$record/herdr")" = 1 ] || fail "HERDR_ENV changed"
  [ "$(cat "$record/cmux")" = 'cmux bytes' ] || fail "cmux identifier changed"
  [ "$(cat "$record/argc")" = 3 ] || fail "argument count changed"
  [ "$(cat "$record/arg.0")" = 'space arg' ] || fail "space argument changed"
  [ "$(cat "$record/arg.1")" = '*?[glob]' ] || fail "glob argument changed"
  [ "$(cat "$record/arg.2")" = $'line one\nline two' ] || fail "newline argument changed"

  rm -rf "$record"
  PATH="$fakebin:/usr/bin:/bin" MX_FAKE_HARNESS_RECORD="$record" \
    "$case_dir/bin/multplx" cursor 'cursor arg' >/dev/null
  [ "$(cat "$record/argc")" = 3 ] || fail "Cursor launch did not prepend exactly --sandbox enabled"
  [ "$(cat "$record/arg.0")" = --sandbox ] && [ "$(cat "$record/arg.1")" = enabled ] \
    || fail "Cursor launch did not keep sandboxing enabled"
  [ "$(cat "$record/arg.2")" = 'cursor arg' ] || fail "Cursor argument changed"
  if PATH="$fakebin:/usr/bin:/bin" MX_FAKE_HARNESS_RECORD="$record" \
      "$case_dir/bin/multplx" cursor --yolo >/dev/null 2>&1; then
    fail "Cursor launcher accepted blanket yolo authority"
  fi

  rm -rf "$record"
  PATH="$fakebin:/usr/bin:/bin" MX_FAKE_HARNESS_RECORD="$record" MX_BACKEND=tmux \
    "$case_dir/bin/multplx" --backend auto pi >/dev/null
  [ "$(cat "$record/backend")" = unset ] || fail "--backend auto did not restore detection"

  rm -rf "$record"
  if PATH=/usr/bin:/bin MX_FAKE_HARNESS_RECORD="$record" \
    "$case_dir/bin/multplx" claude >/dev/null 2>&1; then status=0; else status=$?; fi
  [ "$status" -eq 127 ] || fail "missing harness returned $status, expected 127"
  pass "harness launch preserves cwd, argv, environment, and backend independence"
}

test_live_lock_refusal_and_stale_permission() {
  local root="$TMP_ROOT/lock-root" case_dir="$TMP_ROOT/lock-case" fakebin record holder status
  make_runtime "$root"
  install_fixture "$case_dir" "$root"
  fakebin="$case_dir/fakebin"
  make_fake_harnesses "$fakebin"
  record="$case_dir/record"

  bash -c 'exec -a codex sleep 30' &
  holder=$!
  printf '%s\n' "$holder" >"$root/state/.lock"
  if PATH="$fakebin:/usr/bin:/bin" MX_FAKE_HARNESS_RECORD="$record" \
    "$case_dir/bin/multplx" codex >/dev/null 2>&1; then status=0; else status=$?; fi
  kill "$holder" 2>/dev/null || true
  wait "$holder" 2>/dev/null || true
  expect_code 3 "$status" "known live competing broker"
  [ ! -e "$record/cwd" ] || fail "live-lock refusal executed harness"

  printf '99999999\n' >"$root/state/.lock"
  PATH="$fakebin:/usr/bin:/bin" MX_FAKE_HARNESS_RECORD="$record" \
    "$case_dir/bin/multplx" codex >/dev/null
  [ -f "$record/cwd" ] || fail "stale lock did not permit session-start authority to decide"
  pass "known live locks refuse while stale locks defer to session start"
}

test_operator_delegation_and_nested_refusal() {
  local root="$TMP_ROOT/delegate-root" case_dir="$TMP_ROOT/delegate-case" output status
  make_runtime "$root"
  install_fixture "$case_dir" "$root"
  cat >"$root/bin/mx-doctor.sh" <<'SH'
#!/usr/bin/env bash
printf 'doctor:%s:%s:%s\n' "$MX_ROOT_OVERRIDE" "$MX_HOME" "$*"
SH
  cat >"$root/bin/mx-update.sh" <<'SH'
#!/usr/bin/env bash
printf 'update:%s:%s\n' "$MX_ROOT_OVERRIDE" "$MX_HOME"
SH
  chmod +x "$root/bin/mx-doctor.sh" "$root/bin/mx-update.sh"
  output=$("$case_dir/bin/multplx" doctor --json)
  [ "$output" = "doctor:$root:$root:--json" ] || fail "doctor delegation changed arguments or home"
  output=$("$case_dir/bin/multplx" update)
  [ "$output" = "update:$root:$root" ] || fail "update delegation changed root/home"
  if MULTPLX_ACTIVE=1 "$case_dir/bin/multplx" shell >/dev/null 2>&1; then status=0; else status=$?; fi
  expect_code 2 "$status" "nested activation"
  pass "operator commands delegate and nested activation refuses cleanly"
}

test_existing_install_paths_and_literal_safety
test_collisions_uninstall_and_private_preservation
test_atomic_interruption_recovery
test_managed_clone_and_linked_worktree_refusal
test_harness_cwd_arguments_environment_and_backend
test_live_lock_refusal_and_stale_permission
test_operator_delegation_and_nested_refusal
