#!/usr/bin/env bash
# Tests for bin/mx-update.sh: fast-forward-only self-update of a running
# Multplx repo and every registered daemon home.
#
# The guarantees under test mirror mx-system-sync.sh and prime directive #3:
#   - The running Multplx repo (on its default branch) fast-forwards from
#     origin; a leased daemon home (detached HEAD on the default branch)
#     fast-forwards the same way.
#   - FAST-FORWARD ONLY: a dirty, diverged, offline, or wrong-branch target is
#     skipped and reported, never forced or stashed, so unlanded work survives.
#   - The update is a single-parent fast-forward (never a merge commit) and a
#     fast-forward of one worktree never disturbs another worktree's checkout
#     or the shared default branch.
#   - The caller-action summary is correct: reread-broker flips to yes only
#     when the instruction surface (AGENTS.md / bin / .agents/skills) changed, and
#     nudge-daemons lists exactly the live daemons that advanced.
#   - Daemon homes resolve from both state/<id>.meta and the
#     data/daemons.md registry, deduped, and the Multplx repo is never
#     re-processed as one of its own daemons.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

UPDATE="$ROOT/bin/mx-update.sh"
RUST_BIN=${MX_RUST_BIN:-$ROOT/target/release/mx}

# Deterministic, isolated git identity for fixture commits.
mx_git_identity fmtest fmtest@example.com

TMP_ROOT=$(mx_test_tmproot mx-update-tests)

# Build a fresh world: a bare origin seeded with one commit, a Multplx repo
# clone checked out on main, and a home dir with state/ and data/. Echoes the
# world dir. Files seeded: AGENTS.md, README.md, bin/tool.sh, and an internal skill note.
new_world() {
  local name=$1 w
  w="$TMP_ROOT/$name"
  mkdir -p "$w/home/state" "$w/home/data" "$w/home/config" "$w/home/projects"
  # Fresh watcher beacon keeps mx-guard quiet.
  touch "$w/home/state/.last-watcher-beat"

  git init -q --bare "$w/origin.git"
  git -C "$w/origin.git" symbolic-ref HEAD refs/heads/main
  git clone -q "$w/origin.git" "$w/seed" 2>/dev/null

  printf 'v1\n' > "$w/seed/AGENTS.md"
  printf 'r1\n' > "$w/seed/README.md"
  mkdir -p "$w/seed/bin" "$w/seed/.agents/skills"
  printf 'echo a\n' > "$w/seed/bin/tool.sh"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$w/seed/bin/mx-launcher.sh"
  chmod +x "$w/seed/bin/mx-launcher.sh"
  printf 's1\n' > "$w/seed/.agents/skills/note.md"
  git -C "$w/seed" add -A
  git -C "$w/seed" commit -qm c1
  git -C "$w/seed" push -q origin main

  git clone -q "$w/origin.git" "$w/main"
  git -C "$w/main" remote set-head origin main >/dev/null 2>&1 || true

  printf '%s\n' "$w"
}

# Add a daemon home as a DETACHED worktree of the Multplx repo (matching
# how treehouse leases a daemon home), plus its state meta. Args: world id.
add_sm() {
  local w=$1 id=$2
  git -C "$w/main" worktree add -q --detach "$w/$id" main
  {
    printf 'window=main:mx-%s\n' "$id"
    printf 'kind=daemon\n'
    printf 'home=%s/%s\n' "$w" "$id"
  } > "$w/home/state/$id.meta"
  printf '%s\n' "$id" > "$w/$id/.mx-daemon-home"
}

# Advance origin by one commit. mode=instr changes the instruction surface
# (AGENTS.md, bin, .agents/skills) plus README; mode=readme changes only README.
bump_origin() {
  local w=$1 mode=$2
  git -C "$w/seed" pull -q origin main >/dev/null 2>&1 || true
  printf 'r-%s\n' "$mode" >> "$w/seed/README.md"
  if [ "$mode" = instr ]; then
    printf 'v2\n' > "$w/seed/AGENTS.md"
    printf 'echo b\n' > "$w/seed/bin/tool.sh"
    printf 's2\n' > "$w/seed/.agents/skills/note.md"
  fi
  git -C "$w/seed" add -A
  git -C "$w/seed" commit -qm "bump-$mode"
  git -C "$w/seed" push -q origin main
}

