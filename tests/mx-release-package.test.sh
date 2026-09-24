#!/usr/bin/env bash
# Package-only install, upgrade, recovery, persistent-home and uninstall checks.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_test_tmproot_into TMP_ROOT mx-release-package
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)

package="$TMP_ROOT/multplx-package"
release_binary=${MX_RUST_BIN:-$ROOT/target/release/mx}
"$ROOT/bin/mx-release-package.sh" "$package" "$release_binary" >/dev/null
[ -f "$package/runtime/AGENTS.md" ] || fail 'package omitted the runtime contract'
[ ! -e "$package/runtime/AGENTS_E.md" ] || fail 'package exposed the dormant development filename'
if [ -f "$ROOT/AGENTS.md" ]; then
  source_contract=$ROOT/AGENTS.md
else
  source_contract=$ROOT/AGENTS_E.md
fi
cmp -s "$source_contract" "$package/runtime/AGENTS.md" \
  || fail 'package did not publish the source contract as canonical AGENTS.md'
assert_no_grep 'porting.md\|CLAUDE.md\|dormant release contract\|do not activate' \
  "$package/runtime/AGENTS.md" \
  'packaged runtime contract contains development-only instructions or links'
package_help=$("$package/bin/mx" --help)
assert_contains "$package_help" 'seeded persistent sub-agent home' \
  'packaged help retained an active daemon-role label for backlog handoff'
assert_contains "$package_help" 'legacy publication-mode and yolo compatibility settings' \
  'packaged help presented legacy project settings as current policy'
assert_contains "$package_help" 'live persistent sub-agent homes' \
  'packaged help retained an active daemon-role label for config propagation'
[ ! -e "$package/.git" ] && [ ! -e "$package/runtime/.git" ] \
  || fail 'package contains source Git state'
[ ! -e "$package/runtime/docs/calm-mode-feasibility.md" ] \
  && ! find "$package/runtime/docs" -maxdepth 1 -name 'mx-test-*.json' -print -quit | grep -q . \
  || fail 'package included development evidence instead of runtime documentation'
if LC_ALL=C grep -R -I -q '/Users/' "$package/runtime"; then
  fail 'package included a private macOS user-home path'
fi
for required in \
  runtime/.codex/hooks.json runtime/.cursor/hooks.json \
  runtime/.pi/extensions/mx-primary-turnend-guard.ts \
  runtime/docs/supervision-protocols/codex.md \
  runtime/workflows/new-feature.workflow.md; do
  [ -f "$package/$required" ] || fail "package omitted $required"
done
pass 'release package contains version-matched public runtime assets without a source checkout'

canonical_source="$TMP_ROOT/canonical-source"
mkdir -p "$canonical_source/bin"
cp "$ROOT/bin/mx-release-package.sh" "$canonical_source/bin/mx-release-package.sh"
cp "$ROOT/Cargo.toml" "$canonical_source/Cargo.toml"
printf '# canonical release contract\n' >"$canonical_source/AGENTS.md"
git -C "$canonical_source" init -q
git -C "$canonical_source" add AGENTS.md Cargo.toml bin/mx-release-package.sh
canonical_package="$TMP_ROOT/canonical-package"
"$canonical_source/bin/mx-release-package.sh" "$canonical_package" "$release_binary" >/dev/null
assert_grep 'canonical release contract' "$canonical_package/runtime/AGENTS.md" \
  'release package did not accept the canonical cutover filename'
[ ! -e "$canonical_package/runtime/AGENTS_E.md" ] \
  || fail 'canonical package exposed a dormant development filename'
pass 'release packaging accepts the canonical post-cutover contract filename'

tampered="$TMP_ROOT/tampered"
cp -R "$package" "$tampered"
printf '\ntampered\n' >>"$tampered/runtime/AGENTS.md"
mkdir -p "$TMP_ROOT/tampered-install"
if "$tampered/bin/mx" launcher-install --package "$tampered" \
    --bin-dir "$TMP_ROOT/tampered-install/bin" \
    --config-dir "$TMP_ROOT/tampered-install/config" \
    --data-dir "$TMP_ROOT/tampered-install/data" >/dev/null 2>&1; then
  fail 'installer accepted a package with changed assets'
fi
[ ! -e "$TMP_ROOT/tampered-install/bin/multplx" ] \
  || fail 'tampered package published a binary'
pass 'package validation refuses changed assets before publication'

install="$TMP_ROOT/install"
mkdir -p "$TMP_ROOT/unrelated"
(
  cd "$TMP_ROOT/unrelated"
  env -u MX_RUST_SOURCE_ROOT -u MX_LAUNCHER_DEFAULT_ROOT \
    "$package/bin/mx" launcher-install --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" \
      --data-dir "$install/data" >/dev/null
)
[ "$(cat "$install/config/root")" = "$install/data/runtime" ] \
  || fail 'package root record does not select installed assets'
[ "$(cat "$install/config/home")" = "$install/data/home" ] \
  || fail 'package home record does not select persistent state'
