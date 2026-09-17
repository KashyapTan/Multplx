#!/usr/bin/env bash
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mx_test_tmproot_into TMP_ROOT mx-state-migration
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
home="$TMP_ROOT/home"
mkdir -p "$home/state/pending-replies" "$home/state/.workflow" "$home/data" "$home/config" "$home/projects"
export MX_HOME="$home"

printf 'codex high\n' > "$home/config/actor-harness"
printf 'kind=delivery\nbackend=tmux\nwindow=mx-legacy\n' > "$home/state/legacy.meta"
printf 'wake-bytes\n' > "$home/state/.wake-queue"
printf '{"correlation":"keep"}\n' > "$home/state/pending-replies/keep.json"
printf 'immutable workflow\n' > "$home/state/.workflow/keep"
before_queue=$(mx_test_sha256 "$home/state/.wake-queue")
before_reply=$(mx_test_sha256 "$home/state/pending-replies/keep.json")
before_workflow=$(mx_test_sha256 "$home/state/.workflow/keep")

before=$(mx_test_filesystem_manifest "$home")
"$MX_RUST_BIN" migrate inspect --home "$home" --operation fixture > "$TMP_ROOT/inspect.json"
[ "$before" = "$(mx_test_filesystem_manifest "$home")" ] || fail 'migration inspect mutated the home'
jq -e '.state == "legacy" and .legacy_tasks == 1 and (.actions | length) >= 2' "$TMP_ROOT/inspect.json" >/dev/null || fail 'inspect omitted legacy conversion actions'
pass 'migration inspect is read-only and reports owned conversions'

"$MX_RUST_BIN" migrate apply --home "$home" --operation fixture > "$TMP_ROOT/apply.json"
jq -e '.state == "current" and .backup_manifest != null' "$TMP_ROOT/apply.json" >/dev/null || fail 'apply did not publish current marker and backup'
[ "$(cat "$home/config/subagent-harness")" = 'codex high' ] || fail 'canonical harness lost model and effort'
grep -q '^canonical_model=' "$home/state/legacy.meta" || fail 'legacy task did not gain canonical compatibility model'
sed -n 's/^canonical_model=//p' "$home/state/legacy.meta" | jq -e '.legacy_unknown and .attempt == null and .accepted_brief_revision == null' >/dev/null || fail 'migration fabricated attempt or brief identity'
[ "$before_queue" = "$(mx_test_sha256 "$home/state/.wake-queue")" ] || fail 'wake queue bytes changed'
[ "$before_reply" = "$(mx_test_sha256 "$home/state/pending-replies/keep.json")" ] || fail 'pending reply bytes changed'
[ "$before_workflow" = "$(mx_test_sha256 "$home/state/.workflow/keep")" ] || fail 'workflow snapshot bytes changed'
after=$(mx_test_filesystem_manifest "$home")
"$MX_RUST_BIN" migrate apply --home "$home" --operation fixture > "$TMP_ROOT/retry.json"
[ "$after" = "$(mx_test_filesystem_manifest "$home")" ] || fail 'repeat apply changed current state'
pass 'apply is repeat-safe and preserves queue, reply, workflow and unknown identity evidence'

"$MX_RUST_BIN" migrate summary --home "$home" --limit 20 > "$TMP_ROOT/summary.json"
jq -e '.available and .tasks[0].task_id == "legacy" and .tasks[0].legacy_unknown and .omitted == 0' "$TMP_ROOT/summary.json" >/dev/null || fail 'compact restart summary lost legacy uncertainty'
printf 'not metadata\n' > "$home/state/corrupt.meta"
"$MX_RUST_BIN" migrate summary --home "$home" --limit 20 > "$TMP_ROOT/unavailable-summary.json"
jq -e '.available == false and (.reason | contains("corrupt")) and (.tasks | length) == 0' "$TMP_ROOT/unavailable-summary.json" >/dev/null || fail 'corrupt task produced a partial restart summary'
rm "$home/state/corrupt.meta"
pass 'restart summary reports bounded canonical identities without transcript replay'

"$MX_RUST_BIN" migrate rollback --home "$home" --operation fixture > "$TMP_ROOT/rollback.json"
[ "$(cat "$home/state/legacy.meta")" = $'kind=delivery\nbackend=tmux\nwindow=mx-legacy' ] || fail 'rollback did not restore exact legacy metadata'
[ ! -e "$home/config/subagent-harness" ] || fail 'rollback retained a migration-created canonical alias'
[ ! -e "$home/state/.home-schema-version" ] || fail 'rollback retained the new home marker'
[ -f "$home/state/.migration-backups/fixture/rollback.json" ] || fail 'rollback evidence was not retained'
pass 'matching rollback restores converted files while retaining evidence'

