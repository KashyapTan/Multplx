#!/usr/bin/env bash
# mx-test-run.sh - single owner of Multplx's behavior-test inventory, resource
# manifest, scheduler, CI shard composition, timing, and coverage guard.
#
# Selection modes (exactly one of: --all, --family, --changed, --lane,
# --proven-isolated, or script paths):
#   mx-test-run.sh --all
#   mx-test-run.sh --family <name>
#   mx-test-run.sh --changed [--base <git-ref>]
#   mx-test-run.sh --lane portable-parallel-1|portable-parallel-2|portable-serial
#   mx-test-run.sh --proven-isolated
#   mx-test-run.sh tests/<name>.test.sh [more scripts...]
#
# Inspection (no execution):
#   mx-test-run.sh --list --all
#   mx-test-run.sh --list --family <name>
#   mx-test-run.sh --list --lane portable-parallel-1
#   mx-test-run.sh --list-families
#   mx-test-run.sh --list-lanes
#   mx-test-run.sh --check-coverage
#
# Aggregation (no suite execution):
#   mx-test-run.sh --aggregate-json <out.json> <lane.json> [more lane.json...]
#   mx-test-run.sh --compare-json <serial.json> <accelerated.json>
#
# Options:
#   --json <path>   write a deterministic timing artifact after the run
#   --list          print selected script paths (one per line) and exit 0
#   --base <ref>    with --changed, compare against this ref (default: origin/main)
#   --exclude-family <name>
#                   drop scripts whose primary family matches <name> after selection
#                   (repeatable; portable CI lanes exclude real-herdr-gated so the
#                   dedicated required Herdr lane owns that coverage)
#   --fail-on-gate-skip <token>
#                   after each script, fail the run if any output line contains
#                   "skip: <token>" (e.g. --fail-on-gate-skip 'herdr not found').
#                   The required Herdr CI lane uses this so a missing pin cannot
#                   silently pass as a gate skip.
#   --jobs N|auto   run through the audited resource scheduler with up to N
#                   workers. --all defaults to auto; other selections default
#                   to 1. auto uses available CPUs with a conservative cap of 4.
#                   Shared resources serialize, global overlaps nothing, and an
#                   unknown script fails closed to global.
#   --list-resources
#                   print selected path + resource declaration and exit 0
#   -h, --help      print this header
#
# Per-script machine-parseable markers (stdout):
#   MX_TEST_BEGIN <iso8601> <script> family=<family> expected_gate_skip=<class>
#   MX_TEST_END <iso8601> <script> exit=<code> duration_ms=<n> gate_skip=<true|false>
#
# After all scripts (stdout):
#   MX_TEST_SUMMARY total=<n> failed=<n> skipped_gate=<n> duration_ms=<n>
#   MX_TEST_SUMMARY_FAMILY family=<name> count=<n> duration_ms=<n> failed=<n>
#   MX_TEST_SLOWEST rank=<k> script=<path> duration_ms=<n>
#
# Exit status is non-zero if any selected script exits non-zero or a configured
# --fail-on-gate-skip token appears. Other gate skips (first meaningful line
# matching ^skip:) remain successful and are counted as skipped_gate.
#
# Family labels, the changed-file map, resource manifest, and generated portable
# shard composition live in this script only. The isolation-proof harness
# consumes this manifest; it does not own a second scheduler allowlist.
# --changed is conservative: it over-selects related families rather than
# under-selecting, and never expands to the complete suite unless --all.
set -eu

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

MODE=
LIST_ONLY=0
LIST_RESOURCES=0
LIST_FAMILIES=0
LIST_LANES=0
CHECK_COVERAGE=0
AGGREGATE_OUT=
COMPARE_JSON=0
FAMILY=
LANE=
BASE_REF=origin/main
JSON_PATH=
SCRIPTS=()
EXCLUDE_FAMILIES=()
FAIL_ON_GATE_SKIP=
JOBS=default
JOBS_MAX=8
JOBS_AUTO_MAX=4

usage() {
  awk '
    NR == 1 { next }
    /^#/ { sub(/^# ?/, ""); print; next }
    { exit }
  ' "$0" >&2
}

die() {
  printf 'mx-test-run: %s\n' "$*" >&2
  exit 2
}

log() {
  printf 'mx-test-run: %s\n' "$*" >&2
}

now_iso() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

now_ms() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import time; print(int(time.time() * 1000))'
  else
    # Second precision only when python3 is unavailable.
    echo $(($(date +%s) * 1000))
  fi
}

# Primary family for one tests/*.test.sh basename. Unmapped scripts are
# unclassified so new tests are still runnable and visible in summaries.
family_for_basename() {
  case "$1" in
    mx-arm-pretool-check.test.sh|mx-ask-user-authority.test.sh|mx-brief.test.sh|\
    mx-backlog-lib.test.sh|mx-calm-pi-extension.test.sh|mx-maintainer-translation-contract.test.sh|mx-cd-pretool-check.test.sh|\
    mx-composer-ghost.test.sh|mx-composer-lib.test.sh|\
    mx-actor-state.test.sh|mx-decision-hold-lifecycle.test.sh|\
    mx-documentation-audiences.test.sh|mx-ensure-agents-md.test.sh|mx-naming.test.sh|\
    mx-herdr-lab.test.sh|mx-instruction-owners.test.sh|\
    mx-install-herdr.test.sh|mx-nm-test-contract.test.sh|mx-no-mistakes-ownership.test.sh|\
    mx-report.test.sh|mx-report-mcp.test.sh|mx-signal-precedence.test.sh|\
    mx-removed-deps.test.sh|\
    mx-operational-input.test.sh|mx-pi-primary-types.test.sh|\
    mx-send-popup-settle.test.sh|mx-send-settle.test.sh|mx-stow-contract.test.sh|\
    mx-subagent-pretool-check.test.sh|\
    mx-supervision-instructions.test.sh|mx-tmux-submit-busy.test.sh|mx-transition-lib.test.sh|mx-vplan.test.sh|\
    mx-test-run.test.sh|mx-test-isolation-proof.test.sh|mx-test-split-parity.test.sh)
      printf '%s\n' pure-contract-unit
      ;;
    mx-daemon.test.sh|mx-guard-stale-banner.test.sh|mx-pi-watch-extension.test.sh|\
    mx-nudge.test.sh|mx-supervision-events.test.sh|mx-turnend-guard.test.sh|mx-wake-daemon-lifecycle-e2e.test.sh|\
    mx-wake-queue.test.sh|mx-watch-checkpoint.test.sh|mx-watch-triage.test.sh|\
    mx-watcher-lock.test.sh)
      printf '%s\n' watcher-wake-lock
      ;;
    mx-afk-inject-herdr-e2e.test.sh|mx-afk-launch.test.sh|mx-backend-autodetect-smoke.test.sh|\
    mx-backend-herdr-eventwait-smoke.test.sh|mx-backend-herdr-presentation-e2e.test.sh|\
    mx-backend-herdr-prune-safety-e2e.test.sh|mx-backend-herdr-respawn-idem-e2e.test.sh|\
    mx-herdr-session-cleanup-e2e.test.sh|\
    mx-backend-herdr-smoke.test.sh|mx-backend-herdr-workspace-per-home-e2e.test.sh)
      printf '%s\n' real-herdr-gated
      ;;
    mx-backlog-handoff.test.sh|mx-daemon-harness-model-resolution.test.sh|\
    mx-daemon-harness-reread-retry.test.sh|mx-daemon-harness-spawn-config.test.sh|\
    mx-daemon-lifecycle-e2e.test.sh|\
    mx-daemon-liveness.test.sh|mx-daemon-safety.test.sh|mx-daemon-sync.test.sh|\
    mx-send-daemon-marker.test.sh|mx-shared-maintainer-inheritance.test.sh)
      printf '%s\n' daemon
      ;;
    mx-bootstrap.test.sh|mx-system-sync.test.sh|mx-gate-refuse.test.sh|mx-gotmp.test.sh|\
    mx-session-start-digest-render.test.sh|mx-session-start-lock-bootstrap.test.sh|\
    mx-session-start-process-liveness.test.sh|mx-sessionstart-nudge.test.sh|mx-tangle-guard.test.sh|\
    mx-update.test.sh)
      printf '%s\n' session-bootstrap
      ;;
    mx-afk-pi-herdr-return-e2e.test.sh|\
    mx-codex-continuity-live-e2e.test.sh|mx-pi-primary-live-e2e.test.sh|\
    mx-send-daemon-marker-herdr-e2e.test.sh)
      printf '%s\n' live-harness-optin
      ;;
    mx-backend-herdr.test.sh|mx-backend-tmux-smoke.test.sh|mx-backend.test.sh|mx-dispatch-queue.test.sh|\
    mx-herdr-session-cleanup.test.sh|mx-send-strict.test.sh|mx-spawn-batch.test.sh|\
    mx-spawn-dispatch-profile.test.sh|mx-spawn-worktree-settle.test.sh|mx-headroom.test.sh)
      printf '%s\n' backend-dispatch
      ;;
    mx-pr-check-security-fault-quarantine.test.sh|\
    mx-pr-check-security-parser-entrypoints.test.sh|\
    mx-pr-check-security-publication-migration.test.sh|\
    mx-pr-check-security-retirement-teardown.test.sh|\
    mx-pr-merge.test.sh|mx-review-diff.test.sh|\
    mx-teardown.test.sh)
      printf '%s\n' pr-forge
      ;;
    mx-afk-inject-e2e.test.sh|mx-afk-return.test.sh)
      printf '%s\n' afk
      ;;
    mx-status-snapshot-catchup-forge.test.sh|mx-status-snapshot-landed-bounds.test.sh|\
    mx-status-snapshot-projection-reconciliation.test.sh|mx-system-snapshot-view.test.sh)
      printf '%s\n' snapshot-catchup
      ;;
    mx-backend-cmux.test.sh|mx-backend-cmux-smoke.test.sh)
      printf '%s\n' cmux
      ;;
    *)
      printf '%s\n' unclassified
      ;;
  esac
}