[ ! -e "$install/data/runtime/.git" ] || fail 'install fabricated a runtime repository'
output=$(cd "$TMP_ROOT/unrelated" && "$install/bin/multplx" paths)
assert_contains "$output" "root=$install/data/runtime" 'installed package did not launch away from source'
assert_contains "$output" "home=$install/data/home" 'installed package lost its persistent home'
pass 'package installs and launches from an unrelated directory without Rust or a source checkout'

launcher_help=$(env -u MX_MULTICALL_EXPLICIT "$install/bin/multplx" --help) \
  || fail 'installed multplx --help failed without explicit multicall mode'
assert_contains "$launcher_help" 'multplx PATH|ALIAS' \
  'installed multplx --help did not reach the public launcher'
env -u MX_MULTICALL_EXPLICIT "$install/bin/multplx" task --help >/dev/null \
  || fail 'installed multplx task --help failed without explicit multicall mode'
env -u MX_MULTICALL_EXPLICIT "$install/bin/multplx" project --help >/dev/null \
  || fail 'installed multplx project --help failed without explicit multicall mode'
pass 'installed global help, task and project commands retain launcher dispatch'

for wrapper in \
  mx-workflow.sh mx-deep-review.sh mx-timeline.sh mx-headroom.sh \
  mx-backlog.sh mx-system-view.sh mx-system-snapshot.sh; do
  if wrapper_help=$(env -u MX_MULTICALL_EXPLICIT \
      MX_RUST_SOURCE_ROOT="$install/data/runtime" \
      MX_ROOT_OVERRIDE="$install/data/runtime" MX_HOME="$install/data/home" \
      MX_RUST_BIN="$install/bin/multplx" \
      "$install/data/runtime/bin/$wrapper" --help 2>&1); then
    :
  else
    fail "installed $wrapper --help failed with MX_MULTICALL_EXPLICIT unset: $wrapper_help"
  fi
done
pass 'installed documented command wrappers dispatch explicitly through the multplx binary'

project="$TMP_ROOT/borrowed-project"
mx_git_init_commit "$project"
printf 'borrowed sentinel\n' >"$project/untracked-sentinel"
home_output=$(MX_MULTICALL_EXPLICIT=1 MX_ROOT_OVERRIDE="$install/data/runtime" \
  MX_HOME="$install/data/home" MX_DAEMON_CHARTER='Package home' \
  "$install/bin/multplx" home-seed packaged-worker - "$project") \
  || fail "packaged private home seed failed: $home_output"
private_home=$(printf '%s\n' "$home_output" | sed -n 's/^home=//p' | tail -n 1)
[ -d "$private_home" ] && [ ! -e "$private_home/.git" ] \
  || fail 'packaged persistent home was not a private non-Git directory'
[ -f "$private_home/AGENTS.md" ] && [ -d "$private_home/.agents/skills" ] \
  || fail 'packaged persistent home lost installed assets'
[ "$(cat "$project/untracked-sentinel")" = 'borrowed sentinel' ] \
  || fail 'persistent-home provisioning changed the borrowed repository'
[ -z "$(git -C "$project" status --porcelain --untracked-files=no)" ] \
  || fail 'persistent-home provisioning changed tracked borrowed-repository state'
pass 'packaged persistent homes stay private and retain borrowed repository ownership'

fakebin="$TMP_ROOT/no-source-tools"
mkdir -p "$fakebin"
for forbidden in cargo rustc treehouse; do
  cat >"$fakebin/$forbidden" <<SH
#!/bin/sh
printf '%s\n' '$forbidden' >>'$TMP_ROOT/forbidden-tools-invoked'
exit 99
SH
  chmod +x "$fakebin/$forbidden"
done
cat >"$fakebin/gh" <<'SH'
#!/bin/sh
exit 0
SH
chmod +x "$fakebin/gh"
cat >"$fakebin/codex" <<'SH'
#!/bin/sh
if [ "${1-}" = --dispatch-smoke ]; then
  [ -x "${MX_RUST_BIN:-}" ] || exit 91
  command -v multplx >/dev/null || exit 92
  multplx task --help >/dev/null || exit 93
  multplx project --help >/dev/null || exit 94
  MX_MULTICALL_EXPLICIT=1 "$MX_LAUNCH_BIN_PATH" operational-input --help >/dev/null || exit 93
  printf 'public-dispatch-ok\n'
  exit 0
