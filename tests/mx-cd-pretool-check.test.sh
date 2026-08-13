#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2016
# Behavior tests for the cd-guard PreToolUse seatbelt (docs/cd-guard.md).
#
# The Rust command-policy module owns the block/allow decision and shared shell
# classification.
# bin/mx-cd-pretool-check.sh is the stable transport: it scopes the guard to the
# real primary checkout, then drives all five harness entry forms. This suite
# proves the decision matrix, the harness-output shaping, the primary-checkout
# scoping (including the deliberate daemon-home difference from the turn-end
# guard), the fail-open transport behavior, the prefilter fast path, the
# end-to-end cwd-leak regression, and the per-harness wiring. No harness is
# spawned; live per-harness evidence lives in docs/cd-guard.md.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mx_git_identity fmtest fmtest@example.invalid
TMP_ROOT=$(mx_test_tmproot mx-cd-pretool-check)
if [ "${MX_SUPERVISION_IMPLEMENTATION:-rust}" = rust ]; then
  export MX_RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}
fi

# A primary-shaped checkout: plain (non-worktree) git repo, AGENTS.md, bin/ with
# the transport and the release binary. This is what the transport's
# scoping treats as the real primary Multplx checkout.
install_cd_scripts() {
  local dir=$1
  mkdir -p "$dir/bin"
  cp "$ROOT/bin/mx-cd-pretool-check.sh" "$dir/bin/mx-cd-pretool-check.sh"
  cp "$ROOT/bin/mx-rust-runtime.sh" "$dir/bin/mx-rust-runtime.sh"
  chmod +x "$dir/bin/mx-cd-pretool-check.sh"
}

make_primary_fixture() {
  local dir=$1
  git init -q "$dir"
  git -C "$dir" commit -q --allow-empty -m init
  : > "$dir/AGENTS.md"
  install_cd_scripts "$dir"
  printf '%s\n' "$dir"
}

# Same shape as primary plus the .mx-daemon-home marker: a daemon's own
# primary session, which the cd-guard DOES guard (unlike the turn-end guard).
make_daemon_fixture() {
  local dir=$1
  make_primary_fixture "$dir" >/dev/null
  printf 'sm-cd-1\n' > "$dir/.mx-daemon-home"
  printf '%s\n' "$dir"
}

# A genuine linked git worktree - the shape bin/mx-spawn.sh hands actor/scout
# tasks. git-dir and git-common-dir differ, so the guard must be inert.
make_child_worktree_fixture() {
  local base=$1 dir=$2
  mx_git_worktree "$base" "$dir" mx/cd-guard-test-branch
  : > "$dir/AGENTS.md"
  install_cd_scripts "$dir"
  printf '%s\n' "$dir"
}

PRIMARY=$(make_primary_fixture "$TMP_ROOT/primary")
CHECK="$PRIMARY/bin/mx-cd-pretool-check.sh"

# --- full cross-harness acceptance matrix ----------------------------------

MATRIX_IDS=()
MATRIX_EXPECTED=()
MATRIX_COMMANDS=()

matrix_case() {
  MATRIX_IDS+=("$1")
  MATRIX_EXPECTED+=("$2")
  MATRIX_COMMANDS+=("$3")
}

# BLOCK: a persistent top-level cwd change in the parent shell.
matrix_case B01 deny 'cd projects/foo'
matrix_case B02 deny 'cd ..'
matrix_case B03 deny 'cd'
matrix_case B04 deny 'cd -'
matrix_case B05 deny 'cd /abs/path'
matrix_case B06 deny 'pushd projects/foo'
matrix_case B07 deny 'popd'
matrix_case B08 deny 'X=1 cd projects/foo'
matrix_case B09 deny 'cd projects/foo && bin/mx-backlog.sh add x'
matrix_case B10 deny 'echo before; cd projects/foo'
matrix_case B11 deny 'true && cd projects/foo'
matrix_case B12 deny 'bin/mx-backlog.sh done x || cd projects/foo'
matrix_case B13 deny 'cd "projects/foo"'
matrix_case B14 deny '"cd" projects/foo'
matrix_case B15 deny 'sleep 1 & cd projects/foo'
matrix_case B16 deny 'command cd projects/foo'
matrix_case B17 deny 'cd projects/foo >/dev/null'
matrix_case B18 deny $'cd projects/foo\necho done'
matrix_case B19 deny "\$'\\143d' projects/foo"
matrix_case B20 deny "c'd' projects/foo"
matrix_case B21 deny 'c"d" projects/foo'
matrix_case B22 deny 'c\d projects/foo'
matrix_case B23 deny 'builtin cd projects/foo'
matrix_case B24 deny 'command builtin cd projects/foo'
matrix_case B25 deny 'builtin command cd projects/foo'
matrix_case B26 deny 'command -p cd projects/foo'
matrix_case B27 deny 'command -- cd projects/foo'

