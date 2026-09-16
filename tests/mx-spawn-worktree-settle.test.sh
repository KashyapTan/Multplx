#!/usr/bin/env bash
# Real Git allocation precedes mocked endpoint creation. Pane cwd is never allocation authority.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SPAWN="$ROOT/bin/mx-spawn.sh"
TMP_ROOT=$(mx_test_tmproot mx-spawn-worktree-settle)

# make_settle_fakebin <dir> builds a fake tmux whose `#{pane_current_path}`
# query returns MX_FAKE_PANE_STALE for the first MX_FAKE_PANE_STALE_READS
# calls, then MX_FAKE_PANE_PATH forever after - reproducing a pane that
# transiently reports a stale cwd before settling into the real worktree.
make_settle_fakebin() {
  local dir=$1 fakebin
  fakebin=$(mx_fakebin "$dir")
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "$*" in
  *"#{pane_current_path}"*)
    countfile="${MX_FAKE_PANE_COUNTFILE:?MX_FAKE_PANE_COUNTFILE unset}"
    n=0
    [ -f "$countfile" ] && n=$(cat "$countfile")
    n=$((n + 1))
    printf '%s\n' "$n" > "$countfile"
    if [ "$n" -le "${MX_FAKE_PANE_STALE_READS:-0}" ]; then
      printf '%s\n' "${MX_FAKE_PANE_STALE:-}"
    else
      printf '%s\n' "${MX_FAKE_PANE_PATH:-}"
    fi
    exit 0
    ;;
esac
case "${1:-}" in
  display-message) printf 'broker\n'; exit 0 ;;
  list-windows) exit 0 ;;
  new-window)
    printf '%s\n' "$*" >> "${MX_FAKE_SEND_LOG:?}"
    while [ $# -gt 0 ]; do
      if [ "$1" = -c ]; then shift; cwd=$1; break; fi
      shift
    done
    [ -n "${cwd:-}" ] && git -C "$cwd" rev-parse --git-common-dir >/dev/null || exit 91
    printf '@1\n'; exit 0 ;;
  has-session|new-session|kill-window) exit 0 ;;
  send-keys)
    if [ -n "${MX_FAKE_SEND_LOG:-}" ]; then printf '%s\n' "$*" >> "$MX_FAKE_SEND_LOG"; fi
    exit 0 ;;
esac
exit 0
SH
  local real_git
  real_git=$(command -v git)
  printf '#!/usr/bin/env bash\nreal=%q\n' "$real_git" > "$fakebin/git"
  cat >> "$fakebin/git" <<'SH'
if [[ "$*" == *"worktree add --detach --"* ]] && [ -n "${MX_TEST_ALLOCATION_TAMPER:-}" ]; then
  "$real" "$@" || exit $?
  args=("$@")
  cwd=${args[${#args[@]}-2]}
  case "$MX_TEST_ALLOCATION_TAMPER" in
    base) "$real" -C "$cwd" -c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty -qm tampered ;;
    repository) mv "$cwd" "$cwd-retained"; "$real" init -q "$cwd"; "$real" -C "$cwd" -c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty -qm wrong ;;
  esac
  exit 0
fi
exec "$real" "$@"
SH
  chmod +x "$fakebin/git"
  chmod +x "$fakebin/tmux"
  printf '#!/bin/sh\nexit 93\n' > "$fakebin/treehouse"
  chmod +x "$fakebin/treehouse"
  printf '%s\n' "$fakebin"
}