fi
if [ "${1-}" = --package-hook-smoke ]; then
  [ -x "${MX_RUST_BIN:-}" ] || exit 81
  command -v mx >/dev/null || exit 84
  mx --help >/dev/null || exit 85
  "$MX_RUST_BIN" primitive primary-scope "$MX_ROOT_OVERRIDE" "$MX_HOME/state" || exit 86
  if MX_TASK_ID=worker MX_CURRENT_ADMISSION_ID=worker \
      "$MX_RUST_BIN" primitive primary-scope "$MX_ROOT_OVERRIDE" "$MX_HOME/state"; then
    exit 88
  fi
  if DEEP_REVIEW_GATE=1 MX_GATE_REFUSE_BYPASS=0 \
      "$MX_RUST_BIN" primitive primary-scope "$MX_ROOT_OVERRIDE" "$MX_HOME/state"; then
    exit 89
  fi
  nudge=$("$MX_ROOT_OVERRIDE/bin/mx-sessionstart-nudge.sh") || exit 82
  printf '%s\n' "$nudge" | grep 'MULTPLX_OP: v1 session-start:' >/dev/null || exit 87
  printf '%s\n' '{"tool_name":"Read","tool_input":{}}' \
    | "$MX_ROOT_OVERRIDE/bin/mx-subagent-pretool-check.sh" >/dev/null || exit 83
  printf 'packaged-hooks-ok\n'
  exit 0
fi
printf 'packaged-codex 1.0\n'
SH
chmod +x "$fakebin/codex"
runtime_path="/usr/bin:/bin"
case "$(uname -s)" in
  Darwin) runtime_path="/usr/bin:/bin:/usr/sbin:/sbin" ;;
esac
stale_bin="$TMP_ROOT/stale-global-bin"
mkdir -p "$stale_bin"
cat >"$stale_bin/multplx" <<SH
#!/bin/sh
printf 'stale-global-multplx\n' >>'$TMP_ROOT/stale-multplx-invoked'
exit 97
SH
chmod +x "$stale_bin/multplx"
package_mx() {
  PATH="$fakebin:$runtime_path" MX_MULTICALL_EXPLICIT=1 \
    MX_ROOT_OVERRIDE="$install/data/runtime" MX_HOME="$install/data/home" \
    MX_STATE_OVERRIDE="$install/data/home/state" \
    MX_DATA_OVERRIDE="$install/data/home/data" \
    MX_CONFIG_OVERRIDE="$install/data/home/config" \
    "$install/bin/multplx" "$@"
}

PATH="$fakebin:$runtime_path" "$install/bin/multplx" doctor --json --check tools \
  >"$TMP_ROOT/package-doctor.json" \
  || fail 'packaged startup dependency check failed with declared tools present'
jq -e '.summary == {ok:1,warn:0,fail:0} and
  .findings[0].message == "required tools are present; worktrees use built-in Git allocation"' \
  "$TMP_ROOT/package-doctor.json" >/dev/null \
  || fail 'packaged dependency check did not select the built-in Git manager'
PATH="$fakebin:$runtime_path" "$install/bin/multplx" workspace --plain \
  >"$TMP_ROOT/package-workspace.txt" \
  || fail 'package-only workspace startup failed'
assert_grep 'Multplx workspace' "$TMP_ROOT/package-workspace.txt" \
  'package-only workspace did not render'
PATH="$fakebin:$runtime_path" MX_REAL_CODEX="$fakebin/codex" \
  MX_LAUNCH_BIN_PATH="$install/bin/multplx" \
  MX_RUST_BIN="$install/bin/multplx" \
  MX_ROOT_OVERRIDE="$install/data/runtime" MX_HOME="$install/data/home" \
  env -u MX_LAUNCH_VALIDATED \
  "$install/data/runtime/bin/mx-launch-harness.sh" codex --version \
  >"$TMP_ROOT/package-harness-version.txt" \
  || fail 'packaged harness adapter rejected its non-Git runtime'
assert_grep 'packaged-codex 1.0' "$TMP_ROOT/package-harness-version.txt" \
  'packaged harness adapter did not execute the detected harness'
(
  cd "$project"
  PATH="$stale_bin:$fakebin:$runtime_path" MX_MULTICALL_EXPLICIT=1 \
    MX_ROOT_OVERRIDE="$install/data/runtime" MX_HOME="$install/data/home" \
    "$install/bin/multplx" launcher codex --dispatch-smoke
) >"$TMP_ROOT/package-harness-dispatch.txt" 2>"$TMP_ROOT/package-harness-dispatch.err" \
  || { cat "$TMP_ROOT/package-harness-dispatch.err" >&2; fail 'installed provider did not resolve public commands through the captured package runtime'; }
assert_grep 'public-dispatch-ok' "$TMP_ROOT/package-harness-dispatch.txt" \
  'installed provider dispatch smoke did not complete'
 [ ! -e "$TMP_ROOT/stale-multplx-invoked" ] \
  || fail 'installed provider resolved multplx from stale inherited PATH'
pass 'installed provider resolves public task and project commands through the captured runtime'
cat >"$fakebin/sh" <<'SH'
#!/bin/sh
command -v multplx >/dev/null || exit 94
multplx task --help >/dev/null || exit 95
multplx project --help >/dev/null || exit 96
MX_MULTICALL_EXPLICIT=1 "$MX_LAUNCH_BIN_PATH" operational-input --help >/dev/null || exit 96
printf 'activated-dispatch-ok\n'
SH
chmod +x "$fakebin/sh"
PATH="$stale_bin:$fakebin:$runtime_path" MX_LAUNCH_SHELL="$fakebin/sh" \
  MX_RUST_BIN="$install/bin/multplx" \
  MX_MULTICALL_EXPLICIT=1 \
  "$install/bin/multplx" launcher shell >"$TMP_ROOT/activated-shell-dispatch.txt" \
  || fail 'installed activated shell did not receive public dispatch while retaining explicit internal dispatch'