# ALLOW: not a persistent top-level cwd change (scoped, data, or non-cd).
matrix_case A01 allow 'git -C projects/foo status'
matrix_case A02 allow 'cat /abs/path/file'
matrix_case A03 allow 'ls projects/foo'
matrix_case A04 allow 'echo "cd projects/foo"'
matrix_case A05 allow 'grep cd file'
matrix_case A06 allow '(cd projects/foo && pwd)'
matrix_case A07 allow "bash -c 'cd projects/foo'"
matrix_case A08 allow 'env -C projects/foo make'
matrix_case A09 allow 'make -C projects/foo build'
matrix_case A10 allow 'find . -execdir cd {} \;'
matrix_case A11 allow 'cd projects/foo | cat'
matrix_case A12 allow 'cat foo | cd bar'
matrix_case A13 allow 'cd projects/foo &'
matrix_case A14 allow 'abcd project'
matrix_case A15 allow 'cdk deploy'
matrix_case A16 allow 'env cd projects/foo'
matrix_case A17 allow 'sudo cd projects/foo'
matrix_case A18 allow 'x=$(cd foo && pwd)'
matrix_case A19 allow 'dirs'
matrix_case A20 allow "echo 'pushd x'"
matrix_case A21 allow 'git checkout main'
matrix_case A22 allow "sh -c 'cd projects/foo && ls'"
matrix_case A23 allow "printf '%s\\n' 'cd projects/foo'"
matrix_case A24 allow 'ls -la'
matrix_case A25 allow './cd projects/foo'
matrix_case A26 allow '/tmp/cd projects/foo'
matrix_case A27 allow '/usr/bin/cd projects/foo'
matrix_case A28 allow './builtin cd projects/foo'
matrix_case A29 allow 'c\d\ projects/foo'
matrix_case A30 allow './command cd projects/foo'
matrix_case A31 allow '/usr/bin/command cd projects/foo'
matrix_case A32 allow '/tmp/builtin cd projects/foo'
matrix_case A33 allow 'command -v cd'
matrix_case A34 allow 'command -V cd'
matrix_case A35 allow 'command -pv cd'
matrix_case A36 allow 'command -vp cd'

MATRIX_TMP=$(mktemp -d "${TMPDIR:-/tmp}/mx-cd-policy-matrix.XXXXXX")
MX_TEST_CLEANUP_DIRS+=("$MATRIX_TMP")

run_matrix_entry() {
  local id=$1 expected=$2 entry=$3 cmd=$4 payload out_file err_file rc
  out_file="$MATRIX_TMP/$id-$entry.out"
  err_file="$MATRIX_TMP/$id-$entry.err"

  case "$entry" in
    codex)
      payload=$(jq -cn --arg command "$cmd" '{tool_name:"Bash",tool_input:{command:$command}}')
      printf '%s' "$payload" | "$CHECK" >"$out_file" 2>"$err_file"
      rc=$?
      ;;
    claude)
      payload=$(jq -cn --arg command "$cmd" '{tool_name:"Bash",tool_input:{command:$command}}')
      printf '%s' "$payload" | "$CHECK" --claude >"$out_file" 2>"$err_file"
      rc=$?
      ;;
    pi)
      "$CHECK" --command "$cmd" >"$out_file" 2>"$err_file"
      rc=$?
      ;;
    *)
      fail "unknown matrix entry form: $entry"
      ;;
  esac

  if [ "$expected" = allow ]; then
    [ "$rc" -eq 0 ] || fail "$id via $entry must allow, got exit $rc: $(cat "$err_file")"
    [ ! -s "$out_file" ] || fail "$id via $entry allow must leave stdout empty: $(cat "$out_file")"
    [ ! -s "$err_file" ] || fail "$id via $entry allow must leave stderr empty: $(cat "$err_file")"
    return
  fi

  [ "$rc" -eq 2 ] || fail "$id via $entry must deny, got exit $rc"
  jq -e '.hookSpecificOutput.permissionDecision == "deny" and (.systemMessage | test("\\[persistent-cd\\]"))' "$err_file" >/dev/null 2>&1 \
    || fail "$id via $entry deny must carry the persistent-cd reason code on stderr: $(cat "$err_file")"
  if [ "$entry" = claude ]; then
    [ ! -s "$out_file" ] || fail "$id via claude deny must leave stdout empty: $(cat "$out_file")"
  fi
}

