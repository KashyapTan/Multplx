#!/usr/bin/env bash
# Real Git/CLI crash recovery and scoped cleanup; no external worktree provider.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_test_tmproot_into TMP_ROOT mx-worktree
mx_git_identity allocation allocation@example.invalid
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
export MX_HOME="$TMP_ROOT/home"
mkdir -p "$MX_HOME" "$TMP_ROOT/fakebin"
cat > "$TMP_ROOT/fakebin/treehouse" <<SH
#!/bin/sh
echo invoked > '$TMP_ROOT/provider-invoked'
exit 99
SH
cat > "$TMP_ROOT/fakebin/curl" <<SH
#!/bin/sh
echo invoked > '$TMP_ROOT/download-invoked'
exit 99
SH
chmod +x "$TMP_ROOT/fakebin/treehouse" "$TMP_ROOT/fakebin/curl"
export PATH="$TMP_ROOT/fakebin:$PATH"
project="$TMP_ROOT/project"
mx_git_init_commit "$project"
base=$(git -C "$project" rev-parse HEAD)
acquire() { "$MX_RUST_BIN" worktree acquire "$project" --request "$1" --task "$1" --attempt "attempt-$1" --base "$base"; }
dispose() {
  local operation=$1 record=$2
  shift 2
  "$MX_RUST_BIN" worktree "$operation" "$project" "$(jq -r '.binding.allocation_id' "$record")" --lease "$(jq -r '.binding.lease_id' "$record")" --generation "$(jq -r '.binding.generation' "$record")" --attempt "$(jq -r '.binding.attempt_id' "$record")" "$@"
}
for point in reservation git; do
  rc=0
  MX_WORKTREE_CRASH_AFTER="$point" acquire "crash-$point" > "$TMP_ROOT/crash.out" 2> "$TMP_ROOT/crash.err" || rc=$?
  [ "$rc" -eq 96 ] || fail "acquisition did not crash at $point"
  acquire "crash-$point" > "$TMP_ROOT/$point.json"
  acquire "crash-$point" > "$TMP_ROOT/$point-retry.json"
  cmp "$TMP_ROOT/$point.json" "$TMP_ROOT/$point-retry.json" || fail "retry changed allocation identity"
  path=$(jq -r '.binding.path' "$TMP_ROOT/$point.json")
  [ "$(git -C "$path" rev-parse HEAD)" = "$base" ] || fail "base changed"
  dispose release "$TMP_ROOT/$point.json" >/dev/null
  dispose prune "$TMP_ROOT/$point.json" >/dev/null
  [ -d "$path" ] || fail "preview removed worktree"
  dispose prune "$TMP_ROOT/$point.json" --apply >/dev/null
  [ ! -e "$path" ] || fail "prune did not remove exact path"
done
pass "abrupt CLI exit after reservation and Git acquisition reconciles one allocation"
for point in remove-intent remove-git; do
  acquire "$point" > "$TMP_ROOT/$point.json"
  dispose release "$TMP_ROOT/$point.json" >/dev/null
  rc=0
  MX_WORKTREE_CRASH_AFTER="$point" dispose prune "$TMP_ROOT/$point.json" --apply > "$TMP_ROOT/crash.out" 2> "$TMP_ROOT/crash.err" || rc=$?
  [ "$rc" -eq 96 ] || fail "prune did not crash at $point"
  dispose prune "$TMP_ROOT/$point.json" --apply | jq -e '.state == "removed"' >/dev/null || fail "removal did not reconcile"
