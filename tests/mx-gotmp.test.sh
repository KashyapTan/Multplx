#!/usr/bin/env bash
# Behavior tests for per-task GOTMPDIR support (mx-gotmp).
#
# mx-spawn gives each task a temp root /tmp/mx-<id>/ with Go's build temp nested at
# gotmp/, exports GOTMPDIR into the actor pane, and records tasktmp= in the task's
# meta. mx-teardown reads tasktmp= and removes the whole root on cleanup.
#
# These tests exercise behavior directly: mx-teardown is run as a subprocess against a
# fake MX_HOME/MX_ROOT (built so the real script resolves into it), with stub helper scripts.
# Nothing is sourced. The mx-spawn side is verified both structurally (the source has
# the contract lines) and behaviorally (the mkdir + meta-write pattern it uses).
set -u

# This suite does not source tests/lib.sh, so exempt its teardown subprocess from
# the gate-lifecycle refusal (bin/mx-gate-refuse-lib.sh) the way lib.sh does for
# the rest of the suite: focused deep-review tests run from a gate worktree,
# which the guard would otherwise refuse.
export MX_GATE_REFUSE_BYPASS=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPAWN="$ROOT/bin/mx-spawn.sh"
TEARDOWN="$ROOT/bin/mx-teardown.sh"

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

pass() {
  printf 'ok - %s\n' "$1"
}

TMP_ROOT=

cleanup() {
  if [ -n "${TMP_ROOT:-}" ]; then
    rm -rf "$TMP_ROOT"
  fi
}
trap cleanup EXIT

TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mx-gotmp-tests.XXXXXX")

# Build a fake MX_HOME/MX_ROOT so the real mx-teardown.sh (symlinked in) resolves
# state and helper scripts inside it. Stub the helper scripts mx-teardown calls so no
# live tmux/treehouse/system state is touched. A nonexistent worktree path makes both
# `if [ -d "$WT" ]` guards skip, so teardown runs straight to the cleanup + state rm.
make_fake_root() {
  local id=$1 tasktmp=$2
  local fake="$TMP_ROOT/$id"
  mkdir -p "$fake/bin/backends" "$fake/state"
  # Symlink the REAL teardown so the test exercises actual code, not a copy.
  ln -s "$TEARDOWN" "$fake/bin/mx-teardown.sh"
  # mx-backend.sh + its tmux adapter: symlink the REAL files (teardown sources
  # mx-backend.sh unconditionally, and dispatches the kill call through the
  # tmux adapter; both are unchanged by this suite's fixture, just newly
  # required siblings since the P1 backend extraction).
  ln -s "$ROOT/bin/mx-backend.sh" "$fake/bin/mx-backend.sh"
  ln -s "$ROOT/bin/backends/tmux.sh" "$fake/bin/backends/tmux.sh"
  ln -s "$ROOT/bin/mx-tmux-lib.sh" "$fake/bin/mx-tmux-lib.sh"
  ln -s "$ROOT/bin/mx-composer-lib.sh" "$fake/bin/mx-composer-lib.sh"
  # mx-lock-lib.sh: teardown sources it for the shared lock-staleness proof.
  ln -s "$ROOT/bin/mx-lock-lib.sh" "$fake/bin/mx-lock-lib.sh"
  # mx-gate-refuse-lib.sh: teardown sources it before any system mutation.
  ln -s "$ROOT/bin/mx-gate-refuse-lib.sh" "$fake/bin/mx-gate-refuse-lib.sh"
  # mx-pr-lib.sh: teardown uses its canonical task-ID validator for poll cleanup.
  ln -s "$ROOT/bin/mx-pr-lib.sh" "$fake/bin/mx-pr-lib.sh"
  # Maintainer-override state is sourced unconditionally even when this ordinary
  # landed-cleanup fixture does not consume an exceptional grant.
  ln -s "$ROOT/bin/mx-maintainer-override-lib.sh" "$fake/bin/mx-maintainer-override-lib.sh"
  # mx-guard.sh: stub (teardown calls it with `|| true`).
  cat > "$fake/bin/mx-guard.sh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fake/bin/mx-guard.sh"
  # mx-system-sync.sh: stub (called for non-scout/non-local-only teardowns).
  cat > "$fake/bin/mx-system-sync.sh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fake/bin/mx-system-sync.sh"
  ln -s "$ROOT/bin/mx-backlog-lib.sh" "$fake/bin/mx-backlog-lib.sh"
  # Meta with a nonexistent worktree so the dirty/treehouse blocks skip.
  cat > "$fake/state/$id.meta" <<META
window=fakeses:mx-$id
worktree=$TMP_ROOT/nonexistent-worktree-$id
project=$TMP_ROOT/nonexistent-project-$id
harness=claude
kind=delivery
mode=deep-review
yolo=off
tasktmp=$tasktmp
META
  printf '%s' "$fake"
}