test_full_acceptance_matrix() {
  local i entry
  for ((i = 0; i < ${#MATRIX_IDS[@]}; i++)); do
    for entry in codex claude pi; do
      run_matrix_entry "${MATRIX_IDS[$i]}" "${MATRIX_EXPECTED[$i]}" "$entry" "${MATRIX_COMMANDS[$i]}"
    done
  done
  pass "cd-guard acceptance matrix: ${#MATRIX_IDS[@]} cases x 3 harness entry forms, block/allow all correct"
}

# --- primary-checkout scoping ----------------------------------------------

test_fires_in_daemon_home() {
  local dir out rc
  dir=$(make_daemon_fixture "$TMP_ROOT/daemon")
  out=$("$dir/bin/mx-cd-pretool-check.sh" --claude --command 'cd projects/foo' 2>&1); rc=$?
  expect_code 2 "$rc" "cd-guard must fire in a daemon's own primary session (unlike the turn-end guard)"
  assert_contains "$out" '[persistent-cd]' "daemon-home block must carry the reason code"
  pass "cd-guard: fires in a daemon home (its own primary session is a primary)"
}

test_inert_in_child_worktree() {
  local base dir out rc
  base="$TMP_ROOT/child-base"
  dir="$TMP_ROOT/child-wt"
  make_child_worktree_fixture "$base" "$dir" >/dev/null
  out=$("$dir/bin/mx-cd-pretool-check.sh" --claude --command 'cd projects/foo' 2>&1); rc=$?
  expect_code 0 "$rc" "cd-guard must be inert in an actor/scout linked worktree"
  [ -z "$out" ] || fail "cd-guard produced output in a child worktree: $out"
  pass "cd-guard: inert in an actor/scout task worktree (linked git worktree)"
}

test_inert_when_not_broker_repo() {
  local dir out rc
  dir="$TMP_ROOT/not-broker"
  git init -q "$dir"
  git -C "$dir" commit -q --allow-empty -m init
  install_cd_scripts "$dir"   # bin/ present but no AGENTS.md
  out=$("$dir/bin/mx-cd-pretool-check.sh" --claude --command 'cd projects/foo' 2>&1); rc=$?
  expect_code 0 "$rc" "cd-guard must be inert without AGENTS.md (not a Multplx checkout)"
  [ -z "$out" ] || fail "cd-guard produced output outside a Multplx checkout: $out"
  pass "cd-guard: inert in a non-Multplx repo (no AGENTS.md)"
}

test_inert_when_not_a_git_repo() {
  local dir out rc
  dir="$TMP_ROOT/no-git"
  mkdir -p "$dir"
  : > "$dir/AGENTS.md"
  install_cd_scripts "$dir"   # AGENTS.md + bin/ but no git repo
  out=$("$dir/bin/mx-cd-pretool-check.sh" --claude --command 'cd projects/foo' 2>&1); rc=$?
  expect_code 0 "$rc" "cd-guard must be inert when the checkout is not a git repo"
  [ -z "$out" ] || fail "cd-guard produced output in a non-git dir: $out"
  pass "cd-guard: inert when not inside a git repo"
}

# --- end-to-end cwd-leak regression ----------------------------------------

test_e2e_cwd_leak_regression() {
  local sandbox home home_updated leaked out rc
  sandbox="$TMP_ROOT/e2e"
  home="$sandbox/home"
  mkdir -p "$home/data" "$home/projects/clone/data"
  printf '## In flight\n' > "$home/data/backlog.md"

  # Without the guard, the persistent primary shell's cwd leaks: a stray
  # `cd projects/clone` makes the next broker-owned backlog write land in the
  # clone, and the home backlog is never updated.
  (
    cd "$home" || fail "cannot enter home"
    cd projects/clone || fail "cannot enter clone"
    printf -- '- [x] demo done\n' >> data/backlog.md
  )
  home_updated=0
  grep -q 'demo done' "$home/data/backlog.md" && home_updated=1
  leaked=0
  grep -q 'demo done' "$home/projects/clone/data/backlog.md" 2>/dev/null && leaked=1
  [ "$home_updated" -eq 0 ] || fail "baseline: home backlog was updated, cwd leak did not reproduce"
  [ "$leaked" -eq 1 ] || fail "baseline: backlog write did not leak into the clone"

  # With the guard, the exact stray command is denied before it can run, so the
  # real harness never lets cwd leave the home.
  out=$("$CHECK" --claude --command 'cd projects/clone' 2>&1); rc=$?
  expect_code 2 "$rc" "guard must deny the stray persistent cd that caused the leak"
  assert_contains "$out" '[persistent-cd]' "leak-preventing block must carry the reason code"
  pass "cd-guard: reproduces the cwd leak and denies the exact command that causes it"
}

# --- fail-open transport behavior ------------------------------------------

test_fail_open_empty_stdin() {
  local out rc
  out=$("$CHECK" < /dev/null 2>&1); rc=$?
  expect_code 0 "$rc" "transport must exit 0 on empty stdin"
  [ -z "$out" ] || fail "transport produced output on empty stdin: $out"
  pass "cd-guard: fails open on empty stdin"
}

test_fail_open_unparseable_json() {
  local out rc
  out=$(printf 'not json at all' | "$CHECK" 2>&1); rc=$?
  expect_code 0 "$rc" "transport must exit 0 on unparseable stdin JSON"
  [ -z "$out" ] || fail "transport produced output on unparseable JSON: $out"
  pass "cd-guard: fails open on unparseable stdin JSON"
}

test_policy_runtime_without_node() {
  local fakebin tool tool_path out rc
  fakebin=$(mx_fakebin "$TMP_ROOT/nonode")
  for tool in bash sh git dirname cat printf sed tr jq; do
    tool_path=$(command -v "$tool") || continue
    ln -s "$tool_path" "$fakebin/$tool"
  done
  # node deliberately absent from this PATH.
  out=$(PATH="$fakebin" "$CHECK" --command 'cd projects/foo' 2>&1); rc=$?
  expect_code 2 "$rc" "Rust policy must deny independently of Node availability"
  assert_contains "$out" '[persistent-cd]' "Rust deny without Node must preserve the reason code"
  pass "cd-guard: Rust policy does not depend on Node"
}

test_fail_open_missing_jq_on_stdin() {
  local fakebin tool tool_path out rc
  fakebin=$(mx_fakebin "$TMP_ROOT/nojq")
  for tool in bash sh git dirname cat printf sed tr node; do
    tool_path=$(command -v "$tool") || continue
    ln -s "$tool_path" "$fakebin/$tool"
  done
  # jq deliberately absent: the stdin transport cannot extract the command.
  out=$(printf '{"tool_input":{"command":"cd projects/foo"}}' | PATH="$fakebin" "$CHECK" 2>&1); rc=$?
  expect_code 0 "$rc" "stdin transport must fail open when jq is unavailable"
  [ -z "$out" ] || fail "transport produced output without jq on the stdin path: $out"
  pass "cd-guard: fails open on the stdin path when jq is missing"
}

# --- prefilter fast path ----------------------------------------------------

test_prefilter_skips_policy_without_cd_substring() {
  local dir fakebin marker tool tool_path out rc
  dir="$TMP_ROOT/prefilter"
  make_primary_fixture "$dir" >/dev/null
  fakebin=$(mx_fakebin "$TMP_ROOT/prefilter-fake")
  marker="$TMP_ROOT/prefilter-node-called"
  for tool in bash sh git dirname cat printf sed tr jq; do
    tool_path=$(command -v "$tool") || continue
    ln -s "$tool_path" "$fakebin/$tool"
  done
  cat > "$fakebin/node" <<EOF
#!/usr/bin/env bash
: > "$marker"
exit 0
EOF
  chmod +x "$fakebin/node"
  # No cd/pushd/popd substring: the prefilter must fast-allow before scoping or
  # the policy runtime is ever consulted.
  out=$(PATH="$fakebin" "$dir/bin/mx-cd-pretool-check.sh" --command 'git status' 2>&1); rc=$?
  expect_code 0 "$rc" "prefilter must fast-allow a command with no cd/pushd/popd substring"
  [ -z "$out" ] || fail "prefilter fast-allow produced output: $out"
  [ ! -e "$marker" ] || fail "prefilter fast-allow invoked an unrelated Node sentinel"
  pass "cd-guard: prefilter fast-allows when no cd/pushd/popd substring is present"
}

# --- policy CLI contract ----------------------------------------------------

# --- per-harness wiring -----------------------------------------------------

test_claude_wiring() {
  local settings n
  settings="$ROOT/.claude/settings.json"
  [ -f "$settings" ] || fail "tracked .claude/settings.json is missing"
  n=$(jq -r '[.hooks.PreToolUse[0].hooks[].command | select(contains("mx-cd-pretool-check.sh"))] | length' "$settings")
  [ "$n" = 1 ] || fail "claude PreToolUse must invoke mx-cd-pretool-check.sh exactly once"
  jq -e '[.hooks.PreToolUse[0].hooks[].command | select(contains("mx-cd-pretool-check.sh") and contains("--claude") and contains("CLAUDE_PROJECT_DIR"))] | length == 1' "$settings" >/dev/null \
    || fail "claude cd hook must use CLAUDE_PROJECT_DIR and --claude"
  jq -e '[.hooks.PreToolUse[0].hooks[].command | select(contains("mx-arm-pretool-check.sh"))] | length == 1' "$settings" >/dev/null \
    || fail "claude cd hook must not displace the watcher-arm hook"
  pass ".claude/settings.json: PreToolUse invokes the cd-guard alongside the arm guard"
}

test_codex_wiring() {
  local settings command
  settings="$ROOT/.codex/hooks.json"
  [ -f "$settings" ] || fail "tracked .codex/hooks.json is missing"
  command=$(jq -r '[.hooks.PreToolUse[0].hooks[].command | select(contains("mx-cd-pretool-check.sh"))][0] // empty' "$settings")
  [ -n "$command" ] || fail "codex PreToolUse must invoke mx-cd-pretool-check.sh"
  assert_contains "$command" 'pwd -P' "codex cd hook must anchor from the hook process working directory"
  assert_contains "$command" 'mx-cd-pretool-check.sh' "codex cd hook must invoke the cd-guard"
  jq -e '[.hooks.PreToolUse[0].hooks[].command | select(contains("mx-arm-pretool-check.sh"))] | length == 1' "$settings" >/dev/null \
    || fail "codex cd hook must not displace the watcher-arm hook"
  pass ".codex/hooks.json: PreToolUse invokes the cd-guard alongside the arm guard"
}

test_pi_wiring() {
  local ext content
  ext="$ROOT/.pi/extensions/mx-primary-turnend-guard.ts"
  [ -f "$ext" ] || fail "tracked pi primary extension is missing"
  content=$(cat "$ext")
  assert_contains "$content" 'runCdCheck(command)' "pi extension must run the cd check in tool_call"
  assert_contains "$content" 'mx-cd-pretool-check.sh' "pi extension must invoke the cd-guard owner"
  assert_contains "$content" 'runPretoolCheck(command)' "pi extension must keep running the watcher-arm check"
  assert_contains "$content" 'return { block: true, reason:' "pi extension must block on a checker exit 2"
  pass ".pi primary extension: tool_call runs the cd-guard alongside the watcher-arm check"
}

test_full_acceptance_matrix
test_fires_in_daemon_home
test_inert_in_child_worktree
test_inert_when_not_broker_repo
test_inert_when_not_a_git_repo
test_e2e_cwd_leak_regression
test_fail_open_empty_stdin
test_fail_open_unparseable_json
test_policy_runtime_without_node
test_fail_open_missing_jq_on_stdin
test_prefilter_skips_policy_without_cd_substring
test_claude_wiring
test_codex_wiring
test_pi_wiring
