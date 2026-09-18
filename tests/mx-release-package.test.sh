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
printf 'packaged-codex 1.0\n'
SH
chmod +x "$fakebin/codex"
runtime_path="/usr/bin:/bin"
case "$(uname -s)" in
  Darwin) runtime_path="/usr/bin:/bin:/usr/sbin:/sbin" ;;
esac
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
  MX_ROOT_OVERRIDE="$install/data/runtime" MX_HOME="$install/data/home" \
  env -u MX_LAUNCH_VALIDATED \
  "$install/data/runtime/bin/mx-launch-harness.sh" codex --version \
  >"$TMP_ROOT/package-harness-version.txt" \
  || fail 'packaged harness adapter rejected its non-Git runtime'
assert_grep 'packaged-codex 1.0' "$TMP_ROOT/package-harness-version.txt" \
  'packaged harness adapter did not execute the detected harness'

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
  'launcher did not explain pending package recovery'
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
[ "$(cat "$install/data/home/data/private-state")" = 'state sentinel' ] \
  || fail 'package upgrade changed operational state'
assert_grep 'package upgrade sentinel' "$install/data/runtime/docs/README.md" \
  'package upgrade did not publish the matching asset generation'
pass 'interrupted package upgrade recovers one matching binary and asset generation'

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