# --- mx-spawn side ---

test_spawn_contract_and_mkdir_pattern() {
  # Structural: mx-spawn must create the gotmp dir, record tasktmp in meta, and export
  # GOTMPDIR into the pane. Assert the contract lines are present in the source.
  # shellcheck disable=SC2016  # single quotes are deliberate: these are literal source strings
  grep -F 'mkdir -p "$TASK_TMP/gotmp"' "$SPAWN" >/dev/null \
    || fail "mx-spawn missing: mkdir of gotmp under TASK_TMP"
  # shellcheck disable=SC2016  # single quotes are deliberate: literal source string
  grep -F 'echo "tasktmp=$TASK_TMP"' "$SPAWN" >/dev/null \
    || fail "mx-spawn missing: tasktmp= line in meta write"
  grep -F 'export GOTMPDIR=' "$SPAWN" >/dev/null \
    || fail "mx-spawn missing: GOTMPDIR export into pane"
  # Behavioral: the mkdir + meta-write pattern spawn uses must produce a gotmp dir and
  # a meta line whose value the teardown grep (tasktmp=, cut -d= -f2-) reads back whole.
  local id=spawn-sim-z1
  local sim_root="$TMP_ROOT/$id-root"
  local task_tmp="$sim_root/tmp/mx-$id"
  mkdir -p "$sim_root/state"
  # Replicate spawn's exact mkdir + meta-write lines.
  TASK_TMP="$task_tmp"
  mkdir -p "$TASK_TMP/gotmp"
  {
    echo "tasktmp=$TASK_TMP"
  } > "$sim_root/state/$id.meta"
  [ -d "$task_tmp/gotmp" ] || fail "simulated spawn did not create gotmp dir"
  # Teardown reads tasktmp= with `grep '^tasktmp=' | cut -d= -f2-`; round-trip it.
  local read_back
  read_back=$(grep '^tasktmp=' "$sim_root/state/$id.meta" | cut -d= -f2-)
  [ "$read_back" = "$task_tmp" ] \
    || fail "tasktmp value not round-tripped by teardown's grep|cut (got '$read_back')"
  pass "mx-spawn creates gotmp dir and records tasktmp in meta"
}

# --- mx-teardown side (real subprocess) ---

test_teardown_removes_tasktmp_dir() {
  local id=td-rm-z2
  local task_tmp="$TMP_ROOT/mx-$id"
  mkdir -p "$task_tmp/gotmp"
  printf 'leftover\n' > "$task_tmp/gotmp/build-artifact"
  local fake
  fake=$(make_fake_root "$id" "$task_tmp")
  # Sanity: dir + contents exist before teardown.
  [ -d "$task_tmp/gotmp" ] || fail "precondition: gotmp missing before teardown"
  # Run the REAL teardown against the fake root.
  MX_HOME="$fake" bash "$fake/bin/mx-teardown.sh" "$id" >/dev/null 2>&1 \
    || fail "teardown exited non-zero with a valid tasktmp"
  [ ! -e "$task_tmp" ] \
    || fail "teardown did not remove the tasktmp dir ($task_tmp still exists)"
  pass "mx-teardown removes the dir pointed to by tasktmp= in meta"
}