expected_gate_skip_for_family() {
  case "$1" in
    real-herdr-gated) printf '%s\n' herdr ;;
    live-harness-optin) printf '%s\n' optin-env ;;
    cmux) printf '%s\n' optional-binary ;;
    snapshot-catchup) printf '%s\n' optional-binary ;;
    *) printf '%s\n' none ;;
  esac
}

list_known_families() {
  cat <<'EOF'
pure-contract-unit
watcher-wake-lock
real-herdr-gated
daemon
session-bootstrap
live-harness-optin
backend-dispatch
pr-forge
afk
snapshot-catchup
cmux
unclassified
EOF
}

list_known_lanes() {
  cat <<'EOF'
portable-parallel-1
portable-parallel-2
portable-serial
real-herdr-gated
EOF
}

# Audited resource-conflict manifest.
# `none` means the script owns only its private worker root.
# `global` overlaps nothing. Unknown paths also resolve to global so ad-hoc
# focused fixtures remain safe without gaining concurrency authority.
list_resource_manifest() {
  cat <<'EOF'
tests/mx-actor-state.test.sh	none
tests/mx-afk-inject-e2e.test.sh	afk-watcher-process
tests/mx-afk-inject-herdr-e2e.test.sh	herdr-session
tests/mx-afk-launch.test.sh	afk-watcher-process,herdr-session
tests/mx-afk-pi-herdr-return-e2e.test.sh	herdr-session,live-harness
tests/mx-afk-return.test.sh	afk-watcher-process
tests/mx-arm-pretool-check.test.sh	none
tests/mx-ask-user-authority.test.sh	none
tests/mx-backend-autodetect-smoke.test.sh	herdr-session
tests/mx-backend-cmux-smoke.test.sh	cmux-app
tests/mx-backend-cmux.test.sh	none
tests/mx-backend-herdr-eventwait-smoke.test.sh	herdr-session
tests/mx-backend-herdr-presentation-e2e.test.sh	herdr-session
tests/mx-backend-herdr-prune-safety-e2e.test.sh	herdr-session
tests/mx-backend-herdr-respawn-idem-e2e.test.sh	herdr-session
tests/mx-backend-herdr-smoke.test.sh	herdr-session
tests/mx-backend-herdr-workspace-per-home-e2e.test.sh	herdr-session
tests/mx-backend-herdr.test.sh	none
tests/mx-backend-tmux-smoke.test.sh	tmux-server
tests/mx-backend.test.sh	none
tests/mx-backlog-handoff.test.sh	none
tests/mx-backlog-lib.test.sh	none
tests/mx-bootstrap.test.sh	bootstrap-process-signal
tests/mx-brief.test.sh	none
tests/mx-calm-pi-extension.test.sh	live-harness,tmux-server
tests/mx-cd-pretool-check.test.sh	none
tests/mx-claude-stop-autoarm-live-e2e.test.sh	live-harness
tests/mx-claude-stop-autoarm.test.sh	none
tests/mx-codex-continuity-live-e2e.test.sh	live-harness
tests/mx-composer-ghost.test.sh	none
tests/mx-composer-lib.test.sh	none
tests/mx-daemon-harness-model-resolution.test.sh	none
tests/mx-daemon-harness-reread-retry.test.sh	daemon-reread-process-signal
tests/mx-daemon-harness-spawn-config.test.sh	none
tests/mx-daemon-lifecycle-e2e.test.sh	none
tests/mx-daemon-liveness.test.sh	none
tests/mx-daemon-safety.test.sh	none
tests/mx-daemon-sync.test.sh	none
tests/mx-daemon.test.sh	daemon-process-signal
tests/mx-decision-hold-lifecycle.test.sh	none
tests/mx-dispatch-queue.test.sh	none
tests/mx-documentation-audiences.test.sh	none
tests/mx-ensure-agents-md.test.sh	none
tests/mx-gate-refuse.test.sh	none
tests/mx-gotmp.test.sh	none
tests/mx-guard-stale-banner.test.sh	watcher-process
tests/mx-herdr-lab.test.sh	none
tests/mx-headroom.test.sh	none
tests/mx-herdr-session-cleanup-e2e.test.sh	herdr-session
tests/mx-herdr-session-cleanup.test.sh	none
tests/mx-install-herdr.test.sh	none
tests/mx-instruction-owners.test.sh	none
tests/mx-maintainer-translation-contract.test.sh	none
tests/mx-naming.test.sh	none
tests/mx-nm-test-contract.test.sh	none
tests/mx-no-mistakes-ownership.test.sh	none
tests/mx-nudge.test.sh	watcher-process
tests/mx-operational-input.test.sh	none
tests/mx-pending-reply.test.sh	none
tests/mx-pi-primary-live-e2e.test.sh	live-harness
tests/mx-pi-primary-types.test.sh	none
tests/mx-pi-watch-extension.test.sh	watcher-process
tests/mx-pr-check-security-fault-quarantine.test.sh	pr-security-process
tests/mx-pr-check-security-parser-entrypoints.test.sh	none
tests/mx-pr-check-security-publication-migration.test.sh	global
tests/mx-pr-check-security-retirement-teardown.test.sh	pr-security-process
tests/mx-pr-merge.test.sh	none
tests/mx-removed-deps.test.sh	none
tests/mx-report-mcp.test.sh	none
tests/mx-report.test.sh	none
tests/mx-review-diff.test.sh	none
tests/mx-send-daemon-marker-herdr-e2e.test.sh	herdr-session,live-harness
tests/mx-send-daemon-marker.test.sh	none
tests/mx-send-popup-settle.test.sh	none
tests/mx-send-settle.test.sh	none
tests/mx-send-strict.test.sh	none
tests/mx-session-start-digest-render.test.sh	none
tests/mx-session-start-lock-bootstrap.test.sh	none
tests/mx-session-start-process-liveness.test.sh	session-process-liveness
tests/mx-sessionstart-nudge.test.sh	watcher-process
tests/mx-shared-maintainer-inheritance.test.sh	none
tests/mx-signal-precedence.test.sh	none
tests/mx-spawn-batch.test.sh	none
tests/mx-spawn-dispatch-profile.test.sh	none
tests/mx-spawn-worktree-settle.test.sh	none
tests/mx-status-snapshot-catchup-forge.test.sh	none
tests/mx-status-snapshot-landed-bounds.test.sh	none
tests/mx-status-snapshot-projection-reconciliation.test.sh	none
tests/mx-stow-contract.test.sh	none
tests/mx-subagent-pretool-check.test.sh	none
tests/mx-supervision-events.test.sh	watcher-process
tests/mx-supervision-instructions.test.sh	none
tests/mx-system-snapshot-view.test.sh	none
tests/mx-system-sync.test.sh	none
tests/mx-tangle-guard.test.sh	none
tests/mx-teardown.test.sh	none
tests/mx-test-isolation-proof.test.sh	global
tests/mx-test-run.test.sh	global
tests/mx-test-split-parity.test.sh	none
tests/mx-tmux-submit-busy.test.sh	none
tests/mx-transition-lib.test.sh	none
tests/mx-turnend-guard.test.sh	watcher-process
tests/mx-update.test.sh	none
tests/mx-vplan.test.sh	vplan-port
tests/mx-wake-daemon-lifecycle-e2e.test.sh	watcher-process
tests/mx-wake-queue.test.sh	watcher-process
tests/mx-watch-checkpoint.test.sh	watcher-process
tests/mx-watch-triage.test.sh	watcher-process
tests/mx-watcher-lock.test.sh	watcher-process
tests/no-mistakes-required-workflow.test.sh	none
EOF
}

resources_for_script() {
  local want=$1 row
  row=$(awk -F '\t' -v want="$want" '$1 == want { print $2; found=1; exit } END { if (!found) exit 1 }' \
    < <(list_resource_manifest)) || {
      printf '%s\n' global
      return 0
    }
  printf '%s\n' "$row"
}