assert_grep 'activated-dispatch-ok' "$TMP_ROOT/activated-shell-dispatch.txt" \
  'installed activated shell dispatch smoke did not complete'
 [ ! -e "$TMP_ROOT/stale-multplx-invoked" ] \
  || fail 'activated shell resolved multplx from stale inherited PATH'
pass 'installed activated shell resolves public commands through the captured runtime'
if PATH="$fakebin:$runtime_path" MX_REAL_CODEX="$fakebin/codex" \
    env -u MX_RUST_BIN -u MX_LAUNCH_BIN_PATH \
    "$install/bin/multplx" codex --package-hook-smoke \
    >"$TMP_ROOT/package-hooks.txt" 2>"$TMP_ROOT/package-hooks.err"; then
  package_hook_status=0
else
  package_hook_status=$?
fi
[ "$package_hook_status" -eq 0 ] \
  || fail "packaged launcher runtime hook smoke exited $package_hook_status: $(cat "$TMP_ROOT/package-hooks.err" "$TMP_ROOT/package-hooks.txt")"
assert_grep 'packaged-hooks-ok' "$TMP_ROOT/package-hooks.txt" \
  'packaged SessionStart or PreToolUse adapter could not resolve the installed runtime'
pass 'package-launched SessionStart and PreToolUse adapters resolve the installed Rust runtime'

mkdir -p "$TMP_ROOT/package-mcp-state"
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"package-test","version":"1"}}}' \
  | env -i PATH="$runtime_path" MX_TASK_ID=package-mcp \
      MX_HOME="$install/data/home" MX_REPORT_STATE_OVERRIDE="$TMP_ROOT/package-mcp-state" \
      MX_RUST_BIN="$install/bin/multplx" MX_MULTICALL_EXPLICIT=1 \
      "$install/data/runtime/bin/mx-report-mcp" >"$TMP_ROOT/package-mcp.txt" \
  || fail 'packaged report MCP could not start from its explicit server environment'
jq -e 'select(.id == 1) | .result.protocolVersion == "2025-06-18"' \
  "$TMP_ROOT/package-mcp.txt" >/dev/null \
  || fail 'packaged report MCP omitted its initialize response'
pass 'packaged report MCP binds the installed runtime without inherited launcher state'