test_teardown_skips_gracefully_without_tasktmp() {
  # Backward compat: a meta from a pre-fix task has no tasktmp= line. Teardown must
  # not error and must not remove anything.
  local id=td-absent-z3
  local fake="$TMP_ROOT/$id-root"
  mkdir -p "$fake/bin/backends" "$fake/state"
  ln -s "$TEARDOWN" "$fake/bin/mx-teardown.sh"
  ln -s "$ROOT/bin/mx-backend.sh" "$fake/bin/mx-backend.sh"
  ln -s "$ROOT/bin/backends/tmux.sh" "$fake/bin/backends/tmux.sh"
  ln -s "$ROOT/bin/mx-tmux-lib.sh" "$fake/bin/mx-tmux-lib.sh"
  ln -s "$ROOT/bin/mx-composer-lib.sh" "$fake/bin/mx-composer-lib.sh"
  ln -s "$ROOT/bin/mx-lock-lib.sh" "$fake/bin/mx-lock-lib.sh"
  # mx-gate-refuse-lib.sh: teardown sources it before any system mutation.
  ln -s "$ROOT/bin/mx-gate-refuse-lib.sh" "$fake/bin/mx-gate-refuse-lib.sh"
  # mx-pr-lib.sh: teardown uses its canonical task-ID validator for poll cleanup.
  ln -s "$ROOT/bin/mx-pr-lib.sh" "$fake/bin/mx-pr-lib.sh"
  ln -s "$ROOT/bin/mx-maintainer-override-lib.sh" "$fake/bin/mx-maintainer-override-lib.sh"
  cat > "$fake/bin/mx-guard.sh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fake/bin/mx-guard.sh"
  cat > "$fake/bin/mx-system-sync.sh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fake/bin/mx-system-sync.sh"
  ln -s "$ROOT/bin/mx-backlog-lib.sh" "$fake/bin/mx-backlog-lib.sh"
  # No tasktmp= line at all.
  cat > "$fake/state/$id.meta" <<META
window=fakeses:mx-$id
worktree=$TMP_ROOT/nonexistent-wt-$id
project=$TMP_ROOT/nonexistent-proj-$id
harness=claude
kind=delivery
mode=deep-review
yolo=off
META
  MX_HOME="$fake" bash "$fake/bin/mx-teardown.sh" "$id" >/dev/null 2>&1 \
    || fail "teardown exited non-zero when tasktmp= was absent"
  pass "mx-teardown skips gracefully when tasktmp= is absent (backward compat)"
}

test_teardown_skips_gracefully_when_dir_missing() {
  # tasktmp= points to a path that does not exist. Teardown must not error.
  local id=td-missing-z4
  local task_tmp="$TMP_ROOT/never-created-mx-$id"
  # Intentionally do NOT create $task_tmp.
  [ ! -e "$task_tmp" ] || fail "precondition: task_tmp should not exist yet"
  local fake
  fake=$(make_fake_root "$id" "$task_tmp")
  MX_HOME="$fake" bash "$fake/bin/mx-teardown.sh" "$id" >/dev/null 2>&1 \
    || fail "teardown exited non-zero when tasktmp dir was missing"
  [ ! -e "$task_tmp" ] || fail "teardown created/left the tasktmp dir unexpectedly"
  pass "mx-teardown skips gracefully when tasktmp= points to a nonexistent dir"
}

test_spawn_contract_and_mkdir_pattern
test_teardown_removes_tasktmp_dir
test_teardown_skips_gracefully_without_tasktmp
test_teardown_skips_gracefully_when_dir_missing