run_update() {
  local w=$1
  MX_ROOT_OVERRIDE="$w/main" MX_HOME="$w/home" "$UPDATE" 2>/dev/null
}

# --- T1: main + daemon behind, instruction change; FF, not a merge ------
# Combines the former T1 (fast-forward + reread + nudge signalling) and T2
# (the advance is a single-parent fast-forward, never a merge commit) into one
# world so both contracts are proven against the same update run.
test_updates_main_and_daemon() {
  local w out
  w=$(new_world t1)
  add_sm "$w" sm1
  bump_origin "$w" instr

  out=$(run_update "$w")

  assert_contains "$out" "broker: updated " "broker fast-forwarded"
  assert_contains "$out" "daemon sm1: updated " "daemon fast-forwarded"
  assert_contains "$out" "reread-broker: yes" "instruction change triggers reread"
  assert_contains "$out" "nudge-daemons: mx-sm1" "updated daemon is nudged"

  # Fast-forward landed: HEAD == origin/main on both targets.
  [ "$(git -C "$w/main" rev-parse HEAD)" = "$(git -C "$w/main" rev-parse origin/main)" ] \
    || fail "broker HEAD not at origin/main"
  [ "$(git -C "$w/sm1" rev-parse HEAD)" = "$(git -C "$w/sm1" rev-parse origin/main)" ] \
    || fail "daemon HEAD not at origin/main"
  # Multplx stays on its default branch; daemon stays detached.
  [ "$(git -C "$w/main" symbolic-ref --short HEAD 2>/dev/null)" = "main" ] \
    || fail "broker left its default branch"
  git -C "$w/sm1" symbolic-ref -q HEAD >/dev/null \
    && fail "daemon worktree is no longer detached"
  # A fast-forwarded tip has exactly one parent; a merge commit would have two.
  [ "$(git -C "$w/main" rev-list --parents -n1 HEAD | wc -w | tr -d ' ')" -eq 2 ] \
    || fail "broker tip is not a single-parent fast-forward"
  [ "$(git -C "$w/sm1" rev-list --parents -n1 HEAD | wc -w | tr -d ' ')" -eq 2 ] \
    || fail "daemon tip is not a single-parent fast-forward"
  pass "T1 main + daemon fast-forward (single-parent), reread + nudge signalled"
}

# --- T3: README-only change does not trigger a reread ----------------------
test_reread_gate_is_instruction_only() {
  local w out
  w=$(new_world t3)
  add_sm "$w" sm1
  bump_origin "$w" readme

  out=$(run_update "$w")

  assert_contains "$out" "broker: updated " "broker still advanced"
  assert_contains "$out" "reread-broker: no" "non-instruction change skips reread"
  # The daemon still advanced, so it is still nudged (update-based nudge).
  assert_contains "$out" "nudge-daemons: mx-sm1" "advanced daemon still nudged"
  pass "T3 reread gates on instruction surface, nudge on advancement"
}

# --- T4: dirty daemon is skipped, its edit preserved -------------------
test_dirty_daemon_skipped() {
  local w out
  w=$(new_world t4)
  add_sm "$w" sm1
  bump_origin "$w" instr
  printf 'uncommitted local edit\n' >> "$w/sm1/AGENTS.md"

  out=$(run_update "$w")

  assert_contains "$out" "daemon sm1: skipped: dirty working tree" "dirty home skipped"
  assert_not_contains "$out" "mx-sm1" "skipped daemon is not nudged"
  grep -q 'uncommitted local edit' "$w/sm1/AGENTS.md" \
    || fail "dirty edit was discarded"
  pass "T4 dirty daemon skipped, local edit preserved"
}

# --- T5: diverged daemon is skipped, its commit preserved --------------
test_diverged_daemon_skipped() {
  local w out before
  w=$(new_world t5)
  add_sm "$w" sm1
  # Local commit on the daemon's detached HEAD makes it diverge from origin.
  printf 'fork work\n' > "$w/sm1/AGENTS.md"
  git -C "$w/sm1" add -A
  git -C "$w/sm1" commit -qm local-work
  before=$(git -C "$w/sm1" rev-parse HEAD)
  bump_origin "$w" instr

  out=$(run_update "$w")

  assert_contains "$out" "daemon sm1: skipped: diverged from origin/main" "diverged home skipped"
  assert_not_contains "$out" "mx-sm1" "diverged daemon is not nudged"
  [ "$(git -C "$w/sm1" rev-parse HEAD)" = "$before" ] \
    || fail "diverged daemon HEAD moved (unlanded work at risk)"
  pass "T5 diverged daemon skipped, local commit preserved"
}