# make_settle_case <name> <id> <stale_reads> builds a home, a primary project
# with a real worktree (the eventual settled path), and a separate real git
# repo standing in for the stale path (a real checkout of something else
# entirely, distinct from both the project and the worktree - mirroring the
# live incident where the stale read was another real Multplx home).
make_settle_case() {
  local name=$1 id=$2 stale_reads=$3 case_dir home proj wt stale fakebin countfile
  case_dir="$TMP_ROOT/$name"
  home="$case_dir/home"
  proj="$case_dir/project"
  wt="$case_dir/wt"
  stale="$case_dir/stale-other-checkout"
  countfile="$case_dir/pane-call-count"
  fakebin=$(make_settle_fakebin "$case_dir/fake")
  mkdir -p "$home/data" "$home/projects" "$home/state" "$home/config"
  printf 'codex\n' > "$home/config/actor-harness"
  mx_git_worktree "$proj" "$wt" "wt-$name"
  mx_git_init_commit "$stale"
  mkdir -p "$home/data/$id"
  printf 'brief for %s\n' "$id" > "$home/data/$id/brief.md"
  touch "$home/state/.last-watcher-beat"
  printf '%s\n' "$case_dir|$home|$proj|$wt|$stale|$fakebin|$countfile|$stale_reads"
}

read_settle_record() {
  IFS='|' read -r _ HOME_DIR PROJ_DIR WT_DIR STALE_DIR FAKEBIN_DIR COUNTFILE STALE_READS <<EOF
$1
EOF
}

run_settle_spawn() {
  local id=$1
  shift
  MX_ROOT_OVERRIDE='' MX_HOME="$HOME_DIR" \
    MX_STATE_OVERRIDE="$HOME_DIR/state" MX_DATA_OVERRIDE="$HOME_DIR/data" \
    MX_PROJECTS_OVERRIDE="$HOME_DIR/projects" MX_CONFIG_OVERRIDE="$HOME_DIR/config" \
    MX_SPAWN_NO_GUARD=1 TMUX="fake,1,0" \
    MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 \
    MX_HEADROOM_IN_USE=0 MX_HEADROOM_API_CAPACITY="${MX_HEADROOM_API_CAPACITY:-4}" \
    MX_FAKE_SEND_LOG="$HOME_DIR/send.log" \
    MX_FAKE_PANE_PATH="$WT_DIR" MX_FAKE_PANE_STALE="$STALE_DIR" \
    MX_FAKE_PANE_STALE_READS="$STALE_READS" MX_FAKE_PANE_COUNTFILE="$COUNTFILE" \
    PATH="$FAKEBIN_DIR:$PATH" \
    "$SPAWN" "$id" "$PROJ_DIR" "$@" 2>&1
}

grant_binding() (
  local state=$1 bindings=$2 request boundary task project operation target digest consequence
  export MX_STATE_OVERRIDE=$state
  # shellcheck source=bin/mx-maintainer-override-lib.sh
  . "$ROOT/bin/mx-maintainer-override-lib.sh"
  mx_override_require_primary_lock() { return 0; }
  boundary=$(printf '%s' "$bindings" | jq -r '.boundary')
  task=$(printf '%s' "$bindings" | jq -r '.task')
  project=$(printf '%s' "$bindings" | jq -r '.project')
  operation=$(printf '%s' "$bindings" | jq -r '.operation')
  target=$(printf '%s' "$bindings" | jq -r '.target')
  digest=$(printf '%s' "$bindings" | jq -r '.expected_state_digest')
  consequence=$(printf '%s' "$bindings" | jq -r '.consequence')
  request=$(mx_override_request "$boundary" "$task" "$project" "$operation" "$target" "$digest" "$consequence") || exit 1
  mx_override_grant "$request" "Grant $boundary for $operation on $target only." >/dev/null || exit 1
  printf '%s\n' "$request"
)

# A stale pane observation must never select an allocation.
test_stale_pane_never_selects_allocation() {
  local rec id out status
  id=settle-single-stale-z1
  rec=$(make_settle_case settle-single "$id" 1)
  read_settle_record "$rec"

  out=$(run_settle_spawn "$id")
  status=$?
  expect_code 0 "$status" "spawn should succeed once the pane settles"
  assert_contains "$out" "spawned $id" "spawn did not report success"
  WT_DIR=$(sed -n 's/^worktree=//p' "$HOME_DIR/state/$id.meta")
  [ "$(git -C "$WT_DIR" rev-parse HEAD)" = "$(git -C "$PROJ_DIR" rev-parse HEAD)" ] || fail 'allocation did not use accepted base'
  assert_grep "-c $WT_DIR" "$HOME_DIR/send.log" 'endpoint did not receive pre-created allocation cwd'
  assert_absent "$COUNTFILE" 'spawn still uses pane cwd as allocation authority'
  assert_no_grep "worktree=$STALE_DIR" "$HOME_DIR/state/$id.meta" \
    "meta wrongly recorded the transient stale path as the worktree"
  pass "stale pane cwd cannot override exact owner allocation"
}