repo_two="$TMP_ROOT/nested/team-two/service"
repo_three="$TMP_ROOT/another depth/team-three/tool"
mx_git_init_commit "$repo_two"
mx_git_init_commit "$repo_three"
for item in "one:$project" "two:$repo_two" "three:$repo_three"; do
  label=${item%%:*}
  repo_path=${item#*:}
  package_mx task --project "$repo_path" --request-id "package-$label" \
    --batch-id package-multi --task-id "package-$label" "Package task $label" \
    >"$TMP_ROOT/task-$label.json" || fail "package task intake failed for $label"
  jq -e --arg task "package-$label" '
    .newly_accepted == true and .implementation_started == false and
    .request.task_id == $task and .request.batch_id == "package-multi"
  ' "$TMP_ROOT/task-$label.json" >/dev/null \
    || fail "package task receipt differs for $label"
  base=$(git -C "$repo_path" rev-parse HEAD)
  package_mx worktree acquire "$repo_path" --request "package-allocation-$label" \
    --task "package-$label" --attempt "package-$label-attempt" --base "$base" \
    >"$TMP_ROOT/allocation-$label.json" || fail "package allocation failed for $label"
  allocation_path=$(jq -r '.binding.path' "$TMP_ROOT/allocation-$label.json")
  [ "$allocation_path" != "$repo_path" ] && [ "$(git -C "$allocation_path" rev-parse HEAD)" = "$base" ] \
    || fail "package allocation reused or changed borrowed checkout $label"
done
jq -s -e 'map(.request.project_id) | unique | length == 3' \
  "$TMP_ROOT/task-one.json" "$TMP_ROOT/task-two.json" "$TMP_ROOT/task-three.json" >/dev/null \
  || fail 'package multi-repository intake conflated project identities'
[ "$(cat "$project/untracked-sentinel")" = 'borrowed sentinel' ] \
  || fail 'package multi-repository work changed the borrowed checkout'
[ ! -e "$TMP_ROOT/forbidden-tools-invoked" ] \
  || fail "package-only startup invoked $(tr '\n' ' ' <"$TMP_ROOT/forbidden-tools-invoked")"
pass 'package-only startup and three-repository work use declared tools and built-in worktrees'

printf '{}\n' >"$install/data/home/state/workspace-launch.json"
if "$package/bin/mx" launcher-install --upgrade --package "$package" \
    --home "$TMP_ROOT/proposed-other-home" \
    --bin-dir "$install/bin" --config-dir "$install/config" \
    --data-dir "$install/data" >"$TMP_ROOT/reserved-upgrade.out" \
    2>"$TMP_ROOT/reserved-upgrade.err"; then
  fail 'package upgrade changed assets while a launch reservation existed'
fi
assert_grep 'unresolved workspace launch' "$TMP_ROOT/reserved-upgrade.err" \
  'package upgrade did not explain the unresolved launch boundary'
rm "$install/data/home/state/workspace-launch.json"

printf 'kind=actor\nbackend=tmux\n' >"$install/data/home/state/recorded-user.meta"
if "$package/bin/mx" launcher-install --upgrade --package "$package" \
    --bin-dir "$install/bin" --config-dir "$install/config" \
    --data-dir "$install/data" >"$TMP_ROOT/in-use-upgrade.out" \
    2>"$TMP_ROOT/in-use-upgrade.err"; then
  fail 'package upgrade changed assets while a recorded task user existed'
fi
assert_grep 'recorded task user' "$TMP_ROOT/in-use-upgrade.err" \
  'package upgrade did not explain the active-use boundary'
if "$package/bin/mx" launcher-install --uninstall \
    --bin-dir "$install/bin" --config-dir "$install/config" \
    --data-dir "$install/data" >"$TMP_ROOT/in-use-uninstall.out" \
    2>"$TMP_ROOT/in-use-uninstall.err"; then
  fail 'package uninstall removed assets while a recorded task user existed'
fi
assert_grep 'recorded task user' "$TMP_ROOT/in-use-uninstall.err" \
  'package uninstall did not explain the active-use boundary'
rm "$install/data/home/state/recorded-user.meta"
pass 'package upgrade and uninstall refuse recorded or uncertain runtime users'

# Create a real canonical worker through the installed spawn and typed report
# paths. Its schedule remains unfinished while the exact cmux endpoint is stopped.
worker_project="$TMP_ROOT/upgrade-worker-project"
mx_git_init_commit "$worker_project"
cmux="$TMP_ROOT/upgrade-cmux"
cat >"$cmux" <<'SH'
#!/bin/sh
set -eu
case "$1" in
  version) echo 'cmux 0.64.17 (97) [abcdef1]' ;;
  ping) echo PONG ;;
  workspace)
    [ ! -f "$MX_CMUX_FIXTURE/fail-inventory" ] || exit 99
    if [ -f "$MX_CMUX_FIXTURE/live-at-probe" ]; then
      count=0
      [ ! -f "$MX_CMUX_FIXTURE/probe-count" ] || count=$(cat "$MX_CMUX_FIXTURE/probe-count")
      count=$((count + 1))
      printf '%s' "$count" >"$MX_CMUX_FIXTURE/probe-count"
      if [ "$count" -ge "$(cat "$MX_CMUX_FIXTURE/live-at-probe")" ]; then
        printf '{"workspaces":[{"id":"11111111-1111-4111-8111-111111111111","title":"%s"}]}' "$(cat "$MX_CMUX_FIXTURE/expected-title")"
      else echo '{"workspaces":[]}'; fi
      exit 0
    fi
    if [ -f "$MX_CMUX_FIXTURE/title" ]; then
      printf '{"workspaces":[{"id":"11111111-1111-4111-8111-111111111111","title":"%s"}]}' "$(cat "$MX_CMUX_FIXTURE/title")"
    else echo '{"workspaces":[]}'; fi ;;
  new-workspace)
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in --name) shift; printf '%s' "$1" >"$MX_CMUX_FIXTURE/title";; esac
      shift
    done ;;
  list-panes) echo '{"panes":[{"selected_surface_id":"22222222-2222-4222-8222-222222222222","surface_ids":["22222222-2222-4222-8222-222222222222"]}]}' ;;
  send|send-key) : ;;
  close-workspace) rm -f "$MX_CMUX_FIXTURE/title" ;;
  *) exit 99 ;;
esac
SH
chmod +x "$cmux"
mkdir -p "$TMP_ROOT/upgrade-cmux-bin"
ln -s "$cmux" "$TMP_ROOT/upgrade-cmux-bin/cmux"
mkdir -p "$TMP_ROOT/upgrade-cmux-state"
export MX_CMUX_BIN="$cmux" MX_CMUX_FIXTURE="$TMP_ROOT/upgrade-cmux-state"
export PATH="$TMP_ROOT/upgrade-cmux-bin:$PATH"
export MX_REAL_CODEX=/bin/true
export MX_HOME="$install/data/home" MX_ROOT_OVERRIDE="$install/data/runtime"
export MX_HEADROOM_SKIP_QUEUE=1 MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0
export MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 MX_HEADROOM_API_CAPACITY=8 MX_HEADROOM_IN_USE=0
mkdir -p "$install/data/home/data/upgrade-worker"
printf 'Inspect this upgrade fixture and leave its work unfinished.\n' \
  >"$install/data/home/data/upgrade-worker/brief.md"