# --- T6: idempotent; second run reports already current --------------------
test_idempotent_already_current() {
  local w out
  w=$(new_world t6)
  add_sm "$w" sm1
  bump_origin "$w" instr
  run_update "$w" >/dev/null   # first run advances both

  out=$(run_update "$w")       # second run: nothing to do

  assert_contains "$out" "broker: already current" "broker already current"
  assert_contains "$out" "daemon sm1: already current" "daemon already current"
  assert_contains "$out" "reread-broker: no" "no reread when nothing changed"
  assert_contains "$out" "nudge-daemons: none" "no nudge when nothing advanced"
  pass "T6 idempotent: a second run is a no-op"
}

# --- T7: registry backstop + dedup + self-exclusion, one world -------------
# One world carries every daemon-resolution edge at once:
#   reg1 - registered in daemons.md only, NO live meta (registry backstop);
#   sm1  - present in BOTH meta and the registry (must be processed exactly once);
#   selfish - a bogus registry line pointing the Multplx repo at itself.
# Asserts: reg1 advances but is NOT nudged (no live metadata); sm1 advances,
# is processed once, and IS nudged; the Multplx repo is never re-processed.
test_registry_backstop_dedup_and_self_exclusion() {
  local w out count
  w=$(new_world t7)
  add_sm "$w" sm1
  git -C "$w/main" worktree add -q --detach "$w/reg1" main
  printf 'reg1\n' > "$w/reg1/.mx-daemon-home"
  {
    printf -- '- reg1 - domain supervisor (home: %s/reg1; scope: things; projects: p; added 2026-06-23)\n' "$w"
    printf -- '- sm1 - dup (home: %s/sm1; scope: x; projects: p; added 2026-06-23)\n' "$w"
    printf -- '- selfish - self (home: %s/main; scope: x; projects: p; added 2026-06-23)\n' "$w"
  } > "$w/home/data/daemons.md"
  bump_origin "$w" instr

  out=$(run_update "$w")

  assert_contains "$out" "daemon reg1: updated " "registry-only daemon fast-forwarded"
  assert_contains "$out" "daemon sm1: updated " "meta+registry daemon fast-forwarded"
  count=$(printf '%s\n' "$out" | grep -c '^daemon sm1:' || true)
  [ "$count" -eq 1 ] || fail "daemon sm1 processed $count times, expected 1 (dedup across meta+registry)"
  assert_not_contains "$out" "daemon selfish" "Multplx repo re-processed as its own daemon"
  # sm1 has live metadata, so it is nudged; reg1 has none, so it is not. Pin the
  # nudge line exactly and confirm reg1 is absent from it (not from the whole
  # output, where 'daemon reg1: updated' legitimately appears).
  local nudge_line
  nudge_line=$(printf '%s\n' "$out" | grep '^nudge-daemons:')
  assert_contains "$nudge_line" "mx-sm1" "live-meta daemon is nudged"
  assert_not_contains "$nudge_line" "reg1" "registry-only daemon without live metadata is not nudged"
  pass "T7 registry backstop resolves, dedups meta+registry, excludes the Multplx repo"
}

# --- T9: Multplx repo on a feature branch is skipped ---------------------
test_broker_wrong_branch_skipped() {
  local w out before
  w=$(new_world t9)
  bump_origin "$w" instr
  # Simulate broker mid-delivery its own change: not on the default branch.
  git -C "$w/main" checkout -q -b feature/wip
  before=$(git -C "$w/main" rev-parse HEAD)

  out=$(run_update "$w")

  assert_contains "$out" "broker: skipped: on feature/wip, expected main" "off-default broker skipped"
  assert_contains "$out" "reread-broker: no" "no reread when broker was skipped"
  [ "$(git -C "$w/main" rev-parse HEAD)" = "$before" ] \
    || fail "skipped broker HEAD moved"
  pass "T9 broker off its default branch is skipped, not forced"
}