# Exact allocation is independent of pane settlement.
test_allocation_needs_no_pane_poll() {
  local rec id out status start end elapsed
  id=settle-already-settled-z2
  rec=$(make_settle_case settle-already-settled "$id" 0)
  read_settle_record "$rec"

  start=$(date +%s)
  out=$(run_settle_spawn "$id")
  status=$?
  end=$(date +%s)
  elapsed=$((end - start))
  expect_code 0 "$status" "spawn should succeed when the pane is already settled"
  WT_DIR=$(sed -n 's/^worktree=//p' "$HOME_DIR/state/$id.meta")
  assert_grep "-c $WT_DIR" "$HOME_DIR/send.log" 'endpoint cwd differs from recorded allocation'
  assert_absent "$COUNTFILE" 'spawn unexpectedly polled pane cwd'
  [ "$elapsed" -le 5 ] || fail "already-settled pane took ${elapsed}s to confirm - expected close to the single inter-poll sleep"
  pass "allocation uses exact endpoint cwd without pane polling"
}

test_exact_single_checkout_mode_serializes_and_releases() {
  local rec id bindings request out status record cleanup cleanup_request proj_real
  id=single-checkout-z3
  rec=$(make_settle_case single-checkout "$id" 0)
  read_settle_record "$rec"
  proj_real=$(cd "$PROJ_DIR" && pwd -P)
  bindings=$(MX_HOME="$HOME_DIR" MX_STATE_OVERRIDE="$HOME_DIR/state" \
    "$ROOT/bin/mx-override-bindings.sh" single-checkout "$id" "$PROJ_DIR") || fail "single-checkout binding failed"
  request=$(grant_binding "$HOME_DIR/state" "$bindings") || fail "single-checkout grant failed"

  out=$(run_settle_spawn "$id" --single-checkout "$request")
  status=$?
  expect_code 0 "$status" "exact single-checkout spawn"
  assert_contains "$out" "spawned $id" "single-checkout spawn did not report success"
  assert_grep 'single_checkout=yes' "$HOME_DIR/state/$id.meta" "single-checkout fact was not recorded"
  assert_grep "worktree=$proj_real" "$HOME_DIR/state/$id.meta" "single-checkout did not bind the primary project"
  record=$(sed -n 's/^single_checkout_record=//p' "$HOME_DIR/state/$id.meta")
  [ -f "$record" ] || fail "single-checkout reservation was not retained"
  [ "$(jq -r '.outcome' "$HOME_DIR/state/maintainer-overrides/consumed/$request.json")" = succeeded ] \
    || fail "single-checkout outcome was not recorded truthfully"

  cleanup=$(MX_HOME="$HOME_DIR" MX_STATE_OVERRIDE="$HOME_DIR/state" \
    "$ROOT/bin/mx-override-bindings.sh" cleanup "$id") || fail "single-checkout cleanup binding failed"
  cleanup_request=$(grant_binding "$HOME_DIR/state" "$cleanup") || fail "single-checkout cleanup grant failed"
  MX_HOME="$HOME_DIR" MX_STATE_OVERRIDE="$HOME_DIR/state" MX_DATA_OVERRIDE="$HOME_DIR/data" \
    MX_CONFIG_OVERRIDE="$HOME_DIR/config" PATH="$FAKEBIN_DIR:$PATH" \
    "$ROOT/bin/mx-teardown.sh" "$id" --override "$cleanup_request" >/dev/null \
    || fail "single-checkout teardown failed"
  [ ! -e "$record" ] || fail "single-checkout reservation survived teardown"
  [ -d "$PROJ_DIR/.git" ] || fail "single-checkout teardown removed the project checkout"
  pass "exact single-checkout mode records lost isolation, serializes, and releases at teardown"
}