for point in before-intent after-intent after-write after-progress after-commit; do
  operation="fault-${point}"
  if MX_MIGRATION_FAULT="$point" "$MX_RUST_BIN" migrate apply --home "$home" --operation "$operation" > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then
    fail "migration fault $point unexpectedly succeeded"
  fi
  "$MX_RUST_BIN" migrate apply --home "$home" --operation "$operation" > "$TMP_ROOT/recovered.json"
  jq -e '.state == "current"' "$TMP_ROOT/recovered.json" >/dev/null || fail "migration fault $point did not recover"
  [ "$before_queue" = "$(mx_test_sha256 "$home/state/.wake-queue")" ] || fail "migration fault $point lost a queue record"
  "$MX_RUST_BIN" migrate rollback --home "$home" --operation "$operation" >/dev/null
done
pass 'every home transition boundary resumes without losing durable queue state'

printf 'claude\n' > "$home/config/subagent-harness"
if "$MX_RUST_BIN" migrate apply --home "$home" --operation conflict > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then
  fail 'conflicting canonical and legacy aliases were accepted'
fi
grep -q 'conflicting config/actor-harness' "$TMP_ROOT/err" || fail 'alias conflict diagnostic was not specific'
[ ! -e "$home/state/.home-schema-version" ] || fail 'blocked migration partially published its marker'
rm "$home/config/subagent-harness"
pass 'conflicting aliases block without partial publication'

"$MX_RUST_BIN" migrate inspect --home "$home" --operation missing-coordinator --coordinator missing > "$TMP_ROOT/missing-coordinator.json"
jq -e '.state == "blocked" and (.blockers | any(contains("requested coordinator task missing")))' "$TMP_ROOT/missing-coordinator.json" >/dev/null || fail 'unproven coordinator mapping was not blocked'
pass 'coordinator mapping requires an exact legacy persistent task'

ln -s "$TMP_ROOT/foreign-lock" "$home/state/.lock"
if "$MX_RUST_BIN" migrate apply --home "$home" --operation unsafe-lock > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then
  fail 'migration accepted a symlinked session lock'
fi
grep -q 'unexpected symlink' "$TMP_ROOT/err" || fail 'unsafe session-lock diagnostic was not specific'
rm "$home/state/.lock"
pass 'unsafe session lock is rejected before migration publication'