done
pass "abrupt CLI exits around Git removal preserve repeat-safe disposition"
# Simultaneous CLI processes, independent of an in-process test mutex.
for i in 1 2 3 4; do acquire concurrent > "$TMP_ROOT/concurrent-$i.json" & done
wait
for i in 2 3 4; do cmp "$TMP_ROOT/concurrent-1.json" "$TMP_ROOT/concurrent-$i.json" || fail "concurrent retry allocated twice"; done
pass "concurrent CLI acquisitions converge across processes"
record="$TMP_ROOT/concurrent-1.json"
path=$(jq -r '.binding.path' "$record")
printf keep > "$path/unfinished"
if dispose release "$record" > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then fail "dirty work released"; fi
[ -f "$path/unfinished" ] || fail "dirty result lost"
if dispose prune "$record" --apply > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then fail "retained work pruned"; fi
# An unrelated Git worktree is visible but cannot be selected by an allocation token.
git -C "$project" worktree add --detach "$TMP_ROOT/foreign" "$base" >/dev/null 2>&1
"$MX_RUST_BIN" worktree list "$project" | jq -e --arg path "$TMP_ROOT/foreign" 'any(.[]; .path == $path and .allocation == null)' >/dev/null
[ -d "$TMP_ROOT/foreign" ] || fail "foreign worktree changed"
if "$MX_RUST_BIN" worktree prune "$project" "$TMP_ROOT/foreign" --apply > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then fail "path-only prune accepted"; fi
[ ! -e "$TMP_ROOT/provider-invoked" ] || fail "Treehouse was invoked"
[ ! -e "$TMP_ROOT/download-invoked" ] || fail "lifecycle attempted a download"
pass "dirty and foreign work remain intact with provider and download sentinels"

# A packaged runtime has assets, not a Git checkout. Deliberately Git-backed
# homes must present a persistent allocation owned by this exact home/task.
package="$TMP_ROOT/package"
mkdir -p "$package"
ln -s "$ROOT/bin" "$package/bin"
ln -s "$ROOT/share" "$package/share"
cp "$ROOT/AGENTS_E.md" "$package/AGENTS.md"
export MX_ROOT_OVERRIDE="$package"
"$MX_RUST_BIN" worktree acquire "$project" --request git-home --task git-home --attempt git-home-attempt --base "$base" --persistent > "$TMP_ROOT/git-home.json"
git_home=$(jq -r '.binding.path' "$TMP_ROOT/git-home.json")
git_allocation=$(jq -r '.binding.allocation_id' "$TMP_ROOT/git-home.json")
MX_DAEMON_CHARTER='Maintain this bounded project' "$MX_RUST_BIN" home-seed git-home "$git_home" --no-projects --git-allocation "$project" "$git_allocation" > "$TMP_ROOT/home-seed.out"
jq -e --arg id "$git_allocation" '.state == "active" and .directory_identity != null and .git_allocation.binding.allocation_id == $id and .git_allocation.binding.persistent' "$MX_HOME/data/.home-allocation-git-home.json" >/dev/null || fail 'Git home did not bind its persistent allocation'
[ "$(git -C "$git_home" rev-parse HEAD)" = "$base" ] || fail 'home seeding changed exact Git base'
[ "$(git -C "$project" status --porcelain)" = '' ] || fail 'seeding changed borrowed project'
MX_DAEMON_CHARTER='Maintain this bounded project' "$MX_RUST_BIN" home-seed git-home "$git_home" --no-projects --git-allocation "$project" "$git_allocation" >/dev/null || fail 'same Git home did not reconcile'
if MX_DAEMON_CHARTER='Foreign task' "$MX_RUST_BIN" home-seed foreign-home "$git_home" --no-projects --git-allocation "$project" "$git_allocation" > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then fail 'foreign task adopted persistent allocation'; fi
if dispose release "$TMP_ROOT/git-home.json" > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then fail 'idle persistent home released'; fi
[ -f "$git_home/data/charter.md" ] || fail 'persistent charter lost'
[ ! -e "$package/.git" ] || fail 'packaged runtime was turned into a repository'
[ ! -e "$TMP_ROOT/provider-invoked" ] || fail 'home seed invoked old provider'
[ ! -e "$TMP_ROOT/download-invoked" ] || fail 'home seed downloaded a provider'
pass 'deliberate Git homes bind exact persistent leases using non-Git runtime assets'