test_stale_pane_never_selects_allocation
test_allocation_needs_no_pane_poll
test_exact_single_checkout_mode_serializes_and_releases

echo "# all mx-spawn-worktree-settle tests passed"

test_registry_maps_to_canonical_publication_without_implicit_review() {
  local mode yolo id record output expected_mode expected_destination
  for mode in deep-review direct-PR local-only; do
    for yolo in off on; do
      id="mode-${mode//[^a-zA-Z]/}-${yolo}"
      record=$(make_settle_case "$id" "$id" 0)
      read_settle_record "$record"
      if [ "$mode" = local-only ]; then
        expected_mode=local-only
        expected_destination=local
      else
        # A PR destination has a real local remote. A legacy deep-review label
        # does not select the optional review tool for a new canonical task.
        git clone --quiet --bare "$PROJ_DIR" "$HOME_DIR/origin.git" || fail 'fixture remote clone failed'
        git -C "$PROJ_DIR" remote add origin "$HOME_DIR/origin.git" || fail 'fixture origin registration failed'
        expected_mode=direct-PR
        expected_destination=pull-request
      fi
      if [ "$mode" = local-only ]; then
        # Exercise equivalent path spellings on every platform, including macOS /var aliases.
        ln -s "$WT_DIR" "$WT_DIR-alias"
        WT_DIR="$WT_DIR-alias"
      fi
      ln -s "$PROJ_DIR" "$HOME_DIR/projects/project"
      if [ "$yolo" = on ]; then printf '%s\n' "- project [$mode +yolo] - fixture" > "$HOME_DIR/data/projects.md"; else printf '%s\n' "- project [$mode] - fixture" > "$HOME_DIR/data/projects.md"; fi
      output=$(run_settle_spawn "$id") || fail "mode launch failed: $output"
      WT_DIR=$(sed -n 's/^worktree=//p' "$HOME_DIR/state/$id.meta")
      assert_contains "$output" "mode=$expected_mode yolo=off" 'spawn canonical publication or inert yolo changed'
      assert_grep "mode=$expected_mode" "$HOME_DIR/state/$id.meta" 'metadata publication destination changed'
      assert_grep 'yolo=off' "$HOME_DIR/state/$id.meta" 'legacy yolo granted active authority'
      [ "$(jq -r '.projects[0].publication' "$HOME_DIR/data/projects.json")" = "$expected_destination" ] || fail 'canonical destination disagrees with selected source'
      [ "$(jq -r '.projects[0].review' "$HOME_DIR/data/projects.json")" = null ] || fail 'legacy registry implicitly selected deep review'
      [ "$(jq -r '.projects[0].checkouts[0].ownership' "$HOME_DIR/data/projects.json")" = user-owned ] || fail 'symlinked legacy project was silently claimed as managed'
      if [ "$mode" = local-only ]; then
        before_head=$(git -C "$PROJ_DIR" rev-parse HEAD)
        printf 'borrowed source sentinel\n' > "$PROJ_DIR/borrowed-sentinel"
        before_status=$(git -C "$PROJ_DIR" status --porcelain)
        if MX_HOME="$HOME_DIR" MX_STATE_OVERRIDE="$HOME_DIR/state" "$ROOT/bin/mx-merge-local.sh" "$id" > "$HOME_DIR/merge.out" 2>&1; then
          fail 'local landing mutated a user-owned checkout'
        fi
        assert_grep 'user-owned' "$HOME_DIR/merge.out" 'user-owned local outcome refusal missing'
        [ "$(git -C "$PROJ_DIR" rev-parse HEAD)" = "$before_head" ] || fail 'user-owned source branch moved'
        [ "$(git -C "$PROJ_DIR" status --porcelain)" = "$before_status" ] || fail 'user-owned source index or files changed'
      fi
    done
  done
  pass 'canonical publication matches remote availability, legacy review/yolo remain inert, and local landing retains safety checks'
}
test_registry_maps_to_canonical_publication_without_implicit_review