# Build a current home and an exact legacy worktree for the explicit A10 transfer.
repo="$TMP_ROOT/repo"
mx_git_init_commit "$repo"
git -C "$repo" worktree add --detach "$TMP_ROOT/legacy-worktree" HEAD >/dev/null
git -C "$repo" worktree add --detach "$TMP_ROOT/persistent-worktree" HEAD >/dev/null
"$MX_RUST_BIN" migrate apply --home "$home" --operation transfer-home >/dev/null
project=$("$MX_RUST_BIN" project register "$repo" --alias transfer)
printf 'kind=delivery\nbackend=tmux\nworktree=%s\n' "$TMP_ROOT/legacy-worktree" > "$home/state/transfer.meta"
model=$("$MX_RUST_BIN" task-model inspect transfer | jq -c --arg home "$home" --argjson project "$project" '
  .legacy_unknown=false |
  .owner_home=$home | .owner_state=($home+"/state") |
  .parent_home=$home | .parent_state=($home+"/state") |
  .parent_id="orchestrator" | .root_id="orchestrator" |
  .project=$project |
  .attempt={id:"attempt-transfer",generation:1,brief_revision:1} |
  .accepted_brief_revision=1 |
  .briefs=[{revision:1,scope:"preserve legacy work",acceptance_criteria:[],source_artifacts:[],reason:"accepted legacy assignment"}] |
  .assignments=[{generation:1,role:.role,brief_revision:1,reason:"accepted legacy assignment"}]')
printf 'kind=delivery\nbackend=tmux\nworktree=%s\nschema_version=2\ncanonical_model=%s\n' "$TMP_ROOT/legacy-worktree" "$model" > "$home/state/transfer.meta"
cat > "$TMP_ROOT/treehouse.json" <<JSON
{"worktrees":[
  {"path":"$TMP_ROOT/legacy-worktree","leased":true,"lease_holder":"transfer"},
  {"path":"$TMP_ROOT/persistent-worktree","leased":true,"lease_holder":"persistent"},
  {"path":"$TMP_ROOT/foreign-worktree","leased":true,"lease_holder":"foreign"}
]}
JSON
git -C "$repo" worktree add --detach "$TMP_ROOT/foreign-worktree" HEAD >/dev/null
metadata_before=$(mx_test_sha256 "$TMP_ROOT/treehouse.json")

(cd "$TMP_ROOT/legacy-worktree" && exec sleep 30) &
occupant=$!
sleep 0.2
if "$MX_RUST_BIN" migrate relocate-worktree --home "$home" --metadata "$TMP_ROOT/treehouse.json" --path "$TMP_ROOT/legacy-worktree" --project transfer --task transfer --request transfer-1 > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"; then
  kill "$occupant" 2>/dev/null || true
  fail 'occupied legacy worktree was moved'
fi
grep -q 'occupants' "$TMP_ROOT/err" || fail 'occupied worktree refusal was not specific'
kill "$occupant"
wait "$occupant" 2>/dev/null || true

set +e
MX_WORKTREE_CRASH_AFTER=legacy-git "$MX_RUST_BIN" migrate relocate-worktree --home "$home" --metadata "$TMP_ROOT/treehouse.json" --path "$TMP_ROOT/legacy-worktree" --project transfer --task transfer --request transfer-1 > "$TMP_ROOT/out" 2> "$TMP_ROOT/err"
crash_status=$?
set -e
[ "$crash_status" -eq 96 ] || fail 'legacy Git interruption point did not stop at the expected boundary'
[ ! -e "$TMP_ROOT/legacy-worktree" ] || fail 'interrupted Git transfer did not reach the durable destination'
"$MX_RUST_BIN" migrate relocate-worktree --home "$home" --metadata "$TMP_ROOT/treehouse.json" --path "$TMP_ROOT/legacy-worktree" --project transfer --task transfer --request transfer-1 > "$TMP_ROOT/relocation.json"
new_path=$(jq -r '.new_path' "$TMP_ROOT/relocation.json")
[ ! -e "$TMP_ROOT/legacy-worktree" ] || fail 'legacy worktree remained at the externally reusable path'
[ -d "$new_path" ] || fail 'relocated worktree destination is missing'
[ -d "$TMP_ROOT/foreign-worktree" ] || fail 'unrelated external pool entry changed'
[ "$metadata_before" = "$(mx_test_sha256 "$TMP_ROOT/treehouse.json")" ] || fail 'external pool metadata was mutated'
sed -n 's/^canonical_model=//p' "$home/state/transfer.meta" | jq -e --arg path "$new_path" '.allocation.path == $path and .allocation.lease_id != "" and .attempt.id == "attempt-transfer"' >/dev/null || fail 'task references did not bind the new internal lease'
first_mapping=$(mx_test_sha256 "$home/state/.legacy-worktree-mappings/transfer-1.json")
"$MX_RUST_BIN" migrate relocate-worktree --home "$home" --metadata "$TMP_ROOT/treehouse.json" --path "$TMP_ROOT/legacy-worktree" --project transfer --task transfer --request transfer-1 > "$TMP_ROOT/relocation-retry.json"
[ "$first_mapping" = "$(mx_test_sha256 "$home/state/.legacy-worktree-mappings/transfer-1.json")" ] || fail 'relocation retry changed its mapping'
[ "$(jq -r '.new_path' "$TMP_ROOT/relocation-retry.json")" = "$new_path" ] || fail 'relocation retry changed destination'
pass 'legacy worktree transfer fences occupants, resumes after interruption and leaves foreign pool state untouched'

printf 'kind=daemon\nbackend=tmux\nworktree=%s\nhome=%s\n' "$TMP_ROOT/persistent-worktree" "$TMP_ROOT/persistent-worktree" > "$home/state/persistent.meta"
persistent_model=$("$MX_RUST_BIN" task-model inspect persistent | jq -c --arg home "$home" --arg path "$TMP_ROOT/persistent-worktree" --argjson project "$project" '
  .legacy_unknown=false |
  .persistent=true | .private_home=true | .persistent_home=$path | .home_allocation=null |
  .owner_home=$home | .owner_state=($home+"/state") |
  .parent_home=$home | .parent_state=($home+"/state") |
  .parent_id="orchestrator" | .root_id="orchestrator" |
  .project=$project |
  .attempt={id:"attempt-persistent",generation:1,brief_revision:1} |
  .accepted_brief_revision=1 |
  .briefs=[{revision:1,scope:"preserve persistent legacy home",acceptance_criteria:[],source_artifacts:[],reason:"accepted legacy assignment"}] |
  .assignments=[{generation:1,role:.role,brief_revision:1,reason:"accepted legacy assignment"}]')
printf 'kind=daemon\nbackend=tmux\nworktree=%s\nhome=%s\nschema_version=2\ncanonical_model=%s\n' "$TMP_ROOT/persistent-worktree" "$TMP_ROOT/persistent-worktree" "$persistent_model" > "$home/state/persistent.meta"
"$MX_RUST_BIN" migrate relocate-worktree --home "$home" --metadata "$TMP_ROOT/treehouse.json" --path "$TMP_ROOT/persistent-worktree" --project transfer --task persistent --request persistent-1 > "$TMP_ROOT/persistent-relocation.json"
persistent_path=$(jq -r '.new_path' "$TMP_ROOT/persistent-relocation.json")
jq -e --arg path "$persistent_path" '.state == "active" and .binding.path == $path and .binding.lease_id != "" and .git_allocation.binding.path == $path' "$home/data/.home-allocation-persistent.json" >/dev/null || fail 'persistent legacy home did not gain a canonical home receipt'
sed -n 's/^canonical_model=//p' "$home/state/persistent.meta" | jq -e --arg path "$persistent_path" '.persistent and .persistent_home == $path and .allocation.path == $path and .home_allocation.path == $path' >/dev/null || fail 'persistent task references were not published atomically'
[ "$metadata_before" = "$(mx_test_sha256 "$TMP_ROOT/treehouse.json")" ] || fail 'persistent transfer mutated external pool metadata'
pass 'persistent legacy worktree gains matching worktree and home lease receipts'
