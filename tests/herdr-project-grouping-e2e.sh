#!/usr/bin/env bash
# Real Herdr repository grouping, validated nested root context, no-focus and
# exact sibling-safe cleanup. All endpoints are inert, isolated lab fixtures.
set -eu
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
command -v herdr >/dev/null || { echo 'skip: herdr not found'; exit 0; }
command -v jq >/dev/null || { echo 'skip: jq not found'; exit 0; }
. "$ROOT/tests/herdr-test-safety.sh"
TMP_ROOT=$(mktemp -d "$(cd "${TMPDIR:-/tmp}" && pwd -P)/mx-project-e2e.XXXXXX")
SESSION="mx-lab-project-$$"
export HERDR_SESSION="$SESSION"
cleanup() {
  herdr_safe_stop_and_delete "$SESSION"
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT
fail() { printf 'not ok - %s\n' "$*" >&2; exit 1; }
pass() { printf 'ok - %s\n' "$*"; }
mkdir -p "$TMP_ROOT/fakebin"
cat > "$TMP_ROOT/fakebin/codex" <<'PY'
#!/usr/bin/env python3
import signal
print('inert project grouping fixture', flush=True)
signal.pause()
PY
chmod +x "$TMP_ROOT/fakebin/codex"
export PATH="$TMP_ROOT/fakebin:$PATH"
mx_herdr_lab_provision "$SESSION"
lab() { mx_herdr_lab_cli "$SESSION" "$@"; }
field() { sed -n "s/^$2=//p" "$1" | tail -1; }
make_repo() {
  mkdir -p "$1"
  git -C "$1" init -q
  git -C "$1" -c user.name=Tests -c user.email=tests@example.invalid commit --allow-empty -qm initial
}
HOME_DIR="$TMP_ROOT/home"
REPO="$TMP_ROOT/a/project"
OTHER="$TMP_ROOT/b/project"
make_repo "$REPO"
make_repo "$OTHER"
mkdir -p "$HOME_DIR/state" "$HOME_DIR/config" "$HOME_DIR/data"
# The old presence flag must not defeat the new repository default.
touch "$HOME_DIR/config/herdr-presentation-spaces"
CONTROL=$(lab workspace create --cwd "$TMP_ROOT" --label primary --focus)
CONTROL_TAB=$(printf '%s' "$CONTROL" | jq -r .result.tab.tab_id)
FOREIGN=$(lab workspace create --cwd "$REPO" --label project --no-focus)
FOREIGN_PANE=$(printf '%s' "$FOREIGN" | jq -r .result.root_pane.pane_id)
focus() { lab workspace list | jq -r '.result.workspaces[]|select(.focused)|.active_tab_id'; }
spawn() {
  local id=$1 repo=$2
  mkdir -p "$HOME_DIR/data/$id"
  printf 'Inert repository grouping verification.\n' > "$HOME_DIR/data/$id/brief.md"
  MX_HOME="$HOME_DIR" MX_ROOT_OVERRIDE="$ROOT" MX_SPAWN_NO_GUARD=1 \
    "$ROOT/bin/mx-spawn.sh" "$id" "$repo" --harness codex --backend herdr \
    > "$TMP_ROOT/$id.out" 2> "$TMP_ROOT/$id.err" || fail "spawn $id: $(cat "$TMP_ROOT/$id.err")"
}
spawn first "$REPO"
FIRST_META="$HOME_DIR/state/first.meta"
WORKSPACE=$(field "$FIRST_META" herdr_workspace_id)
[ "$WORKSPACE" != "$(printf '%s' "$FOREIGN" | jq -r .result.workspace.workspace_id)" ] || fail 'adopted foreign basename workspace'
[ "$(focus)" = "$CONTROL_TAB" ] || fail 'first spawn moved focus'
[ ! -e "$HOME_DIR/state/first.herdr-presentation" ] || fail 'presence flag created per-task projection'
pass 'repository default overrides old projection flag and preserves foreign workspace and focus'
LINKED="$TMP_ROOT/detached"
git -C "$REPO" worktree add --detach -q "$LINKED" HEAD
spawn linked "$LINKED"
[ "$(field "$HOME_DIR/state/linked.meta" herdr_workspace_id)" = "$WORKSPACE" ] || fail 'linked checkout split repository workspace'
spawn other "$OTHER"
[ "$(field "$HOME_DIR/state/other.meta" herdr_workspace_id)" != "$WORKSPACE" ] || fail 'same basename repositories conflated'
pass 'linked detached worktree shares workspace; distinct same-basename repository stays separate'
mkdir -p "$HOME_DIR/data/standing"
printf '# Charter\nInert persistent researcher on this repository.\n# Routing scope\nRepository grouping verification.\n' > "$HOME_DIR/data/standing/brief.md"
PRIVATE_HOME="$TMP_ROOT/standing-home"
MX_HOME="$HOME_DIR" MX_ROOT_OVERRIDE="$ROOT" "$ROOT/bin/mx-home-seed.sh" standing "$PRIVATE_HOME" "$REPO" \
  > "$TMP_ROOT/seed.out" 2> "$TMP_ROOT/seed.err" || fail "home seed: $(cat "$TMP_ROOT/seed.err")"
MX_HOME="$HOME_DIR" MX_ROOT_OVERRIDE="$ROOT" MX_SPAWN_NO_GUARD=1 \
  "$ROOT/bin/mx-spawn.sh" standing "$PRIVATE_HOME" --persistent --role researcher --output report \
  --harness codex --backend herdr > "$TMP_ROOT/standing.out" 2> "$TMP_ROOT/standing.err" \
  || fail "standing spawn: $(cat "$TMP_ROOT/standing.err")"
STANDING_META="$HOME_DIR/state/standing.meta"
[ "$(field "$STANDING_META" herdr_workspace_id)" = "$WORKSPACE" ] || fail 'standing researcher split workspace'
PRIVATE_HOME=$(field "$STANDING_META" home)
MODEL=$(MX_HOME="$HOME_DIR" MX_ROOT_OVERRIDE="$ROOT" "$ROOT/target/release/mx" task-model inspect standing)
mkdir -p "$PRIVATE_HOME/data/nested"
printf 'Inert nested implementer.\n' > "$PRIVATE_HOME/data/nested/brief.md"
MX_HOME="$PRIVATE_HOME" MX_ROOT_OVERRIDE="$ROOT" MX_STATE_OVERRIDE="$PRIVATE_HOME/state" \
  MX_DATA_OVERRIDE="$PRIVATE_HOME/data" MX_CONFIG_OVERRIDE="$PRIVATE_HOME/config" \
  MX_PROJECTS_OVERRIDE="$PRIVATE_HOME/projects" MX_REPORT_STATE_OVERRIDE="$HOME_DIR/state" \
  MX_TASK_ID=standing MX_ATTEMPT_ID="$(printf '%s' "$MODEL" | jq -r .attempt.id)" \
  MX_ATTEMPT_GENERATION="$(printf '%s' "$MODEL" | jq -r .attempt.generation)" \
  MX_BRIEF_REVISION="$(printf '%s' "$MODEL" | jq -r .accepted_brief_revision)" MX_SPAWN_NO_GUARD=1 \
  "$ROOT/bin/mx-spawn.sh" nested "$REPO" --harness codex --backend herdr \
  > "$TMP_ROOT/nested.out" 2> "$TMP_ROOT/nested.err" || fail "nested spawn: $(cat "$TMP_ROOT/nested.err")"
NESTED_META="$PRIVATE_HOME/state/nested.meta"
[ "$(field "$NESTED_META" herdr_workspace_id)" = "$WORKSPACE" ] || fail 'validated nested worker split workspace'
pass 'standing researcher and validated nested worker retain separate ownership in the same repo workspace'
spawn parallel-a "$REPO" &
PID_A=$!
spawn parallel-b "$REPO" &
PID_B=$!
wait "$PID_A"
wait "$PID_B"
[ "$(field "$HOME_DIR/state/parallel-a.meta" herdr_workspace_id)" = "$WORKSPACE" ] || fail 'parallel-a split workspace'
[ "$(field "$HOME_DIR/state/parallel-b.meta" herdr_workspace_id)" = "$WORKSPACE" ] || fail 'parallel-b split workspace'
[ "$(focus)" = "$CONTROL_TAB" ] || fail 'concurrent spawn moved focus'
pass 'concurrent launches converge without stealing focus'
MX_HOME="$HOME_DIR" MX_ROOT_OVERRIDE="$ROOT" "$ROOT/bin/mx-teardown.sh" first \
  > "$TMP_ROOT/teardown.out" 2> "$TMP_ROOT/teardown.err" || fail "teardown: $(cat "$TMP_ROOT/teardown.err")"
for meta in "$HOME_DIR/state/linked.meta" "$STANDING_META" "$NESTED_META"; do
  lab pane get "$(field "$meta" herdr_pane_id)" | jq -e '.result.pane.pane_id' >/dev/null || fail 'cleanup touched sibling'
done
lab pane get "$FOREIGN_PANE" | jq -e '.result.pane.pane_id' >/dev/null || fail 'cleanup touched foreign pane'
[ "$(focus)" = "$CONTROL_TAB" ] || fail 'cleanup moved focus'
pass 'exact owned cleanup preserves sibling, nested, foreign panes and human focus'
OLD_STANDING_PANE=$(field "$STANDING_META" herdr_pane_id)
OLD_NESTED_PANE=$(field "$NESTED_META" herdr_pane_id)
mx_herdr_lab_stop "$SESSION"
. "$ROOT/bin/mx-backend.sh"
mx_backend_source herdr
mx_backend_herdr_server_ensure "$SESSION"
MX_HOME="$HOME_DIR" MX_ROOT_OVERRIDE="$ROOT" MX_SPAWN_NO_GUARD=1 MX_SPAWN_RECOVERY=1 \
  "$ROOT/bin/mx-spawn.sh" standing "$PRIVATE_HOME" --persistent --role researcher --output report \
  --harness codex --backend herdr > "$TMP_ROOT/restart.out" 2> "$TMP_ROOT/restart.err" \
  || fail "exact grouped restart: $(cat "$TMP_ROOT/restart.err")"
[ "$(field "$STANDING_META" herdr_workspace_id)" = "$WORKSPACE" ] || fail 'restart moved repository workspace'
[ "$(field "$STANDING_META" herdr_pane_id)" != "$OLD_STANDING_PANE" ] || fail 'restart did not replace exact owned shell husk'
lab pane get "$OLD_NESTED_PANE" | jq -e '.result.pane.pane_id' >/dev/null || fail 'restart closed nested sibling husk'
lab pane get "$FOREIGN_PANE" | jq -e '.result.pane.pane_id' >/dev/null || fail 'restart closed foreign pane'
[ "$(focus)" = "$CONTROL_TAB" ] || fail 'restart moved human focus'
pass 'named-server restart reuses exact repo workspace and replaces only its metadata-bound owned husk'
cleanup
trap - EXIT