assert_rejected_before_harness() {
  local id=$1
  if [ -f "$HOME_DIR/send.log" ]; then assert_no_grep 'new-window' "$HOME_DIR/send.log" 'invalid allocation reached endpoint creation'; fi
  touch "$HOME_DIR/send.log"
  assert_no_grep 'treehouse' "$HOME_DIR/send.log" 'spawn invoked retired allocation transport'
  assert_no_grep 'MX_ATTEMPT_ID=' "$HOME_DIR/send.log" 'identity refusal sent canonical harness launch environment'
  assert_no_grep 'codex' "$HOME_DIR/send.log" 'identity refusal launched the harness'
  [ -f "$HOME_DIR/state/.spawn-$id.intent" ] || fail 'identity refusal lost durable launch intent'
  assert_absent "$HOME_DIR/state/$id.meta" 'identity refusal published a runnable task endpoint'
}

test_tampered_allocation_refuses_before_endpoint() {
  local variant rec id out status captured
  for variant in repository base; do
    id="settled-wrong-$variant"
    rec=$(make_settle_case "$id" "$id" 0)
    read_settle_record "$rec"
    captured=$(git -C "$PROJ_DIR" rev-parse HEAD)
    out=$(MX_TEST_ALLOCATION_TAMPER="$variant" run_settle_spawn "$id" --harness codex --backend tmux)
    status=$?
    expect_code 1 "$status" "settled wrong $variant must refuse"
    if [ "$variant" = repository ]; then
      assert_contains "$out" 'worktree identity mismatch' 'wrong-repository refusal lost its cause'
    else
      assert_contains "$out" 'acquisition HEAD differs from recorded base' 'wrong-base refusal lost its cause'
    fi
    assert_rejected_before_harness "$id"
    [ "$(git -C "$PROJ_DIR" rev-parse HEAD)" = "$captured" ] || fail 'identity refusal changed source revision'
    [ -d "$WT_DIR" ] || fail 'identity refusal deleted unrelated worktree'
  done
  pass 'tampered wrong repository and wrong base refuse before endpoint and retain launch intent'
}