resource_manifest_has_script() {
  local want=$1
  awk -F '\t' -v want="$want" '$1 == want { found=1 } END { exit(found ? 0 : 1) }' \
    < <(list_resource_manifest)
}

# Compatibility selection for the old flag: scripts whose audited declaration
# is `none`. The resource manifest, not this derived view, owns authority.
list_proven_isolated() {
  list_resource_manifest | awk -F '\t' '$2 == "none" { print $1 }' | LC_ALL=C sort
}

list_portable_scheduler_pool() {
  local s family resources
  while IFS= read -r s; do
    family=$(family_for_basename "$(basename "$s")")
    [ "$family" = real-herdr-gated ] && continue
    resources=$(resources_for_script "$s")
    case ",$resources," in
      *,global,*|*,live-harness,*) continue ;;
    esac
    printf '%s\n' "$s"
  done < <(all_repo_tests)
}

# Generate a deterministic two-way LPT partition from the accepted timing
# baseline. Unknown/new scripts receive a conservative one-second estimate.
list_portable_shard() {
  local shard=$1 baseline="$ROOT/docs/mx-test-performance-baseline.json" pool
  command -v python3 >/dev/null 2>&1 || die "portable shard generation requires python3"
  pool=$(mktemp "${TMPDIR:-/tmp}/mx-test-shard-pool.XXXXXX")
  list_portable_scheduler_pool >"$pool"
  python3 - "$shard" "$baseline" "$pool" <<'PY'
import json
import sys
from pathlib import Path

want = int(sys.argv[1])
baseline = Path(sys.argv[2])
pool = Path(sys.argv[3])
paths = [line.strip() for line in pool.read_text(encoding="utf-8").splitlines() if line.strip()]
estimates = {}
if baseline.exists():
    doc = json.loads(baseline.read_text(encoding="utf-8"))
    estimates.update({
        row["path"]: int(row.get("duration_ms") or 1000)
        for row in doc.get("scripts", [])
    })
    estimates.update({
        key: int(value)
        for key, value in (doc.get("scheduler_estimates_ms") or {}).items()
    })
bins = [[], []]
totals = [0, 0]
for path in sorted(paths, key=lambda p: (-estimates.get(p, 1000), p)):
    target = 0 if totals[0] <= totals[1] else 1
    bins[target].append(path)
    totals[target] += estimates.get(path, 1000)
for path in bins[want - 1]:
    print(path)
PY
  rm -f "$pool"
}

list_portable_parallel_1() {
  list_portable_shard 1
}

list_portable_parallel_2() {
  list_portable_shard 2
}

list_portable_serial() {
  local s family resources
  while IFS= read -r s; do
    family=$(family_for_basename "$(basename "$s")")
    [ "$family" = real-herdr-gated ] && continue
    resources=$(resources_for_script "$s")
    case ",$resources," in
      *,global,*|*,live-harness,*) printf '%s\n' "$s" ;;
    esac
  done < <(all_repo_tests)
}

is_proven_isolated_script() {
  local want=$1 line
  while IFS= read -r line; do
    [ "$line" = "$want" ] && return 0
  done < <(list_proven_isolated)
  return 1
}

select_proven_isolated() {
  local s
  while IFS= read -r s; do
    [ -n "$s" ] || continue
    add_script "$s"
  done < <(list_proven_isolated)
}

select_lane() {
  local want=$1 s base fam found=0
  case "$want" in
    portable-parallel-1)
      while IFS= read -r s; do
        [ -n "$s" ] || continue
        add_script "$s"
        found=1
      done < <(list_portable_parallel_1)
      ;;
    portable-parallel-2)
      while IFS= read -r s; do
        [ -n "$s" ] || continue
        add_script "$s"
        found=1
      done < <(list_portable_parallel_2)
      ;;
    portable-serial)
      while IFS= read -r s; do
        [ -n "$s" ] || continue
        add_script "$s"
        found=1
      done < <(list_portable_serial)
      ;;
    real-herdr-gated)
      select_family real-herdr-gated
      found=1
      ;;
    *)
      die "unknown lane '$want' (see --list-lanes)"
      ;;
  esac
  [ "$found" -eq 1 ] || die "lane '$want' selected no tests"
}