"$install/bin/multplx" spawn upgrade-worker "$worker_project" --backend cmux --harness codex \
  >"$TMP_ROOT/upgrade-worker-spawn.out" || fail 'installed typed spawn did not create upgrade fixture'
worker_meta="$install/data/home/state/upgrade-worker.meta"
worker_report="$install/data/home/state/upgrade-worker.status"
worker_attempt=$(sed -n 's/^canonical_model=//p' "$worker_meta" | jq -r '.attempt.id')
worker_generation=$(sed -n 's/^canonical_model=//p' "$worker_meta" | jq -r '.attempt.generation')
worker_revision=$(sed -n 's/^canonical_model=//p' "$worker_meta" | jq -r '.attempt.brief_revision')
cp "$MX_CMUX_FIXTURE/title" "$MX_CMUX_FIXTURE/expected-title"
MX_MULTICALL_EXPLICIT=1 MX_TASK_ID=upgrade-worker MX_ATTEMPT_ID="$worker_attempt" \
  MX_ATTEMPT_GENERATION="$worker_generation" MX_BRIEF_REVISION="$worker_revision" \
  "$install/bin/multplx" supervision mx-report --id upgrade-worker --state working \
    --message 'unfinished work retained for upgrade test' --message-id upgrade-progress \
    >/dev/null || fail 'typed task report did not record current progress'
sed -n 's/^canonical_model=//p' "$worker_meta" \
  | jq -e '(.schedule.state | ascii_downcase) != "completed"' >/dev/null \
  || fail 'worker fixture incorrectly claims task completion'
cp "$worker_meta" "$TMP_ROOT/worker-meta.before"
cp "$worker_report" "$TMP_ROOT/worker-report.before"
cp "$install/data/home/state/.spawn-actions/upgrade-worker.json" "$TMP_ROOT/worker-receipt.before"
worker_worktree=$(jq -r '.binding.allocation.path' "$install/data/home/state/.spawn-actions/upgrade-worker.json")
worker_allocation=$(jq -r '.binding.allocation.allocation_id' "$install/data/home/state/.spawn-actions/upgrade-worker.json")
worker_lease=$(jq -r '.binding.allocation.lease_id' "$install/data/home/state/.spawn-actions/upgrade-worker.json")
worker_lease_record=$(find "$TMP_ROOT" -type f -path '*/records/*.json' \
  -exec grep -l -F "$worker_allocation" {} + | head -n 1)
[ -n "$worker_lease_record" ] || fail 'could not locate the canonical allocation lease record'
cp "$worker_lease_record" "$TMP_ROOT/worker-lease.before"
worker_commit=$(git -C "$worker_project" rev-parse HEAD)
worker_worktrees=$(git -C "$worker_project" worktree list --porcelain)

# Same valid metadata must refuse while live and when inventory is unknown.
if "$package/bin/mx" launcher-install --upgrade --package "$package" \
    --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" \
    >"$TMP_ROOT/live-worker-upgrade.out" 2>"$TMP_ROOT/live-worker-upgrade.err"; then
  fail 'package upgrade accepted a live endpoint for valid current metadata'
fi
[ -f "$MX_CMUX_FIXTURE/title" ] || fail 'live endpoint fixture was not present'
assert_grep 'recorded task user' "$TMP_ROOT/live-worker-upgrade.err" \
  'live endpoint refusal did not identify retained runtime use'
: >"$MX_CMUX_FIXTURE/fail-inventory"
if "$package/bin/mx" launcher-install --upgrade --package "$package" \
    --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" \
    >"$TMP_ROOT/unknown-worker-upgrade.out" 2>"$TMP_ROOT/unknown-worker-upgrade.err"; then
  fail 'package upgrade accepted unknown endpoint inventory for valid current metadata'
fi
assert_grep 'cannot verify recorded task endpoint' "$TMP_ROOT/unknown-worker-upgrade.err" \
  'unknown inventory refusal did not report an inconclusive endpoint probe'
rm "$MX_CMUX_FIXTURE/fail-inventory"
# Become live only at the transaction-barrier probe, after both ordinary
# installer preflights already observed authoritative absence.
printf '3' >"$MX_CMUX_FIXTURE/live-at-probe"
rm -f "$MX_CMUX_FIXTURE/probe-count"
before_asset_generation=$(cat "$install/config/package-SHA256SUMS")
if "$package/bin/mx" launcher-install --upgrade --package "$package" \
    --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" \
    >"$TMP_ROOT/race-worker-upgrade.out" 2>"$TMP_ROOT/race-worker-upgrade.err"; then
  fail 'package upgrade did not recheck endpoint presence under its launch barrier'
fi
assert_grep 'recorded task user' "$TMP_ROOT/race-worker-upgrade.err" \
  "transaction-time live endpoint refusal did not identify retained runtime use: $(cat "$TMP_ROOT/race-worker-upgrade.err")"
[ "$(cat "$MX_CMUX_FIXTURE/probe-count")" = 3 ] \
  || fail 'endpoint did not change at the transaction-time quiescence probe'
[ "$(cat "$install/config/package-SHA256SUMS")" = "$before_asset_generation" ] \
  || fail 'transaction-time live endpoint changed installed assets'