test_queued_head_drift_never_retargets_the_accepted_base() {
  local rec id out status queued captured changed
  id=queued-base-drift
  rec=$(make_settle_case "$id" "$id" 0)
  read_settle_record "$rec"
  captured=$(git -C "$PROJ_DIR" rev-parse HEAD)
  # The shared fake-test harness bypasses queueing by default; this case must
  # exercise the real durable queue and its later drain.
  out=$(MX_HEADROOM_SKIP_QUEUE=0 MX_HEADROOM_API_CAPACITY=0 run_settle_spawn "$id" --harness codex --backend tmux)
  expect_code 0 "$?" 'initial queue request failed'
  assert_contains "$out" "queued: $id parked" 'request did not reach durable queue'
  queued=$(cat "$HOME_DIR/state/.dispatch-queue/$id.request")
  assert_grep "\"starting_revision\":\"$captured\"" "$HOME_DIR/state/.dispatch-queue/$id.request" 'queue did not freeze accepted starting revision'
  printf 'later source change\n' > "$PROJ_DIR/later-change"
  git -C "$PROJ_DIR" add later-change
  git -C "$PROJ_DIR" -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm 'source advanced while queued'
  changed=$(git -C "$PROJ_DIR" rev-parse HEAD)
  # Model the allocator choosing latest HEAD instead of the frozen queue base.
  # Both mutations affect only this synthetic repository and its test worktree.
  git -C "$WT_DIR" reset --hard "$changed" >/dev/null
  out=$(MX_ROOT_OVERRIDE='' MX_HOME="$HOME_DIR" \
    MX_STATE_OVERRIDE="$HOME_DIR/state" MX_DATA_OVERRIDE="$HOME_DIR/data" \
    MX_PROJECTS_OVERRIDE="$HOME_DIR/projects" MX_CONFIG_OVERRIDE="$HOME_DIR/config" \
    MX_SPAWN_NO_GUARD=1 TMUX="fake,1,0" \
    MX_HEADROOM_CPU_COUNT=8 MX_HEADROOM_LOAD1=0 MX_HEADROOM_MEM_AVAILABLE_BYTES=17179869184 \
    MX_HEADROOM_IN_USE=0 MX_HEADROOM_API_CAPACITY=4 MX_HEADROOM_SPAWN_BIN="$SPAWN" \
    MX_FAKE_SEND_LOG="$HOME_DIR/send.log" \
    MX_FAKE_PANE_PATH="$WT_DIR" MX_FAKE_PANE_STALE="$STALE_DIR" \
    MX_FAKE_PANE_STALE_READS=0 MX_FAKE_PANE_COUNTFILE="$COUNTFILE" \
    PATH="$FAKEBIN_DIR:$PATH" "$ROOT/bin/mx-headroom.sh" --queue-drain 2>&1)
  status=$?
  expect_code 0 "$status" 'queued spawn must acquire the captured revision'
  assert_absent "$HOME_DIR/state/.dispatch-queue/$id.request" 'successful queue drain kept request'
  [ "$(git -C "$PROJ_DIR" rev-parse HEAD)" = "$changed" ] || fail 'queue drift reset the user source'
  [ "$(git -C "$WT_DIR" rev-parse HEAD)" = "$changed" ] || fail 'queue drift reset the unrelated worktree'
  WT_DIR=$(sed -n 's/^worktree=//p' "$HOME_DIR/state/$id.meta")
  [ "$(git -C "$WT_DIR" rev-parse HEAD)" = "$captured" ] || fail 'queue drain retargeted accepted base'
  sed -n 's/^canonical_model=//p' "$HOME_DIR/state/$id.meta" | jq -e --arg base "$captured" '.project.starting_revision == $base and .allocation.base_revision == $base' >/dev/null || fail 'queued allocation lost base binding'
  pass 'queued HEAD drift acquires the frozen revision without changing source or unrelated worktree'

}

test_explicit_replacement_retains_its_recorded_worktree_commits() {
  local rec id out status first model captured retained second sends
  id=retained-worktree-replacement
  rec=$(make_settle_case "$id" "$id" 0)
  read_settle_record "$rec"
  captured=$(git -C "$PROJ_DIR" rev-parse HEAD)
  out=$(run_settle_spawn "$id" --harness codex --backend tmux)
  expect_code 0 "$?" "initial retained-worktree launch failed: $out"
  WT_DIR=$(sed -n 's/^worktree=//p' "$HOME_DIR/state/$id.meta")
  first=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/$id.meta" | jq -r '.attempt.id')
  printf 'retained task progress\n' > "$WT_DIR/task-progress"
  git -C "$WT_DIR" add task-progress
  git -C "$WT_DIR" -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm 'task progress before replacement'
  retained=$(git -C "$WT_DIR" rev-parse HEAD)

  out=$(run_settle_spawn "$id" --replace-attempt "$first" --harness codex --backend tmux)
  status=$?
  expect_code 0 "$status" "proven replacement refused retained commits: $out"
  assert_contains "$out" "spawned $id" 'replacement did not launch'
  model=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/$id.meta")
  second=$(printf '%s' "$model" | jq -r '.attempt.id')
  [ "$first" != "$second" ] || fail 'replacement reused prior attempt identity'
  printf '%s' "$model" | jq -e --arg first "$first" --arg base "$captured" \
    '.attempt.generation == 2 and .prior_attempts[0].id == $first and .project.starting_revision == $base' >/dev/null \
    || fail 'replacement lost previous attempt or changed accepted base'
  assert_grep "worktree=$WT_DIR" "$HOME_DIR/state/$id.meta" 'replacement changed recorded worktree'
  [ "$(git -C "$WT_DIR" rev-parse HEAD)" = "$retained" ] || fail 'replacement reset retained task commits'
  [ "$(git -C "$PROJ_DIR" rev-parse HEAD)" = "$captured" ] || fail 'replacement changed source HEAD'

  sends=$(cat "$HOME_DIR/send.log")
  out=$(run_settle_spawn "$id" --replace-attempt "$first" --harness codex --backend tmux)
  expect_code 1 "$?" 'stale replacement proof must refuse'
  [ "$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/$id.meta")" = "$model" ] || fail 'stale replacement rewrote current task'
  [ "$(cat "$HOME_DIR/send.log")" = "$sends" ] || fail 'stale replacement reached allocation or harness transport'
  [ "$(git -C "$WT_DIR" rev-parse HEAD)" = "$retained" ] || fail 'stale replacement changed retained task commits'
  pass 'proven replacement retains its recorded worktree commits and stale prior proof refuses unchanged'
}