run_coverage_guard() {
  local tmp missing extra a b
  local -a saved_scripts=()
  tmp=$(mktemp -d "${TMPDIR:-/tmp}/mx-test-coverage.XXXXXX")

  all_repo_tests | LC_ALL=C sort -u >"$tmp/all"
  list_resource_manifest | cut -f1 | LC_ALL=C sort >"$tmp/manifest"
  uniq -d "$tmp/manifest" >"$tmp/manifest_dups"
  if [ -s "$tmp/manifest_dups" ]; then
    log "coverage guard: duplicate scripts in resource manifest:"
    cat "$tmp/manifest_dups" >&2
    rm -rf "$tmp"
    return 1
  fi
  missing=$(comm -23 "$tmp/all" "$tmp/manifest" || true)
  extra=$(comm -13 "$tmp/all" "$tmp/manifest" || true)
  if [ -n "$missing" ] || [ -n "$extra" ]; then
    log "coverage guard: resource manifest must equal tests/*.test.sh exactly"
    [ -z "$missing" ] || { log "missing resource declarations:"; printf '%s\n' "$missing" >&2; }
    [ -z "$extra" ] || { log "stale resource declarations:"; printf '%s\n' "$extra" >&2; }
    rm -rf "$tmp"
    return 1
  fi
  if ! list_resource_manifest | awk -F '\t' '
    NF != 2 || $1 !~ /^tests\/.*\.test\.sh$/ || $2 == "" { exit 1 }
    $2 == "none" || $2 == "global" { next }
    $2 ~ /(^|,)(none|global)(,|$)/ { exit 1 }
    $2 !~ /^[a-z0-9-]+(,[a-z0-9-]+)*$/ { exit 1 }
  '; then
    log "coverage guard: invalid resource manifest row"
    rm -rf "$tmp"
    return 1
  fi

  list_portable_parallel_1 | LC_ALL=C sort -u >"$tmp/s1"
  list_portable_parallel_2 | LC_ALL=C sort -u >"$tmp/s2"

  cat "$tmp/s1" "$tmp/s2" | LC_ALL=C sort | uniq -d >"$tmp/shard_dups"
  if [ -s "$tmp/shard_dups" ]; then
    log "coverage guard: portable parallel shards share scripts:"
    cat "$tmp/shard_dups" >&2
    rm -rf "$tmp"
    return 1
  fi
  cat "$tmp/s1" "$tmp/s2" | LC_ALL=C sort -u >"$tmp/shards_union"
  list_portable_scheduler_pool | LC_ALL=C sort -u >"$tmp/scheduler_pool"
  missing=$(comm -23 "$tmp/scheduler_pool" "$tmp/shards_union" || true)
  extra=$(comm -13 "$tmp/scheduler_pool" "$tmp/shards_union" || true)
  if [ -n "$missing" ] || [ -n "$extra" ]; then
    log "coverage guard: portable shards must equal the generated scheduler pool"
    [ -z "$missing" ] || { log "missing from shards:"; printf '%s\n' "$missing" >&2; }
    [ -z "$extra" ] || { log "extra beyond scheduler pool:"; printf '%s\n' "$extra" >&2; }
    rm -rf "$tmp"
    return 1
  fi

  # Serial + Herdr lane listings without disturbing a caller's selection.
  saved_scripts=("${SCRIPTS[@]+"${SCRIPTS[@]}"}")
  SCRIPTS=()
  select_lane portable-serial
  printf '%s\n' "${SCRIPTS[@]+"${SCRIPTS[@]}"}" | LC_ALL=C sort -u >"$tmp/serial"
  SCRIPTS=()
  select_family real-herdr-gated
  printf '%s\n' "${SCRIPTS[@]+"${SCRIPTS[@]}"}" | LC_ALL=C sort -u >"$tmp/herdr"
  SCRIPTS=("${saved_scripts[@]+"${saved_scripts[@]}"}")

  for pair in "shards_union:serial" "shards_union:herdr" "serial:herdr"; do
    a=${pair%%:*}
    b=${pair#*:}
    comm -12 "$tmp/$a" "$tmp/$b" >"$tmp/overlap"
    if [ -s "$tmp/overlap" ]; then
      log "coverage guard: overlap between $a and $b:"
      cat "$tmp/overlap" >&2
      rm -rf "$tmp"
      return 1
    fi
  done

  cat "$tmp/shards_union" "$tmp/serial" "$tmp/herdr" | LC_ALL=C sort >"$tmp/union_raw"
  uniq -d "$tmp/union_raw" >"$tmp/union_dups"
  if [ -s "$tmp/union_dups" ]; then
    log "coverage guard: duplicate scripts across lanes:"
    cat "$tmp/union_dups" >&2
    rm -rf "$tmp"
    return 1
  fi
  LC_ALL=C sort -u "$tmp/union_raw" >"$tmp/union"
  missing=$(comm -23 "$tmp/all" "$tmp/union" || true)
  extra=$(comm -13 "$tmp/all" "$tmp/union" || true)
  if [ -n "$missing" ] || [ -n "$extra" ]; then
    log "coverage guard: union of portable shards + portable serial + Herdr must equal tests/*.test.sh"
    [ -z "$missing" ] || { log "missing from union:"; printf '%s\n' "$missing" >&2; }
    [ -z "$extra" ] || { log "extra beyond inventory:"; printf '%s\n' "$extra" >&2; }
    rm -rf "$tmp"
    return 1
  fi

  printf 'MX_TEST_COVERAGE ok total=%s accelerated=%s serial=%s herdr=%s manifest=%s\n' \
    "$(wc -l <"$tmp/all" | tr -d ' ')" \
    "$(wc -l <"$tmp/shards_union" | tr -d ' ')" \
    "$(wc -l <"$tmp/serial" | tr -d ' ')" \
    "$(wc -l <"$tmp/herdr" | tr -d ' ')" \
    "$(wc -l <"$tmp/manifest" | tr -d ' ')"
  rm -rf "$tmp"
  return 0
}

aggregate_timing_json() {
  local out=$1
  shift
  [ "$#" -gt 0 ] || die "--aggregate-json requires at least one input timing JSON"
  command -v python3 >/dev/null 2>&1 || die "--aggregate-json requires python3"
  python3 - "$out" "$@" <<'PY'
import json, sys
from pathlib import Path

out = Path(sys.argv[1])
inputs = [Path(p) for p in sys.argv[2:]]
lanes = []
all_scripts = []
failed = 0
skipped = 0
total = 0
wall_ms = 0
for path in inputs:
    doc = json.loads(path.read_text(encoding="utf-8"))
    summary = doc.get("summary") or {}
    lane = {
        "path": str(path),
        "run_id": doc.get("run_id"),
        "selection": doc.get("selection"),
        "started_at": doc.get("started_at"),
        "finished_at": doc.get("finished_at"),
        "summary": summary,
    }
    lanes.append(lane)
    total += int(summary.get("total") or 0)
    failed += int(summary.get("failed") or 0)
    skipped += int(summary.get("skipped_gate") or 0)
    wall_ms = max(wall_ms, int(summary.get("duration_ms") or 0))
    for s in doc.get("scripts") or []:
        row = dict(s)
        row["lane_selection"] = doc.get("selection")
        row["lane_run_id"] = doc.get("run_id")
        all_scripts.append(row)

all_scripts.sort(key=lambda s: (-int(s.get("duration_ms") or 0), s.get("path") or ""))
agg = {
    "kind": "aggregate",
    "lanes": lanes,
    "summary": {
        "lanes": len(lanes),
        "total": total,
        "failed": failed,
        "skipped_gate": skipped,
        "critical_path_duration_ms": wall_ms,
    },
    "scripts": all_scripts,
    "slowest": all_scripts[:15],
}

out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(agg, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"MX_TEST_AGGREGATE lanes={len(lanes)} total={total} failed={failed} skipped_gate={skipped} critical_path_duration_ms={wall_ms}")
PY
}

compare_timing_json() {
  local serial=$1 accelerated=$2
  command -v python3 >/dev/null 2>&1 || die "--compare-json requires python3"
  python3 - "$serial" "$accelerated" <<'PY'
import collections
import json
import sys

serial_path, accelerated_path = sys.argv[1:3]
serial = json.load(open(serial_path, encoding="utf-8"))
accelerated = json.load(open(accelerated_path, encoding="utf-8"))

def normalized(doc):
    rows = {}
    for row in doc.get("scripts", []):
        rows[row["path"]] = {
            "exit": int(row.get("exit") or 0),
            "gate_skip": bool(row.get("gate_skip")),
            "assertions": sorted(collections.Counter(row.get("assertions") or []).items()),
        }
    return rows

left = normalized(serial)
right = normalized(accelerated)
errors = []
if set(left) != set(right):
    errors.append(f"inventory differs: serial_only={sorted(set(left)-set(right))} accelerated_only={sorted(set(right)-set(left))}")
for path in sorted(set(left) & set(right)):
    if left[path] != right[path]:
        errors.append(f"{path}: serial={left[path]!r} accelerated={right[path]!r}")
for field in ("failed", "skipped_gate"):
    lval = int((serial.get("summary") or {}).get(field) or 0)
    rval = int((accelerated.get("summary") or {}).get(field) or 0)
    if lval != rval:
        errors.append(f"summary.{field}: serial={lval} accelerated={rval}")
if errors:
    print("MX_TEST_PARITY mismatch")
    for error in errors:
        print(error)
    raise SystemExit(1)
assertion_count = sum(len(row.get("assertions") or []) for row in serial.get("scripts", []))
print(f"MX_TEST_PARITY ok scripts={len(left)} assertions={assertion_count}")
PY
}

all_repo_tests() {
  # Deterministic lexical order (same as bash glob expansion under LC_ALL=C).
  local f
  # shellcheck disable=SC2035
  for f in tests/*.test.sh; do
    [ -f "$f" ] || continue
    printf '%s\n' "$f"
  done | LC_ALL=C sort
}

normalize_script_path() {
  local p=$1
  case "$p" in
    /*) printf '%s\n' "$p" ;;
    tests/*|./tests/*)
      p=${p#./}
      printf '%s\n' "$p"
      ;;
    *.test.sh)
      if [ -f "tests/$p" ]; then
        printf 'tests/%s\n' "$p"
      else
        printf '%s\n' "$p"
      fi
      ;;
    *)
      printf '%s\n' "$p"
      ;;
  esac
}

# Append unique relative-or-absolute script paths to SCRIPTS.
add_script() {
  local p existing
  p=$(normalize_script_path "$1")
  for existing in "${SCRIPTS[@]+"${SCRIPTS[@]}"}"; do
    [ "$existing" = "$p" ] && return 0
  done
  SCRIPTS+=("$p")
}

select_all() {
  local s
  while IFS= read -r s; do
    [ -n "$s" ] || continue
    add_script "$s"
  done < <(all_repo_tests)
}

select_family() {
  local want=$1 s base fam found=0
  [ -n "$want" ] || die "--family requires a name"
  while IFS= read -r s; do
    [ -n "$s" ] || continue
    base=$(basename "$s")
    fam=$(family_for_basename "$base")
    if [ "$fam" = "$want" ]; then
      add_script "$s"
      found=1
    fi
  done < <(all_repo_tests)
  [ "$found" -eq 1 ] || die "no tests mapped to family '$want'"
}

families_for_test_reference() {
  local needle=$1 s
  local found=0
  while IFS= read -r s; do
    [ -n "$s" ] || continue
    if grep -Fq "$needle" "$s"; then
      family_for_basename "$(basename "$s")"
      found=1
    fi
  done < <(all_repo_tests)
  [ "$found" -eq 1 ]
}

families_for_shared_test_helper() {
  local needle=$1 helper found=0
  if families_for_test_reference "$needle"; then
    found=1
  fi
  for helper in tests/*-helpers.sh; do
    [ -f "$helper" ] || continue
    grep -Fq "$needle" "$helper" || continue
    if families_for_test_reference "$(basename "$helper")"; then
      found=1
    fi
  done
  [ "$found" -eq 1 ]
}

# Conservative path → family map. Over-selects rather than under-selects.
# Never expands to the complete suite.
families_for_changed_path() {
  local path=$1
  case "$path" in
    tests/mx-test-run.test.sh)
      printf '%s\n' pure-contract-unit
      ;;
    tests/mx-backend-herdr-eventwait.test.py)
      printf '%s\n' real-herdr-gated
      printf '%s\n' backend-dispatch
      ;;
    tests/*.test.sh)
      # A single test file change selects only that script via basename family
      # resolution in the caller; emit a marker family of __script__
      printf '%s\n' "__script__:$(basename "$path")"
      ;;
    bin/mx-test-run.sh|bin/mx-test-isolation-proof.sh)
      printf '%s\n' pure-contract-unit
      ;;
    bin/backends/herdr*|bin/mx-herdr-lab.sh|tests/herdr-test-safety.sh)
      printf '%s\n' real-herdr-gated
      printf '%s\n' backend-dispatch
      printf '%s\n' pure-contract-unit
      ;;
    bin/mx-herdr-session-cleanup.sh)
      printf '%s\n' session-bootstrap
      printf '%s\n' real-herdr-gated
      printf '%s\n' backend-dispatch
      ;;
    bin/backends/cmux*|tests/cmux-test-safety.sh)
      printf '%s\n' cmux
      printf '%s\n' backend-dispatch
      ;;
    bin/backends/tmux.sh)
      printf '%s\n' backend-dispatch
      ;;
    bin/mx-backend.sh|bin/mx-backend-hometag-lib.sh)
      printf '%s\n' backend-dispatch
      printf '%s\n' real-herdr-gated
      ;;
    bin/mx-headroom.sh)
      printf '%s\n' backend-dispatch
      printf '%s\n' watcher-wake-lock
      ;;
    bin/mx-vplan.sh|bin/mx-vplan-server.mjs|share/vplan/*|docs/vplan.md|docs/vplan-authoring.md)
      printf '%s\n' pure-contract-unit
      ;;
    bin/mx-watch*|bin/mx-wake*|\
    bin/mx-classify-lib.sh|bin/mx-daemon*|bin/mx-turnend-guard*|bin/mx-guard.sh)
      printf '%s\n' watcher-wake-lock
      ;;
    bin/mx-report)
      printf '%s\n' pure-contract-unit
      printf '%s\n' watcher-wake-lock
      ;;
    bin/mx-afk*)
      printf '%s\n' afk
      printf '%s\n' real-herdr-gated
      ;;
    bin/mx-supervisor-target-lib.sh)
      printf '%s\n' watcher-wake-lock
      printf '%s\n' real-herdr-gated
      printf '%s\n' live-harness-optin
      printf '%s\n' afk
      ;;
    bin/mx-daemon*|bin/mx-home-seed.sh|bin/mx-backlog-handoff.sh|bin/mx-backlog.sh|\
    bin/mx-config-inherit-lib.sh|bin/mx-config-push.sh|bin/mx-shared*)
      printf '%s\n' daemon
      ;;
    bin/mx-session-start.sh|bin/mx-bootstrap.sh|bin/mx-system-sync.sh|\
    bin/mx-sessionstart-nudge.sh|bin/mx-tangle*|bin/mx-update.sh|\
    bin/mx-gate-refuse*|bin/mx-lock*)
      printf '%s\n' session-bootstrap
      ;;
    bin/mx-pr-*|bin/mx-merge-local.sh|bin/mx-teardown.sh|bin/mx-review-diff.sh|\
    bin/mx-check*)
      printf '%s\n' pr-forge
      ;;
    bin/mx-spawn.sh|bin/mx-send.sh|bin/mx-harness.sh|\
    bin/mx-peek.sh|bin/mx-composer*)
      printf '%s\n' backend-dispatch
      printf '%s\n' pure-contract-unit
      ;;
    bin/mx-status-snapshot.sh|bin/mx-system-snapshot.sh|bin/mx-system-view.sh)
      printf '%s\n' snapshot-catchup
      ;;
    bin/mx-install-herdr.sh|bin/mx-install-treehouse.sh|bin/mx-herdr-ci-cleanup.sh)
      printf '%s\n' pure-contract-unit
      # Pin or cleanup changes also select the real-Herdr family so the required
      # lane's contract coverage re-runs.
      printf '%s\n' real-herdr-gated
      ;;
    bin/mx-brief.sh|bin/mx-report-mcp.mjs|\
    bin/mx-ensure-agents-md.sh|bin/mx-actor-state.sh|\
    bin/mx-decision-hold.sh|bin/mx-supervision*|bin/mx-transition-lib.sh|\
    bin/mx-tmux-lib.sh|bin/mx-marker-lib.sh|bin/mx-operational-input.sh|bin/mx-backlog-lib.sh|\
    bin/mx-primary-scope-lib.sh|bin/mx-project-mode.sh|bin/mx-promote.sh|\
    bin/mx-ff-lib.sh|bin/mx-gotmp*|bin/*pretool*)
      printf '%s\n' pure-contract-unit
      ;;
    .agents/skills/*/SKILL.md)
      printf '%s\n' pure-contract-unit
      ;;
    .github/workflows/ci.yml|.no-mistakes.yaml)
      printf '%s\n' pure-contract-unit
      printf '%s\n' real-herdr-gated
      ;;
    docs/mx-test-portable-shards.md|docs/mx-test-isolation-proof.md|\
    docs/mx-test-isolation-proof.json)
      printf '%s\n' pure-contract-unit
      ;;
    .github/*|AGENTS.md|CLAUDE.md|CONTRIBUTING.md|example_agents.md|\
    docs/configuration.md|docs/supervision-protocols/*)
      printf '%s\n' pure-contract-unit
      ;;
    plans/*)
      printf '%s\n' pure-contract-unit
      ;;
    tests/lib.sh)
      families_for_shared_test_helper "$(basename "$path")" \
        || printf '%s\n' "__unmapped__:$path"
      ;;
    tests/*-helpers.sh)
      families_for_test_reference "$(basename "$path")" \
        || printf '%s\n' "__unmapped__:$path"
      ;;
    bin/*)
      families_for_test_reference "$(basename "$path")" \
        || printf '%s\n' "__unmapped__:$path"
      ;;
    tests/*)
      printf '%s\n' "__unmapped__:$path"
      ;;
    README.md|LICENSE|assets/*|docs/*|.gitignore)
      ;;
    *)
      families_for_test_reference "$path" \
        || printf '%s\n' "__unmapped__:$path"
      ;;
  esac
}

select_changed() {
  local base=$1 path entry fam script_name s
  local -a wanted_families=()
  local -a wanted_scripts=()

  if ! git -C "$ROOT" rev-parse --verify "$base" >/dev/null 2>&1; then
    die "changed-file base ref not found: $base (pass --base <ref>)"
  fi

  while IFS= read -r path; do
    [ -n "$path" ] || continue
    while IFS= read -r entry; do
      [ -n "$entry" ] || continue
      case "$entry" in
        __script__:*)
          script_name=${entry#__script__:}
          wanted_scripts+=("$script_name")
          ;;
        __unmapped__:*)
          die "no changed-test mapping for source path: ${entry#__unmapped__:}"
          ;;
        *)
          wanted_families+=("$entry")
          ;;
      esac
    done < <(families_for_changed_path "$path")
  done < <(git -C "$ROOT" diff --name-only "${base}...HEAD" 2>/dev/null; \
           git -C "$ROOT" diff --name-only HEAD 2>/dev/null; \
           git -C "$ROOT" ls-files --others --exclude-standard 2>/dev/null)

  # Dedup families
  local f seen_f
  local -a unique_families=()
  for f in "${wanted_families[@]+"${wanted_families[@]}"}"; do
    seen_f=0
    for u in "${unique_families[@]+"${unique_families[@]}"}"; do
      [ "$u" = "$f" ] && { seen_f=1; break; }
    done
    [ "$seen_f" -eq 0 ] && unique_families+=("$f")
  done

  for f in "${unique_families[@]+"${unique_families[@]}"}"; do
    while IFS= read -r s; do
      [ -n "$s" ] || continue
      if [ "$(family_for_basename "$(basename "$s")")" = "$f" ]; then
        add_script "$s"
      fi
    done < <(all_repo_tests)
  done

  for script_name in "${wanted_scripts[@]+"${wanted_scripts[@]}"}"; do
    if [ -f "tests/$script_name" ]; then
      add_script "tests/$script_name"
    fi
  done

  if [ "${#SCRIPTS[@]}" -eq 0 ]; then
    log "no tests selected for changes vs $base (map is conservative; use --all for the complete suite)"
  fi
}

detect_gate_skip() {
  # True when the first non-empty output line is a skip: gate message.
  local file=$1 first
  first=$(awk 'NF { print; exit }' "$file" 2>/dev/null || true)
  case "$first" in
    skip:*) return 0 ;;
    *) return 1 ;;
  esac
}

# True when any output line contains "skip: <token>" (token may contain spaces).
detect_gate_skip_token() {
  local file=$1 token=$2
  [ -n "$token" ] || return 1
  grep -F -q "skip: $token" "$file" 2>/dev/null
}

apply_exclude_families() {
  local s fam keep ex
  local -a kept=()
  [ "${#EXCLUDE_FAMILIES[@]}" -gt 0 ] || return 0
  for s in "${SCRIPTS[@]+"${SCRIPTS[@]}"}"; do
    fam=$(family_for_basename "$(basename "$s")")
    keep=1
    for ex in "${EXCLUDE_FAMILIES[@]}"; do
      if [ "$fam" = "$ex" ]; then
        keep=0
        break
      fi
    done
    [ "$keep" -eq 1 ] && kept+=("$s")
  done
  SCRIPTS=("${kept[@]+"${kept[@]}"}")
}

write_json_artifact() {
  local out=$1
  local started=$2
  local finished=$3
  local run_id=$4
  local total=$5
  local failed=$6
  local skipped=$7
  local duration=$8
  local selection=$9
  local records_file=${10}
  local families_file=${11}
  local jobs=${12}

  if ! command -v python3 >/dev/null 2>&1; then
    die "--json requires python3 to emit a valid timing artifact"
  fi

  python3 - "$out" "$started" "$finished" "$run_id" "$total" "$failed" "$skipped" "$duration" "$selection" "$records_file" "$families_file" "$jobs" <<'PY'
import json, sys

out, started, finished, run_id, total, failed, skipped, duration, selection, records_file, families_file, jobs = sys.argv[1:]

scripts = []
with open(records_file, encoding="utf-8") as fh:
    for line in fh:
        line = line.rstrip("\n")
        if not line:
            continue
        path, family, expected, resources, exit_s, dur_s, gate, output_path = line.split("\t")
        assertions = []
        try:
            with open(output_path, encoding="utf-8", errors="replace") as output:
                for output_line in output:
                    text = output_line.rstrip("\n")
                    if text.startswith("ok - ") or text.startswith("not ok - "):
                        assertions.append(text)
        except FileNotFoundError:
            pass
        scripts.append({
            "path": path,
            "family": family,
            "expected_gate_skip": expected,
            "resources": resources.split(",") if resources not in ("none", "global") else [resources],
            "duration_ms": int(dur_s),
            "exit": int(exit_s),
            "gate_skip": gate == "true",
            "assertions": assertions,
        })

families = []
with open(families_file, encoding="utf-8") as fh:
    for line in fh:
        line = line.rstrip("\n")
        if not line:
            continue
        name, count_s, dur_s, failed_s = line.split("\t")
        families.append({
            "name": name,
            "count": int(count_s),
            "duration_ms": int(dur_s),
            "failed": int(failed_s),
        })

doc = {
    "run_id": run_id,
    "started_at": started,
    "finished_at": finished,
    "selection": selection,
    "scheduler": {
        "jobs": int(jobs),
        "resource_aware": int(jobs) > 1,
    },
    "summary": {
        "total": int(total),
        "failed": int(failed),
        "skipped_gate": int(skipped),
        "duration_ms": int(duration),
    },
    "scripts": scripts,
    "families": families,
}
with open(out, "w", encoding="utf-8") as fh:
    json.dump(doc, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --all)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      MODE=all
      shift
      ;;
    --family)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      [ "$#" -gt 1 ] || die "--family requires a name"
      MODE=family
      FAMILY=$2
      shift 2
      ;;
    --family=*)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      MODE=family
      FAMILY=${1#--family=}
      shift
      ;;
    --lane)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      [ "$#" -gt 1 ] || die "--lane requires a name (see --list-lanes)"
      MODE=lane
      LANE=$2
      shift 2
      ;;
    --lane=*)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      MODE=lane
      LANE=${1#--lane=}
      shift
      ;;
    --proven-isolated)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      MODE=proven-isolated
      shift
      ;;
    --changed)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      MODE=changed
      shift
      ;;
    --base)
      [ "$#" -gt 1 ] || die "--base requires a git ref"
      BASE_REF=$2
      shift 2
      ;;
    --base=*)
      BASE_REF=${1#--base=}
      shift
      ;;
    --json)
      [ "$#" -gt 1 ] || die "--json requires a path"
      JSON_PATH=$2
      shift 2
      ;;
    --json=*)
      JSON_PATH=${1#--json=}
      shift
      ;;
    --jobs)
      [ "$#" -gt 1 ] || die "--jobs requires a positive integer"
      JOBS=$2
      shift 2
      ;;
    --jobs=*)
      JOBS=${1#--jobs=}
      shift
      ;;
    --list)
      LIST_ONLY=1
      shift
      ;;
    --list-resources)
      LIST_RESOURCES=1
      shift
      ;;
    --list-families)
      LIST_FAMILIES=1
      shift
      ;;
    --list-lanes)
      LIST_LANES=1
      shift
      ;;
    --check-coverage)
      CHECK_COVERAGE=1
      shift
      ;;
    --aggregate-json)
      [ "$#" -gt 1 ] || die "--aggregate-json requires an output path"
      AGGREGATE_OUT=$2
      shift 2
      # Remaining args after options will be collected as inputs below via MODE.
      # For aggregation we accept only input JSON paths as free args after this.
      MODE=aggregate
      ;;
    --compare-json)
      [ -z "$MODE" ] || die "only one selection mode is allowed"
      MODE=compare
      COMPARE_JSON=1
      shift
      ;;
    --exclude-family)
      [ "$#" -gt 1 ] || die "--exclude-family requires a name"
      EXCLUDE_FAMILIES+=("$2")
      shift 2
      ;;
    --exclude-family=*)
      EXCLUDE_FAMILIES+=("${1#--exclude-family=}")
      shift
      ;;
    --fail-on-gate-skip)
      [ "$#" -gt 1 ] || die "--fail-on-gate-skip requires a token (e.g. 'herdr not found')"
      FAIL_ON_GATE_SKIP=$2
      shift 2
      ;;
    --fail-on-gate-skip=*)
      FAIL_ON_GATE_SKIP=${1#--fail-on-gate-skip=}
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      while [ "$#" -gt 0 ]; do
        SCRIPTS+=("$1")
        shift
      done
      ;;
    -*)
      die "unknown option: $1"
      ;;
    *)
      if [ "${MODE:-}" = "aggregate" ] || [ "${MODE:-}" = compare ]; then
        SCRIPTS+=("$1")
      elif [ -z "$MODE" ] || [ "$MODE" = scripts ]; then
        MODE=scripts
        SCRIPTS+=("$1")
      else
        die "script paths cannot be combined with --$MODE"
      fi
      shift
      ;;
  esac
done

if [ "$LIST_FAMILIES" -eq 1 ]; then
  list_known_families
  exit 0
fi

if [ "$LIST_LANES" -eq 1 ]; then
  list_known_lanes
  exit 0
fi

if [ "$CHECK_COVERAGE" -eq 1 ]; then
  run_coverage_guard
  exit $?
fi

if [ "${MODE:-}" = "aggregate" ]; then
  [ -n "$AGGREGATE_OUT" ] || die "--aggregate-json requires an output path"
  [ "${#SCRIPTS[@]}" -gt 0 ] || die "--aggregate-json requires at least one input timing JSON"
  for s in "${SCRIPTS[@]}"; do
    [ -f "$s" ] || die "aggregate input not found: $s"
  done
  aggregate_timing_json "$AGGREGATE_OUT" "${SCRIPTS[@]}"
  exit 0
fi

if [ "${MODE:-}" = compare ]; then
  [ "$COMPARE_JSON" -eq 1 ] || die "--compare-json mode was not initialized"
  [ "${#SCRIPTS[@]}" -eq 2 ] || die "--compare-json requires exactly two timing JSON paths"
  for s in "${SCRIPTS[@]}"; do
    [ -f "$s" ] || die "compare input not found: $s"
  done
  compare_timing_json "${SCRIPTS[0]}" "${SCRIPTS[1]}"
  exit $?
fi

case "${MODE:-}" in
  all)
    select_all
    SELECTION_DESC="all"
    ;;
  family)
    select_family "$FAMILY"
    SELECTION_DESC="family=$FAMILY"
    ;;
  lane)
    select_lane "$LANE"
    SELECTION_DESC="lane=$LANE"
    ;;
  proven-isolated)
    select_proven_isolated
    SELECTION_DESC="proven-isolated"
    ;;
  changed)
    select_changed "$BASE_REF"
    SELECTION_DESC="changed:base=$BASE_REF"
    ;;
  scripts)
    # Normalize and re-add through add_script for consistent paths.
    raw=("${SCRIPTS[@]}")
    SCRIPTS=()
    for s in "${raw[@]}"; do
      add_script "$s"
    done
    SELECTION_DESC="scripts"
    ;;
  *)
    die "select with --all, --family <name>, --lane <name>, --proven-isolated, --changed, or one or more script paths (see --help)"
    ;;
esac

detect_auto_jobs() {
  local cpus=
  if command -v getconf >/dev/null 2>&1; then
    cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)
  fi
  if [ -z "$cpus" ] && command -v sysctl >/dev/null 2>&1; then
    cpus=$(sysctl -n hw.ncpu 2>/dev/null || true)
  fi
  case "$cpus" in ''|*[!0-9]*) cpus=2 ;; esac
  [ "$cpus" -ge 1 ] || cpus=1
  [ "$cpus" -le "$JOBS_AUTO_MAX" ] || cpus=$JOBS_AUTO_MAX
  printf '%s\n' "$cpus"
}

if [ "$JOBS" = default ]; then
  if [ "${MODE:-}" = all ]; then
    JOBS=auto
  else
    JOBS=1
  fi
fi
if [ "$JOBS" = auto ]; then
  JOBS=$(detect_auto_jobs)
else
  case "$JOBS" in
    ''|*[!0-9]*) die "--jobs must be a positive integer or auto" ;;
  esac
  [ "$JOBS" -ge 1 ] || die "--jobs must be >= 1"
  [ "$JOBS" -le "$JOBS_MAX" ] || die "--jobs is capped at $JOBS_MAX (got $JOBS)"
fi

apply_exclude_families
if [ "${#EXCLUDE_FAMILIES[@]}" -gt 0 ]; then
  SELECTION_DESC="${SELECTION_DESC};exclude-family=$(IFS=,; printf '%s' "${EXCLUDE_FAMILIES[*]}")"
fi
if [ -n "$FAIL_ON_GATE_SKIP" ]; then
  SELECTION_DESC="${SELECTION_DESC};fail-on-gate-skip=$FAIL_ON_GATE_SKIP"
fi
if [ "$JOBS" -gt 1 ]; then
  SELECTION_DESC="${SELECTION_DESC};jobs=$JOBS"
fi

if [ "$LIST_RESOURCES" -eq 1 ]; then
  for s in "${SCRIPTS[@]+"${SCRIPTS[@]}"}"; do
    printf '%s\t%s\n' "$s" "$(resources_for_script "$s")"
  done
  exit 0
fi

if [ "$LIST_ONLY" -eq 1 ]; then
  for s in "${SCRIPTS[@]+"${SCRIPTS[@]}"}"; do
    printf '%s\n' "$s"
  done
  exit 0
fi

if [ "${#SCRIPTS[@]}" -eq 0 ]; then
  log "nothing to run"
  printf 'MX_TEST_SUMMARY total=0 failed=0 skipped_gate=0 duration_ms=0\n'
  if [ -n "$JSON_PATH" ]; then
    empty_rec=$(mktemp)
    empty_fam=$(mktemp)
    : >"$empty_rec"
    : >"$empty_fam"
    started=$(now_iso)
    mkdir -p "$(dirname "$JSON_PATH")"
    write_json_artifact "$JSON_PATH" "$started" "$started" "empty" 0 0 0 0 "$SELECTION_DESC" "$empty_rec" "$empty_fam" "$JOBS"
    rm -f "$empty_rec" "$empty_fam"
  fi
  exit 0
fi

# Verify selected scripts exist before starting.
for s in "${SCRIPTS[@]}"; do
  [ -f "$s" ] || die "test script not found: $s"
  [ -x "$s" ] || [ -r "$s" ] || die "test script not readable: $s"
done

RUN_TMP=$(mktemp -d "${TMPDIR:-/tmp}/mx-test-run.XXXXXX")
RECORDS="$RUN_TMP/records.tsv"
FAMILIES_TSV="$RUN_TMP/families.tsv"
WORKER_PIDS=()
: >"$RECORDS"

mx_runner_kill_tree() {
  local parent=$1 child
  while IFS= read -r child; do
    [ -n "$child" ] || continue
    mx_runner_kill_tree "$child"
  done < <(ps -axo pid=,ppid= 2>/dev/null \
    | awk -v parent="$parent" '$2 == parent { print $1 }')
  kill "$parent" 2>/dev/null || true
}

cleanup_runner() {
  local pid
  trap - EXIT INT TERM
  for pid in "${WORKER_PIDS[@]+"${WORKER_PIDS[@]}"}"; do
    [ -n "${pid:-}" ] || continue
    kill -0 "$pid" 2>/dev/null || continue
    mx_runner_kill_tree "$pid"
  done
  for pid in "${WORKER_PIDS[@]+"${WORKER_PIDS[@]}"}"; do
    [ -n "${pid:-}" ] || continue
    wait "$pid" 2>/dev/null || true
  done
  chmod -R u+w "$RUN_TMP" 2>/dev/null || true
  rm -rf "$RUN_TMP"
}

trap cleanup_runner EXIT
trap 'cleanup_runner; exit 130' INT TERM

RUN_STARTED_ISO=$(now_iso)
RUN_STARTED_MS=$(now_ms)
RUN_ID="mx-test-run-${RUN_STARTED_MS}-$$"
TOTAL=0
FAILED=0
SKIPPED_GATE=0
AGG_RC=0

# Family accumulators as TSV lines updated in-memory via temp files.
# family -> count, duration_ms, failed
family_bump() {
  local fam=$1 dur=$2 failed_delta=$3
  local line name count duration failed_count rest
  local found=0
  local tmp="$RUN_TMP/families.new"
  : >"$tmp"
  if [ -s "$FAMILIES_TSV" ]; then
    while IFS= read -r line; do
      name=${line%%$'\t'*}
      rest=${line#*$'\t'}
      count=${rest%%$'\t'*}
      rest=${rest#*$'\t'}
      duration=${rest%%$'\t'*}
      failed_count=${rest#*$'\t'}
      if [ "$name" = "$fam" ]; then
        count=$((count + 1))
        duration=$((duration + dur))
        failed_count=$((failed_count + failed_delta))
        found=1
      fi
      printf '%s\t%s\t%s\t%s\n' "$name" "$count" "$duration" "$failed_count" >>"$tmp"
    done <"$FAMILIES_TSV"
  fi
  if [ "$found" -eq 0 ]; then
    printf '%s\t%s\t%s\t%s\n' "$fam" 1 "$dur" "$failed_delta" >>"$tmp"
  fi
  mv "$tmp" "$FAMILIES_TSV"
}

record_script_result() {
  local script=$1 rc=$2 duration=$3 out=$4 end_iso=$5
  local base family expected resources gate_skip fail_delta
  base=$(basename "$script")
  family=$(family_for_basename "$base")
  expected=$(expected_gate_skip_for_family "$family")
  resources=$(resources_for_script "$script")

  if [ -n "$FAIL_ON_GATE_SKIP" ] && detect_gate_skip_token "$out" "$FAIL_ON_GATE_SKIP"; then
    log "required gate skip token seen in $script: skip: $FAIL_ON_GATE_SKIP"
    rc=1
  fi

  gate_skip=false
  if [ "$rc" -eq 0 ] && detect_gate_skip "$out"; then
    gate_skip=true
    SKIPPED_GATE=$((SKIPPED_GATE + 1))
  fi

  printf 'MX_TEST_END %s %s exit=%s duration_ms=%s gate_skip=%s\n' \
    "$end_iso" "$script" "$rc" "$duration" "$gate_skip"

  fail_delta=0
  if [ "$rc" -ne 0 ]; then
    FAILED=$((FAILED + 1))
    fail_delta=1
    AGG_RC=1
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$script" "$family" "$expected" "$resources" "$rc" "$duration" "$gate_skip" "$out" >>"$RECORDS"
  family_bump "$family" "$duration" "$fail_delta"
  TOTAL=$((TOTAL + 1))
}

run_one_serial() {
  local script=$1
  local base family expected out begin_iso begin_ms end_ms end_iso duration rc
  base=$(basename "$script")
  family=$(family_for_basename "$base")
  expected=$(expected_gate_skip_for_family "$family")
  out="$RUN_TMP/out.$TOTAL"
  begin_iso=$(now_iso)
  begin_ms=$(now_ms)

  printf 'MX_TEST_BEGIN %s %s family=%s expected_gate_skip=%s\n' \
    "$begin_iso" "$script" "$family" "$expected"

  set +e
  # Stream live output while retaining a copy for gate-skip detection.
  # PIPESTATUS[0] is the test script; tee's exit is ignored for aggregate.
  bash "$script" 2>&1 | tee "$out"
  rc=${PIPESTATUS[0]}
  set -e
  : "${rc:=1}"

  end_ms=$(now_ms)
  end_iso=$(now_iso)
  duration=$((end_ms - begin_ms))
  if [ "$duration" -lt 0 ]; then
    duration=0
  fi
  record_script_result "$script" "$rc" "$duration" "$out" "$end_iso"
}

if [ "$JOBS" -eq 1 ]; then
  for script in "${SCRIPTS[@]}"; do
    run_one_serial "$script"
  done
else
  # Resource-aware bounded execution. Every worker gets a private mode-0700
  # TMPDIR. Results are buffered and replayed in selection order, independent
  # of completion order. Retries are never used as a green strategy.
  declare -a WORKER_STATE=()
  declare -a WORKER_RESOURCES=()
  declare -a WORKER_RC=()
  declare -a WORKER_DURATION=()
  declare -a WORKER_ESTIMATE=()
  declare -a WORKER_BEGIN_ISO=()
  declare -a WORKER_END_ISO=()
  declare -a WORKER_DIRS=()
  active_workers=0
  completed_workers=0
  script_count=${#SCRIPTS[@]}

  resources_conflict() {
    local left=$1 right=$2 l r old_ifs
    [ "$left" = global ] && return 0
    [ "$right" = global ] && return 0
    [ "$left" = none ] && return 1
    [ "$right" = none ] && return 1
    old_ifs=$IFS
    IFS=,
    for l in $left; do
      for r in $right; do
        if [ "$l" = "$r" ]; then
          IFS=$old_ifs
          return 0
        fi
      done
    done
    IFS=$old_ifs
    return 1
  }

  can_launch_worker() {
    local candidate=$1 running
    for running in "${!WORKER_STATE[@]}"; do
      [ "${WORKER_STATE[$running]}" = running ] || continue
      if resources_conflict "${WORKER_RESOURCES[$candidate]}" "${WORKER_RESOURCES[$running]}"; then
        return 1
      fi
    done
    return 0
  }

  finish_worker() {
    local idx=$1 pid work rc begin_ms end_ms duration
    pid=${WORKER_PIDS[$idx]}
    work=${WORKER_DIRS[$idx]}
    set +e
    wait "$pid"
    set -e
    rc=$(cat "$work/exit" 2>/dev/null || echo 1)
    begin_ms=$(cat "$work/begin_ms" 2>/dev/null || echo 0)
    end_ms=$(cat "$work/end_ms" 2>/dev/null || now_ms)
    duration=$((end_ms - begin_ms))
    [ "$duration" -ge 0 ] || duration=0
    WORKER_RC[$idx]=$rc
    WORKER_DURATION[$idx]=$duration
    WORKER_BEGIN_ISO[$idx]=$(cat "$work/begin_iso" 2>/dev/null || now_iso)
    WORKER_END_ISO[$idx]=$(cat "$work/end_iso" 2>/dev/null || now_iso)
    WORKER_STATE[$idx]=done
    active_workers=$((active_workers - 1))
    completed_workers=$((completed_workers + 1))
  }

  wait_one_completed_worker() {
    local idx work
    while :; do
      for idx in "${!WORKER_STATE[@]}"; do
        [ "${WORKER_STATE[$idx]}" = running ] || continue
        work=${WORKER_DIRS[$idx]}
        if [ -f "$work/exit" ] || ! kill -0 "${WORKER_PIDS[$idx]}" 2>/dev/null; then
          finish_worker "$idx"
          return
        fi
      done
      sleep 0.01
    done
  }

  launch_worker() {
    local idx=$1 script=${SCRIPTS[$1]} work base family expected
    work="$RUN_TMP/w$idx"
    mkdir -p "$work/tmp"
    chmod 0700 "$work" "$work/tmp" || die "could not chmod 0700 worker root $work"
    base=$(basename "$script")
    family=$(family_for_basename "$base")
    expected=$(expected_gate_skip_for_family "$family")
    WORKER_DIRS[$idx]=$work
    WORKER_STATE[$idx]=running
    (
      set +e
      export TMPDIR="$work/tmp"
      export TMP="$work/tmp"
      unset MX_HOME MX_STATE_OVERRIDE MX_DATA_OVERRIDE MX_ROOT_OVERRIDE \
        MX_PROJECTS_OVERRIDE MX_CONFIG_OVERRIDE MX_BACKEND 2>/dev/null || true
      cd "$ROOT" || exit 1
      begin_ms=$(now_ms)
      begin_iso=$(now_iso)
      printf '%s\n' "$begin_ms" >"$work/begin_ms"
      printf '%s\n' "$begin_iso" >"$work/begin_iso"
      bash "$script" >"$work/output" 2>&1
      rc=$?
      end_ms=$(now_ms)
      end_iso=$(now_iso)
      printf '%s\n' "$end_ms" >"$work/end_ms"
      printf '%s\n' "$end_iso" >"$work/end_iso"
      printf '%s\n' "$rc" >"$work/exit"
      exit 0
    ) &
    WORKER_PIDS[$idx]=$!
    active_workers=$((active_workers + 1))
  }

  selection_file="$RUN_TMP/selection.txt"
  estimates_file="$RUN_TMP/estimates.tsv"
  printf '%s\n' "${SCRIPTS[@]}" >"$selection_file"
  python3 - "$ROOT/docs/mx-test-performance-baseline.json" "$selection_file" >"$estimates_file" <<'PY'
import json
import sys
from pathlib import Path

baseline_path = Path(sys.argv[1])
selection_path = Path(sys.argv[2])
estimates = {}
if baseline_path.exists():
    doc = json.loads(baseline_path.read_text(encoding="utf-8"))
    estimates.update({
        path: int(duration)
        for path, duration in doc.get("scheduler_estimates_ms", {}).items()
    })
    for row in doc.get("scripts", []):
        estimates.setdefault(row["path"], int(row.get("duration_ms") or 1000))
for path in selection_path.read_text(encoding="utf-8").splitlines():
    if path:
        print(f"{path}\t{estimates.get(path, 1000)}")
PY

  idx=0
  while IFS=$'\t' read -r estimated_script estimated_duration; do
    [ "$estimated_script" = "${SCRIPTS[$idx]}" ] \
      || die "scheduler estimate order diverged at $estimated_script"
    WORKER_STATE[$idx]=pending
    WORKER_RESOURCES[$idx]=$(resources_for_script "${SCRIPTS[$idx]}")
    WORKER_ESTIMATE[$idx]=$estimated_duration
    idx=$((idx + 1))
  done <"$estimates_file"
  [ "$idx" -eq "$script_count" ] || die "scheduler estimates omitted selected scripts"

  while [ "$completed_workers" -lt "$script_count" ]; do
    launched=0
    while [ "$active_workers" -lt "$JOBS" ]; do
      best_idx=-1
      best_estimate=-1
      idx=0
      while [ "$idx" -lt "$script_count" ]; do
        if [ "${WORKER_STATE[$idx]}" = pending ] && can_launch_worker "$idx"; then
          if [ "${WORKER_ESTIMATE[$idx]}" -gt "$best_estimate" ]; then
            best_idx=$idx
            best_estimate=${WORKER_ESTIMATE[$idx]}
          fi
        fi
        idx=$((idx + 1))
      done
      if [ "$best_idx" -lt 0 ]; then
        break
      fi
      launch_worker "$best_idx"
      launched=1
    done
    if [ "$active_workers" -gt 0 ]; then
      wait_one_completed_worker
    elif [ "$launched" -eq 0 ]; then
      die "resource scheduler deadlocked with pending tests"
    fi
  done

  # Deterministic replay and aggregation in the original selection order.
  idx=0
  while [ "$idx" -lt "$script_count" ]; do
    script=${SCRIPTS[$idx]}
    work=${WORKER_DIRS[$idx]}
    base=$(basename "$script")
    family=$(family_for_basename "$base")
    expected=$(expected_gate_skip_for_family "$family")
    printf 'MX_TEST_BEGIN %s %s family=%s expected_gate_skip=%s\n' \
      "${WORKER_BEGIN_ISO[$idx]}" "$script" "$family" "$expected"
    [ ! -s "$work/output" ] || cat "$work/output"
    rc=${WORKER_RC[$idx]}
    mode=$(stat -c %a "$work" 2>/dev/null || stat -f %Lp "$work" 2>/dev/null || echo unknown)
    case "$mode" in
      700|0700) ;;
      *)
        log "isolation failure: worker root mode is $mode, expected 0700 ($work)"
        rc=1
        ;;
    esac
    record_script_result "$script" "$rc" "${WORKER_DURATION[$idx]}" \
      "$work/output" "${WORKER_END_ISO[$idx]}"
    idx=$((idx + 1))
  done
fi

RUN_FINISHED_ISO=$(now_iso)
RUN_FINISHED_MS=$(now_ms)
RUN_DURATION=$((RUN_FINISHED_MS - RUN_STARTED_MS))
if [ "$RUN_DURATION" -lt 0 ]; then
  RUN_DURATION=0
fi

printf 'MX_TEST_SUMMARY total=%s failed=%s skipped_gate=%s duration_ms=%s\n' \
  "$TOTAL" "$FAILED" "$SKIPPED_GATE" "$RUN_DURATION"

if [ -s "$FAMILIES_TSV" ]; then
  # Stable family summary order by name.
  sort -t$'\t' -k1,1 "$FAMILIES_TSV" | while IFS=$'\t' read -r name count duration failed_count; do
    printf 'MX_TEST_SUMMARY_FAMILY family=%s count=%s duration_ms=%s failed=%s\n' \
      "$name" "$count" "$duration" "$failed_count"
  done
fi

# Slowest scripts (top 15) from records.
if [ -s "$RECORDS" ]; then
  rank=1
  sort -t$'\t' -k6,6nr "$RECORDS" | head -n 15 | while IFS=$'\t' read -r path _family _expected _resources _rc duration _gate _output; do
    printf 'MX_TEST_SLOWEST rank=%s script=%s duration_ms=%s\n' \
      "$rank" "$path" "$duration"
    rank=$((rank + 1))
  done
fi

if [ -n "$JSON_PATH" ]; then
  mkdir -p "$(dirname "$JSON_PATH")"
  # Families file may be unsorted; write_json reads as-is (deterministic sort in python).
  if [ -s "$FAMILIES_TSV" ]; then
    sort -t$'\t' -k1,1 "$FAMILIES_TSV" -o "$FAMILIES_TSV"
  else
    : >"$FAMILIES_TSV"
  fi
  write_json_artifact "$JSON_PATH" \
    "$RUN_STARTED_ISO" "$RUN_FINISHED_ISO" "$RUN_ID" \
    "$TOTAL" "$FAILED" "$SKIPPED_GATE" "$RUN_DURATION" \
    "$SELECTION_DESC" "$RECORDS" "$FAMILIES_TSV" "$JOBS"
  log "wrote timing artifact: $JSON_PATH"
fi

exit "$AGG_RC"