test_broker_detached_head_skipped() {
  local w out before
  w=$(new_world t10)
  bump_origin "$w" instr
  git -C "$w/main" checkout -q --detach HEAD
  before=$(git -C "$w/main" rev-parse HEAD)

  out=$(run_update "$w")

  assert_contains "$out" "broker: skipped: detached HEAD, expected main" "detached broker skipped"
  assert_contains "$out" "reread-broker: no" "no reread when detached broker was skipped"
  [ "$(git -C "$w/main" rev-parse HEAD)" = "$before" ] \
    || fail "detached broker HEAD moved"
  pass "T10 broker detached HEAD is skipped"
}

test_unsafe_daemon_home_skipped_before_git_update() {
  local w out bad before
  w=$(new_world t11)
  bad="$w/home/projects/bad"
  mkdir -p "$w/home/projects"
  git clone -q "$w/origin.git" "$bad"
  printf 'bad\n' > "$bad/.mx-daemon-home"
  before=$(git -C "$bad" rev-parse HEAD)
  printf -- '- bad - bad home (home: %s; scope: x; projects: p; added 2026-06-23)\n' \
    "$bad" > "$w/home/data/daemons.md"
  bump_origin "$w" instr

  out=$(run_update "$w")

  assert_contains "$out" "daemon bad: skipped: unsafe home: daemon home cannot be inside the active Multplx home" \
    "unsafe project-like home skipped"
  assert_contains "$out" "nudge-daemons: none" "unsafe home is not nudged"
  [ "$(git -C "$bad" rev-parse HEAD)" = "$before" ] \
    || fail "unsafe daemon home HEAD moved"
  pass "T11 unsafe daemon home is not fast-forwarded"
}

test_registered_launcher_binary_updates_after_fast_forward() {
  local w fakebin installed config old_hash new_hash out
  w=$(new_world t12)
  bump_origin "$w" readme
  fakebin="$w/fakebin"
  installed="$w/installed/multplx"
  config="$w/launcher-config"
  mkdir -p "$fakebin" "${installed%/*}" "$config"
  cp "$RUST_BIN" "$installed"
  printf '\0old-installed-generation\0' >>"$installed"
  chmod +x "$installed"
  old_hash=$(shasum -a 256 "$installed" | awk '{print $1}')
  new_hash=$(shasum -a 256 "$RUST_BIN" | awk '{print $1}')
  printf '%s\n' "$(cd "$w/main" && pwd -P)" >"$config/root"
  printf '%s\n' "$(cd "$w/home" && pwd -P)" >"$config/home"
  printf '%s\n' "$old_hash" >"$config/binary.sha256"
  printf '%s\n' "$(cd "$config" && pwd -P)" >"${installed%/*}/.multplx-config"
  chmod 600 "$config/root" "$config/home" "$config/binary.sha256" \
    "${installed%/*}/.multplx-config"
  cat >"$fakebin/cargo" <<'SH'
#!/usr/bin/env bash
set -eu
[ "${MX_UPDATE_CARGO_FAIL:-0}" != 1 ] || exit 42
mkdir -p "$MX_ROOT_OVERRIDE/target/release"
cp "$MX_UPDATE_ARTIFACT" "$MX_ROOT_OVERRIDE/target/release/mx"
chmod +x "$MX_ROOT_OVERRIDE/target/release/mx"
SH
  chmod +x "$fakebin/cargo"
  out=$(PATH="$fakebin:$PATH" MX_UPDATE_ARTIFACT="$RUST_BIN" \
    MX_ROOT_OVERRIDE="$w/main" MX_HOME="$w/home" \
    MX_LAUNCH_CONFIG_DIR="$config" MX_LAUNCH_BIN_PATH="$installed" \
    "$UPDATE" 2>"$w/update.err") \
    || fail "registered launcher update failed: $(cat "$w/update.err")"
  assert_contains "$out" "broker: updated " "source checkout fast-forwarded before binary update"
  assert_contains "$out" "launcher-binary: updated" "installed launcher update was not reported"
  [ "$(shasum -a 256 "$installed" | awk '{print $1}')" = "$new_hash" ] \
    || fail "installed launcher did not receive the rebuilt artifact"
  [ "$(cat "$config/binary.sha256")" = "$new_hash" ] \
    || fail "installed launcher digest did not advance with its binary"
  [ "$(cat "$config/root")" = "$(cd "$w/main" && pwd -P)" ] \
    && [ "$(cat "$config/home")" = "$(cd "$w/home" && pwd -P)" ] \
    || fail "binary update changed registered root or home"
  pass "T12 source fast-forward rebuilds and transactionally upgrades the registered launcher"
}