rm "$MX_CMUX_FIXTURE/live-at-probe" "$MX_CMUX_FIXTURE/probe-count"
"$cmux" close-workspace

# Canonical variants retain all task bindings while exercising ownership refusals.
cp "$worker_meta" "$TMP_ROOT/worker-meta.canonical"
for refusal in persistent coordinator foreign-owner; do
  if [ "$refusal" = persistent ]; then
    sed -n 's/^canonical_model=//p' "$worker_meta" | jq -c '.persistent=true | .private_home=true' >"$TMP_ROOT/model.json"
  elif [ "$refusal" = foreign-owner ]; then
    sed -n 's/^canonical_model=//p' "$worker_meta" | jq -c '.owner_home="/foreign/owner"' >"$TMP_ROOT/model.json"
  else
    sed -n 's/^canonical_model=//p' "$worker_meta" | jq -c '.artifact="coordination" | .owning_coordinator="coordinator-fixture"' >"$TMP_ROOT/model.json"
  fi
  awk 'BEGIN{done=0} /^canonical_model=/ && !done {next} /^kind=/ {next} {print} END{}' "$TMP_ROOT/worker-meta.canonical" >"$worker_meta"
  printf 'kind=%s\ncanonical_model=%s\n' "$([ "$refusal" = persistent ] && echo daemon || echo delivery)" "$(cat "$TMP_ROOT/model.json")" >>"$worker_meta"
  if "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" \
      >"$TMP_ROOT/$refusal-canonical.out" 2>"$TMP_ROOT/$refusal-canonical.err"; then
    fail "package upgrade accepted canonical $refusal worker metadata"
  fi
done
cp "$TMP_ROOT/worker-meta.canonical" "$worker_meta"

printf 'state sentinel\n' >"$install/data/home/data/private-state"

# Uncertain metadata must fail closed before any installed bytes change.
for refusal in malformed oversized linked; do
  case "$refusal" in
    malformed) printf 'not canonical metadata\n' >"$install/data/home/state/uncertain.meta" ;;
    oversized) head -c 4194305 /dev/zero >"$install/data/home/state/uncertain.meta" ;;
    linked)
      printf 'kind=actor\n' >"$TMP_ROOT/linked.meta"
      ln -s "$TMP_ROOT/linked.meta" "$install/data/home/state/uncertain.meta"
      ;;
  esac
  before=$(cat "$install/config/package-SHA256SUMS")
  if "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" \
      --data-dir "$install/data" >"$TMP_ROOT/$refusal.out" 2>"$TMP_ROOT/$refusal.err"; then
    fail "package upgrade accepted $refusal worker metadata"
  fi
  [ "$(cat "$install/config/package-SHA256SUMS")" = "$before" ] \
    || fail "package upgrade changed the installed generation after $refusal metadata"
  rm "$install/data/home/state/uncertain.meta"
done
pass 'package upgrade refuses malformed, oversized, and linked worker metadata without changing the generation'

for refusal in persistent coordinator; do
  if [ "$refusal" = persistent ]; then
    printf 'kind=actor\npersistent=true\n' >"$install/data/home/state/uncertain.meta"
  else
    printf 'kind=coordinator\n' >"$install/data/home/state/uncertain.meta"
  fi
  if "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" \
      --data-dir "$install/data" >"$TMP_ROOT/$refusal.out" 2>"$TMP_ROOT/$refusal.err"; then
    fail "package upgrade accepted $refusal runtime user"
  fi
  rm "$install/data/home/state/uncertain.meta"
done
pass 'package upgrade refuses persistent, coordinator, and foreign-owner runtime users'

printf 'state sentinel\n' >"$install/data/home/data/private-state"
upgrade_package="$TMP_ROOT/upgrade-package"
cp -R "$package" "$upgrade_package"
printf '\npackage upgrade sentinel\n' >>"$upgrade_package/runtime/docs/README.md"
if command -v shasum >/dev/null 2>&1; then
  upgrade_digest=$(shasum -a 256 "$upgrade_package/runtime/docs/README.md" | awk '{print $1}')
else
  upgrade_digest=$(sha256sum "$upgrade_package/runtime/docs/README.md" | awk '{print $1}')
fi
awk -F '\t' -v OFS='\t' -v digest="$upgrade_digest" '
  $3 == "runtime/docs/README.md" { $1 = digest }
  { print }
' "$upgrade_package/SHA256SUMS" >"$upgrade_package/SHA256SUMS.next"
mv "$upgrade_package/SHA256SUMS.next" "$upgrade_package/SHA256SUMS"
if MX_LAUNCHER_INSTALL_CRASH_AFTER=asset-0000 \
    "$upgrade_package/bin/mx" launcher-install --upgrade --package "$upgrade_package" \
      --bin-dir "$install/bin" --config-dir "$install/config" \
      --data-dir "$install/data" >/dev/null 2>&1; then
  fail 'injected package upgrade crash returned success'
else
  status=$?
