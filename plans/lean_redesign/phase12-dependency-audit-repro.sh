#!/usr/bin/env bash
# Reproduce stale dependency completion after a current task reports failure.
# Uses the built release CLI and real owner commands; tmux transport is mocked
# so no model process or provider session is started.
set -euo pipefail

ROOT=$(git -C "$(dirname "${BASH_SOURCE[0]}")/../.." rev-parse --show-toplevel)
MX="$ROOT/target/release/mx"
OUT=${MX_AUDIT_OUT:-$(mktemp -d /private/tmp/mx-audit-dependency-repro.XXXXXX)}
mkdir -p "$OUT/home" "$OUT/project" "$OUT/fakebin"
exec > >(tee "$OUT/run.log") 2>&1

export MX_HOME="$OUT/home"
export MX_ROOT_OVERRIDE="$ROOT"
export MX_RUST_SOURCE_ROOT="$ROOT"
export MX_HEADROOM_CPU_COUNT=64 MX_HEADROOM_LOAD1=0
export MX_HEADROOM_MEM_AVAILABLE_BYTES=68719476736 MX_HEADROOM_IN_USE=0
export MX_HEADROOM_API_CAPACITY=20
mkdir -p "$MX_HOME/config" "$MX_HOME/data" "$MX_HOME/state" "$MX_HOME/projects"

cat > "$OUT/fakebin/tmux" <<'SH'
#!/bin/sh
case "${1:-}" in
  has-session|new-session) exit 0 ;;
  new-window) printf '%s\n' '@audit' ;;
  display-message) printf '%s\n' '%1' ;;
  list-windows) printf '%s\n' 'audit:window' ;;
  *) exit 0 ;;
esac
SH
chmod +x "$OUT/fakebin/tmux"
export PATH="$OUT/fakebin:$PATH"

printf '# temporary audit checkout\n' > "$OUT/project/README.md"
git -C "$OUT/project" init -q -b main
git -C "$OUT/project" -c user.name=Audit -c user.email=audit@example.invalid add README.md
git -C "$OUT/project" -c user.name=Audit -c user.email=audit@example.invalid commit -qm initial

echo "Evidence directory: $OUT"
echo '$ mx project register <temporary-checkout> --alias audit-project'
"$MX" project register "$OUT/project" --alias audit-project > "$OUT/project.json"
project_id=$(jq -r .project_id "$OUT/project.json")
checkout_id=$(jq -r .checkout_id "$OUT/project.json")
starting_revision=$(jq -r .starting_revision "$OUT/project.json")

write_brief() {
  local id=$1 text=$2
  "$MX" brief "$id" audit-project --role implementer >/dev/null
  python3 - "$MX_HOME/data/$id/brief.md" "$text" <<'PY'
import sys
path, text = sys.argv[1:]
with open(path, encoding="utf-8") as source:
    content = source.read()
with open(path, "w", encoding="utf-8") as destination:
    destination.write(content.replace("{TASK}", text))
PY
}

write_brief prerequisite 'Create the prerequisite fixture commit and report the observed result.'
echo '$ mx spawn prerequisite ...'
"$MX" spawn prerequisite audit-project --role implementer --backend tmux \
  --harness codex --request-id prerequisite-spawn
"$MX" task-model inspect prerequisite > "$OUT/prerequisite-before.json"
attempt=$(jq -r .attempt.id "$OUT/prerequisite-before.json")
generation=$(jq -r .attempt.generation "$OUT/prerequisite-before.json")
brief_revision=$(jq -r .accepted_brief_revision "$OUT/prerequisite-before.json")
allocation=$(jq -r .allocation.path "$OUT/prerequisite-before.json")
commit=$(git -C "$allocation" rev-parse HEAD)
observed_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
cat > "$OUT/evidence.json" <<EOF
{"evidence_id":"audit-evidence-1","attempt_id":"$attempt","attempt_generation":$generation,"brief_revision":$brief_revision,"commit":"$commit","checks":[{"name":"audit fixture","outcome":"passed","summary":"passed","artifact":null}],"review":null,"limitations":[],"pr_url":null,"outcome":"evidence-updated","observed_at":"$observed_at","mark_current":true,"expected_current_commit":null}
EOF

echo '$ mx task-model evidence prerequisite ...'
"$MX" task-model evidence prerequisite --request-file "$OUT/evidence.json" > "$OUT/evidence-result.json"
echo '$ mx-report --state done prerequisite ...'
MX_TASK_ID=prerequisite MX_ATTEMPT_ID="$attempt" \
  MX_ATTEMPT_GENERATION="$generation" MX_BRIEF_REVISION="$brief_revision" \
  MX_RUST_BIN="$MX" MX_LAUNCH_BIN_PATH="$MX" MX_MULTICALL_EXPLICIT=1 \
"$ROOT/bin/mx-report" --id prerequisite --state done \
  --message 'current typed completion' --message-id audit-done-1
"$MX" task-model inspect prerequisite > "$OUT/prerequisite-after-done.json"
jq -e '.schedule.state == "completed"' "$OUT/prerequisite-after-done.json" >/dev/null
jq '{attempt, accepted_brief_revision, schedule, delivery}' \
  "$OUT/prerequisite-after-done.json"

echo '$ mx-report --state failed prerequisite ...'
MX_TASK_ID=prerequisite MX_ATTEMPT_ID="$attempt" \
  MX_ATTEMPT_GENERATION="$generation" MX_BRIEF_REVISION="$brief_revision" \
  MX_RUST_BIN="$MX" MX_LAUNCH_BIN_PATH="$MX" MX_MULTICALL_EXPLICIT=1 \
  "$ROOT/bin/mx-report" --id prerequisite --state failed \
  --message 'new failure after prior completion' --message-id audit-failed-1
"$MX" task-model inspect prerequisite > "$OUT/prerequisite-after-failed.json"
echo 'Canonical prerequisite state after accepted failed report:'
jq -e '.schedule.state == "completed"' "$OUT/prerequisite-after-failed.json" >/dev/null
jq '{schedule, delivery}' "$OUT/prerequisite-after-failed.json"

echo '$ mx request submit dependent with --depends prerequisite ...'
"$MX" request submit --batch audit-batch --request dependent-request \
  --task dependent --client audit-client --project "$project_id" \
  --checkout "$checkout_id" --start "$starting_revision" --brief 1 \
  --scope 'dependent task' --depends prerequisite > "$OUT/dependent-request.json"
write_brief dependent 'Create the dependent fixture after its declared prerequisite.'
echo '$ mx spawn dependent ...'
spawn_output=$("$MX" spawn dependent audit-project --role implementer --backend tmux \
  --harness codex --request-id dependent-request)
printf '%s\n' "$spawn_output"
[[ "$spawn_output" == "spawned dependent "* ]] || {
  echo 'expected a spawned dependent result; admission may have deferred' >&2
  exit 1
}
echo 'DEPENDENT_SPAWNED_AFTER_FAILED_REPORT'
echo "Retained evidence: $OUT"