test_failed_release_build_preserves_installed_generation() {
  local w fakebin installed config old_hash new_hash out status
  w=$(new_world t13)
  bump_origin "$w" readme
  fakebin="$w/fakebin"
  installed="$w/installed/multplx"
  config="$w/launcher-config"
  mkdir -p "$fakebin" "${installed%/*}" "$config"
  cp "$RUST_BIN" "$installed"
  printf '\0preserved-installed-generation\0' >>"$installed"
  chmod +x "$installed"
  old_hash=$(shasum -a 256 "$installed" | awk '{print $1}')
  printf '%s\n' "$(cd "$w/main" && pwd -P)" >"$config/root"
  printf '%s\n' "$(cd "$w/home" && pwd -P)" >"$config/home"
  printf '%s\n' "$old_hash" >"$config/binary.sha256"
  printf '%s\n' "$(cd "$config" && pwd -P)" >"${installed%/*}/.multplx-config"
  cat >"$fakebin/cargo" <<'SH'
#!/usr/bin/env bash
exit 42
SH
  chmod +x "$fakebin/cargo"
  if PATH="$fakebin:$PATH" MX_ROOT_OVERRIDE="$w/main" MX_HOME="$w/home" \
      MX_LAUNCH_CONFIG_DIR="$config" MX_LAUNCH_BIN_PATH="$installed" \
      "$UPDATE" >"$w/update.out" 2>"$w/update.err"; then
    status=0
  else
    status=$?
  fi
  expect_code 1 "$status" "failed post-fast-forward release build"
  [ "$(shasum -a 256 "$installed" | awk '{print $1}')" = "$old_hash" ] \
    || fail "failed release build changed the installed launcher"
  [ "$(cat "$config/binary.sha256")" = "$old_hash" ] \
    || fail "failed release build changed the installed digest"
  [ "$(git -C "$w/main" rev-parse HEAD)" = "$(git -C "$w/main" rev-parse origin/main)" ] \
    || fail "source fast-forward did not complete before the release build failure"
  [ -f "$config/.launcher-update-pending" ] \
    || fail "failed release build did not leave a retry marker"

  cat >"$fakebin/cargo" <<'SH'
#!/usr/bin/env bash
set -eu
mkdir -p "$MX_ROOT_OVERRIDE/target/release"
cp "$MX_UPDATE_ARTIFACT" "$MX_ROOT_OVERRIDE/target/release/mx"
chmod +x "$MX_ROOT_OVERRIDE/target/release/mx"
SH
  chmod +x "$fakebin/cargo"
  new_hash=$(shasum -a 256 "$RUST_BIN" | awk '{print $1}')
  out=$(PATH="$fakebin:$PATH" MX_UPDATE_ARTIFACT="$RUST_BIN" \
    MX_ROOT_OVERRIDE="$w/main" MX_HOME="$w/home" \
    MX_LAUNCH_CONFIG_DIR="$config" MX_LAUNCH_BIN_PATH="$installed" \
    "$UPDATE" 2>"$w/retry.err") \
    || fail "pending launcher update did not retry: $(cat "$w/retry.err")"
  assert_contains "$out" "broker: already current" "retry unexpectedly moved source again"
  assert_contains "$out" "launcher-binary: updated" "pending launcher update was not retried"
  [ "$(shasum -a 256 "$installed" | awk '{print $1}')" = "$new_hash" ] \
    && [ "$(cat "$config/binary.sha256")" = "$new_hash" ] \
    || fail "retry did not atomically advance the installed launcher generation"
  [ ! -e "$config/.launcher-update-pending" ] \
    || fail "successful launcher retry did not clear its pending marker"
  pass "T13 failed rebuild preserves and later recovers the installed launcher generation"
}

test_updates_main_and_daemon
test_reread_gate_is_instruction_only
test_dirty_daemon_skipped
test_diverged_daemon_skipped
test_idempotent_already_current
test_registry_backstop_dedup_and_self_exclusion
test_broker_wrong_branch_skipped
test_broker_detached_head_skipped
test_unsafe_daemon_home_skipped_before_git_update
test_registered_launcher_binary_updates_after_fast_forward
test_failed_release_build_preserves_installed_generation

echo "# all mx-update tests passed"