fi
expect_code 97 "$status" 'interrupted package upgrade'
if "$install/bin/multplx" paths >"$TMP_ROOT/pending-launch.out" \
    2>"$TMP_ROOT/pending-launch.err"; then
  fail 'launcher entered a runtime with a pending package generation'
fi
assert_grep 'installation transaction is pending' "$TMP_ROOT/pending-launch.err" \
  "launcher did not explain pending package recovery: $(cat "$TMP_ROOT/pending-launch.err")"
if PATH="$fakebin:$runtime_path" MX_REAL_CODEX="$fakebin/codex" \
    MX_LAUNCH_BIN_PATH="$install/bin/multplx" \
    MX_ROOT_OVERRIDE="$install/data/runtime" MX_HOME="$install/data/home" \
    env -u MX_LAUNCH_VALIDATED \
    "$install/data/runtime/bin/mx-launch-harness.sh" codex --version \
    >"$TMP_ROOT/pending-adapter.out" 2>"$TMP_ROOT/pending-adapter.err"; then
  fail 'direct packaged harness adapter entered a pending generation'
fi
assert_grep 'installation transaction is pending' "$TMP_ROOT/pending-adapter.err" \
  'backend launch did not recheck the transaction after startup exclusion'
"$upgrade_package/bin/mx" launcher-install --upgrade --package "$upgrade_package" \
  --bin-dir "$install/bin" --config-dir "$install/config" \
  --data-dir "$install/data" >/dev/null \
  || fail 'package upgrade did not recover its interrupted generation'
cmp -s "$TMP_ROOT/worker-meta.before" "$worker_meta" \
  || fail 'upgrade changed canonical task metadata'
cmp -s "$TMP_ROOT/worker-report.before" "$worker_report" \
  || fail 'upgrade changed the task report'
cmp -s "$TMP_ROOT/worker-receipt.before" "$install/data/home/state/.spawn-actions/upgrade-worker.json" \
  || fail 'upgrade changed the spawn receipt or allocation lease binding'
cmp -s "$TMP_ROOT/worker-lease.before" "$worker_lease_record" \
  || fail 'upgrade changed the allocation lease record bytes'
[ "$(git -C "$worker_project" rev-parse HEAD)" = "$worker_commit" ] \
  || fail 'upgrade changed the worker project commit'
[ "$(git -C "$worker_project" worktree list --porcelain)" = "$worker_worktrees" ] \
  || fail 'upgrade changed the allocated worktree'
[ -d "$worker_worktree" ] && [ -n "$worker_lease" ] \
  || fail 'upgrade lost the retained worktree or allocation lease'
[ "$(cat "$install/config/package-SHA256SUMS")" != "$(cat "$package/SHA256SUMS")" ] \
  || fail 'positive upgrade did not publish a changed installer generation'
[ "$(cat "$install/data/home/data/private-state")" = 'state sentinel' ] \
  || fail 'package upgrade changed operational state'
assert_grep 'package upgrade sentinel' "$install/data/runtime/docs/README.md" \
  'package upgrade did not publish the matching asset generation'
pass 'stopped unfinished worker survives a changed package generation byte-for-byte; interrupted upgrade recovers'

if "$package/bin/mx" launcher-install --uninstall \
    --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" \
    >"$TMP_ROOT/stopped-worker-uninstall.out" 2>"$TMP_ROOT/stopped-worker-uninstall.err"; then
  fail 'uninstall accepted a stopped unfinished worker'
fi
assert_grep 'recorded task user' "$TMP_ROOT/stopped-worker-uninstall.err" \
  'uninstall did not remain strict for a stopped unfinished worker'
git -C "$worker_project" worktree remove --force "$worker_worktree"
git -C "$worker_project" worktree prune
rm -f "$worker_meta" "$worker_report" "$install/data/home/state/.spawn-actions/upgrade-worker.json"

if MX_LAUNCHER_INSTALL_FAIL_AFTER=asset-0000 \
    "$package/bin/mx" launcher-install --uninstall \
      --bin-dir "$install/bin" --config-dir "$install/config" \
      --data-dir "$install/data" >/dev/null 2>&1; then
  fail 'injected package uninstall fault returned success'
fi
[ -x "$install/bin/multplx" ] && [ -f "$install/data/runtime/AGENTS.md" ] \
  || fail 'failed package uninstall did not restore the application generation'
"$package/bin/mx" launcher-install --uninstall \
  --bin-dir "$install/bin" --config-dir "$install/config" \
  --data-dir "$install/data" >/dev/null
[ ! -e "$install/bin/multplx" ] && [ ! -e "$install/data/runtime/AGENTS.md" ] \
  || fail 'package uninstall retained owned application files'
[ "$(cat "$install/data/home/data/private-state")" = 'state sentinel' ] \
  || fail 'package uninstall changed operational state'
[ "$(cat "$project/untracked-sentinel")" = 'borrowed sentinel' ] \
  || fail 'package uninstall changed a borrowed repository'
pass 'package uninstall rolls back faults and preserves state and user repositories'