test_tampered_allocation_refuses_before_endpoint
test_queued_head_drift_never_retargets_the_accepted_base
test_explicit_replacement_retains_its_recorded_worktree_commits

# The shared task lock must protect both a first launch and replacement from a
# concurrent teardown. A live process owns the real production lock format.
test_live_task_lifecycle_lock_fences_spawn_and_replacement() {
  local variant id rec out status owner pid_before identity_before identity_after meta_before sends_before lock ready release attempt tick
  for variant in first replacement; do
    id="lifecycle-contention-$variant"
    rec=$(make_settle_case "$id" "$id" 0)
    read_settle_record "$rec"
    attempt=
    if [ "$variant" = replacement ]; then
      out=$(run_settle_spawn "$id" --harness codex --backend tmux) || fail "initial contention fixture launch failed: $out"
      attempt=$(sed -n 's/^canonical_model=//p' "$HOME_DIR/state/$id.meta" | jq -r '.attempt.id')
      meta_before=$(cat "$HOME_DIR/state/$id.meta")
      sends_before=$(cat "$HOME_DIR/send.log")
    fi
    lock="$HOME_DIR/state/.teardown.$id.lock"
    ready="$HOME_DIR/lock-ready"
    release="$HOME_DIR/lock-release"
    ROOT="$ROOT" MX_ROOT_OVERRIDE= MX_HOME="$HOME_DIR" MX_STATE_OVERRIDE="$HOME_DIR/state" LOCK="$lock" READY="$ready" RELEASE="$release" bash -c '
      . "$ROOT/bin/mx-wake-lib.sh"
      mx_lock_try_acquire "$LOCK" || exit 1
      trap '\''mx_lock_release "$LOCK"'\'' EXIT
      mx_pid_identity "${BASHPID:-$$}" > "$LOCK/pid-identity" || exit 1
      : > "$READY"
      count=0
      while [ ! -e "$RELEASE" ] && [ "$count" -lt 1000 ]; do sleep 0.05; count=$((count + 1)); done
      [ -e "$RELEASE" ] || exit 1
    ' &
    owner=$!
    tick=0
    while [ ! -e "$ready" ] && kill -0 "$owner" 2>/dev/null && [ "$tick" -lt 100 ]; do
      sleep 0.05
      tick=$((tick + 1))
    done
    [ -e "$ready" ] || { kill "$owner" 2>/dev/null || true; wait "$owner" 2>/dev/null || true; fail 'could not acquire live task lifecycle fixture lock'; }
    pid_before=$(cat "$lock/pid")
    identity_before=$("$MX_RUST_BIN" primitive process-identity "$owner") || { : > "$release"; wait "$owner"; fail 'lock owner identity unavailable'; }
    [ "$pid_before" = "$owner" ] || { : > "$release"; wait "$owner"; fail 'fixture lock belongs to a different process'; }
    [ -s "$lock/pid-identity" ] || { : > "$release"; wait "$owner"; fail 'fixture did not record real process identity'; }
    if [ -n "$attempt" ]; then
      out=$(run_settle_spawn "$id" --replace-attempt "$attempt" --harness codex --backend tmux)
    else
      out=$(run_settle_spawn "$id" --harness codex --backend tmux)
    fi
    status=$?
    identity_after=$("$MX_RUST_BIN" primitive process-identity "$owner") || { : > "$release"; wait "$owner"; fail 'spawn lost live lock owner'; }
    : > "$release"
    wait "$owner" || fail 'task lifecycle lock holder could not release its lock'
    expect_code 1 "$status" 'spawn must refuse while teardown owns the live task lifecycle lock'
    assert_contains "$out" 'lock' 'task lifecycle lock contention lost its refusal reason'
    [ "$identity_before" = "$identity_after" ] || fail 'spawn replaced the live lock owner'
    assert_absent "$HOME_DIR/state/.spawn-$id.intent" 'contended spawn published or replaced a launch intent'
    if [ "$variant" = replacement ]; then
      [ "$(cat "$HOME_DIR/state/$id.meta")" = "$meta_before" ] || fail 'contended replacement rewrote accepted task metadata'
      [ "$(cat "$HOME_DIR/send.log")" = "$sends_before" ] || fail 'contended replacement touched the runtime endpoint'
    else
      assert_absent "$HOME_DIR/state/$id.meta" 'contended first launch published endpoint metadata'
      assert_absent "$HOME_DIR/send.log" 'contended first launch created an endpoint'
      out=$(run_settle_spawn "$id" --harness codex --backend tmux) || fail "spawn failed after owner released lock: $out"
      sed -n 's/^canonical_model=//p' "$HOME_DIR/state/$id.meta" | jq -e '.attempt.generation == 1' >/dev/null || fail 'contention consumed an attempt generation'
    fi
  done
  pass 'live task lifecycle ownership fences first launch and replacement without endpoint or intent mutation'
}

test_live_task_lifecycle_lock_fences_spawn_and_replacement

test_single_checkout_endpoint_failure_records_consumed_grant_without_deleting_source() {
  local rec id bindings request out status base intent
  id=single-checkout-failed
  rec=$(make_settle_case single-checkout-failed "$id" 0)
  read_settle_record "$rec"
  base=$(git -C "$PROJ_DIR" rev-parse HEAD)
  bindings=$(MX_HOME="$HOME_DIR" MX_STATE_OVERRIDE="$HOME_DIR/state" \
    "$ROOT/bin/mx-override-bindings.sh" single-checkout "$id" "$PROJ_DIR") || fail 'single-checkout failure binding failed'
  request=$(grant_binding "$HOME_DIR/state" "$bindings") || fail 'single-checkout failure grant failed'
  out=$(MX_SPAWN_FAULT=after-endpoint run_settle_spawn "$id" --single-checkout "$request")
  status=$?
  expect_code 1 "$status" 'single-checkout endpoint failure'
  assert_contains "$out" 'injected failure after endpoint' 'single-checkout failed before endpoint boundary'
  [ "$(jq -r '.outcome' "$HOME_DIR/state/maintainer-overrides/consumed/$request.json")" = failed ] || fail 'failed single-checkout grant outcome was not recorded'
  [ -z "$(find "$HOME_DIR/state" -maxdepth 1 -name '.single-checkout-*.json' -print)" ] || fail 'failed launch kept active single-checkout reservation'
  assert_absent "$HOME_DIR/state/$id.meta" 'failed single-checkout published task metadata'
  intent="$HOME_DIR/state/.spawn-$id.intent"
  [ -f "$intent" ] || fail 'failed single-checkout lost recovery intent'
  [ "$(git -C "$PROJ_DIR" rev-parse HEAD)" = "$base" ] || fail 'failed single-checkout rewrote source'
  [ -d "$PROJ_DIR/.git" ] || fail 'failed single-checkout deleted source checkout'
  out=$(run_settle_spawn "$id" --single-checkout "$request")
  status=$?
  expect_code 1 "$status" 'consumed single-checkout grant replay'
  assert_contains "$out" 'already consumed' 'single-checkout failure grant could be reused'
  [ -f "$intent" ] || fail 'grant replay deleted recovery intent'
  pass 'single-checkout endpoint failure consumes grant once, releases reservation, and retains source plus recovery intent'
}

test_single_checkout_endpoint_failure_records_consumed_grant_without_deleting_source
