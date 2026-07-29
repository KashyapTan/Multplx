#!/usr/bin/env bash
# Shared behavior cases for the catchup projection wrapper over mx-system-snapshot.sh.
# Covers the output/token bound, TOON/JSON parity, the local-only default (zero
# GitHub/network calls), the --include-prs opt-in path, graceful degradation on a
# partial PR-fetch failure, end-to-end unresolved-decision durability, and current
# report pointers.
set -u

# shellcheck source=tests/lib.sh
# shellcheck disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

STATUS="$ROOT/bin/mx-status-snapshot.sh"
TMP_ROOT=$(mx_test_tmproot mx-catchup)
MX_TEST_CLEANUP_DIRS+=("$TMP_ROOT")
trap mx_test_cleanup EXIT

command -v jq >/dev/null 2>&1 || { echo "skip: jq not found"; exit 0; }

# A fakebin that stubs the local tools the canonical snapshot may reach for, plus a
# gh that RECORDS every call to $NET_LOG so a test can prove the default path
# makes no network call. gh returns one fixture open PR keyed to the delivery task.
make_fakebin() {  # <dir>
  local fb
  fb=$(mx_fakebin "$1")
  cat > "$fb/no-mistakes" <<'SH'
#!/usr/bin/env bash
[ "${FAKE_NM_SLEEP:-0}" = 1 ] && sleep 30
exit 0
SH
  cat > "$fb/tmux" <<'SH'
#!/usr/bin/env bash
case "${1:-}" in
  display-message) case "$*" in *dead-*) exit 1 ;; *) printf '%%1\n' ;; esac ;;
  capture-pane)
    case "$*" in
      *mx-domain-alpha*) printf 'stale terminal summary: Phase 7 started\n> \n' ;;
      *) printf 'all quiet\n> \n' ;;
    esac
    ;;
esac
exit 0
SH
  cat > "$fb/gh" <<'SH'
#!/usr/bin/env bash
echo "gh $*" >> "$NET_LOG"
if [ "${FAKE_GH_FAIL:-0}" = 1 ]; then exit 1; fi
if [ "${FAKE_GH_SLEEP:-0}" = 1 ]; then sleep 30; fi
if [ "${FAKE_GH_MANY:-0}" = 1 ]; then
  cat <<'JSON'
[{"number":1,"title":"One","url":"https://github.com/acme/repo/pull/1","headRefName":"mx/one","reviewDecision":"","mergeable":"MERGEABLE","statusCheckRollup":[]},{"number":2,"title":"Two","url":"https://github.com/acme/repo/pull/2","headRefName":"mx/two","reviewDecision":"","mergeable":"MERGEABLE","statusCheckRollup":[]},{"number":3,"title":"Three","url":"https://github.com/acme/repo/pull/3","headRefName":"mx/three","reviewDecision":"","mergeable":"MERGEABLE","statusCheckRollup":[]}]
JSON
  exit 0
fi
cat <<'JSON'
[{"number":9,"title":"Deliver the thing","url":"https://github.com/KashyapTan/Multplx/pull/9","headRefName":"mx/delivery-task","reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":[{"conclusion":"SUCCESS","status":"COMPLETED"}]}]
JSON
SH
  cat > "$fb/curl" <<'SH'
#!/usr/bin/env bash
echo "curl $*" >> "$NET_LOG"
exit 1
SH
  chmod +x "$fb/no-mistakes" "$fb/tmux" "$fb/gh" "$fb/curl"
  printf '%s\n' "$fb"
}

make_home() {  # <name>
  local home=$TMP_ROOT/$1
  mkdir -p "$home/state" "$home/data" "$home/projects" "$home/config"
  printf '%s\n' "$home"
}

fixture_mate_home() {  # <parent-home>
  printf '%s/%s-daemon-home\n' "$TMP_ROOT" "$(basename "$1")"
}

# Standard fixture: a delivery task with a recorded PR, a scout task with a report, a
# daemon with a MASKED open decision (needs-decision then a later unrelated
# done), and a backlog with a superseded queued item.
write_fixture() {  # <home>
  local home=$1 mate
  mate=$(fixture_mate_home "$home")
  mkdir -p "$home/projects/delivery-wt" "$home/data/scout-x" "$mate/data" "$mate/state" "$mate/config" "$mate/projects" "$mate/bin"
  printf '# Multplx fixture\n' > "$mate/AGENTS.md"
  printf 'mate\n' > "$mate/.mx-daemon-home"
  printf -- '- mate - fixture domain (home: %s; scope: fixture work; projects: broker; added 2026-07-11)\n' \
    "$mate" > "$home/data/daemons.md"
  cat > "$home/data/backlog.md" <<EOF
## In flight
- [ ] delivery-task - Deliver the thing (repo: broker) (kind: delivery) (since 2026-07-11)
- [ ] scout-x - Investigate the thing data/scout-x/report.md (repo: broker) (kind: scout) (since 2026-07-11)

## Queued
- [ ] live-gate - Real queued work blocked-by: delivery-task (repo: broker) (kind: delivery)
- [ ] dead-gate - Old conditional work (repo: broker) (kind: scout)
  NOT REQUIRED - superseded 2026-07-11; kept as reference only.

## Done
- [x] done-a - Landed thing https://github.com/KashyapTan/Multplx/pull/7 (repo: broker) (kind: delivery) (merged 2026-07-10)
EOF
  printf '# Scout X\n' > "$home/data/scout-x/report.md"
  mx_write_meta "$home/state/delivery-task.meta" \
    "window=broker:mx-delivery-task" \
    "worktree=$home/projects/delivery-wt" \
    "project=broker" \
    "harness=codex" \
    "kind=delivery" \
    "mode=no-mistakes" \
    "pr=https://github.com/KashyapTan/Multplx/pull/9"
  printf 'working: building the thing\n' > "$home/state/delivery-task.status"
  mx_write_meta "$home/state/scout-x.meta" \
    "window=broker:mx-scout-x" \
    "worktree=$home/projects/delivery-wt" \
    "project=broker" \
    "harness=codex" \
    "kind=scout" \
    "mode=scout"
  printf 'done: report ready\n' > "$home/state/scout-x.status"
  mx_write_meta "$home/state/mate.meta" \
    "window=broker:mx-mate" \
    "worktree=$mate" \
    "project=$mate" \
    "harness=codex" \
    "kind=daemon" \
    "mode=daemon" \
    "home=$mate" \
    "projects=broker"
  printf 'needs-decision [key=race]: pick subscribe order\n' > "$home/state/mate.status"
  printf 'done: an unrelated subtask finished\n' >> "$home/state/mate.status"
  mx_write_meta "$home/state/external-wait.meta" \
    "window=broker:mx-external-wait" \
    "worktree=$home/projects/delivery-wt" \
    "project=broker" \
    "harness=codex" \
    "kind=delivery" \
    "mode=no-mistakes"
  printf 'paused: declared external-wait for upstream release\n' > "$home/state/external-wait.status"
  # The daemon's OWN home backlog records a merge it managed. This lands in the
  # daemon home, never the main backlog, so landed-work views only see it via the
  # bounded cross-home Done roll-up.
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight
- [ ] mate - Decide subscription order (repo: broker) (kind: delivery) (since 2026-07-11)

## Queued
- [ ] mate-decision-race - Choose subscription order (repo: broker) (kind: maintainer) (hold: maintainer choice pending) (hold-kind: maintainer)

## Done
- [x] mate-landed - Daemon-managed fix https://github.com/KashyapTan/Multplx/pull/50 (repo: broker) (kind: delivery) (merged 2026-07-11)
EOF
  mkdir -p "$mate/projects/mate"
  mx_write_meta "$mate/state/mate.meta" \
    "window=broker:mx-mate" "worktree=$mate/projects/mate" "project=broker" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'needs-decision [key=race]: pick subscribe order\n' > "$mate/state/mate.status"
}

run() {  # <home> <fakebin> <args...>
  local home=$1 fakebin=$2; shift 2
  PATH="$fakebin:$PATH" MX_HOME="$home" MX_STATUS_NOW=2026-07-11T18:00:00Z NET_LOG="$home/net.log" "$STATUS" "$@"
}

# End-to-end Domain Alpha regression fixture.
# The parent event claims Phase 7 started, while the registered home has no child
# metadata, every sample-rollout item is Done, and only an external legal hold remains.
write_domain_alpha_fixture() {  # <parent-home> <daemon-home>
  local home=$1 mate=$2 i
  mkdir -p "$mate/state" "$mate/data" "$mate/config" "$mate/projects" "$mate/bin"
  printf '# Multplx fixture\n' > "$mate/AGENTS.md"
  printf 'domain-alpha\n' > "$mate/.mx-daemon-home"
  printf -- '- domain-alpha - sample rollout (home: %s; scope: sample rollout and legal release; projects: sample; added 2026-07-13)\n' \
    "$mate" > "$home/data/daemons.md"
  mx_write_daemon_meta "$home/state/domain-alpha.meta" "$mate" "broker:mx-domain-alpha" sample
  printf 'working [key=phase7]: Phase 7 started\n' > "$home/state/domain-alpha.status"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] legal-release - Release approval blocked-by: external-legal - external legal dependency (repo: sample) (kind: delivery)

## Done
EOF
  i=1
  while [ "$i" -le 7 ]; do
    printf -- '- [x] phase%s - Sample rollout Phase %s (repo: sample) (kind: delivery) (done 2026-07-%02d)\n' \
      "$i" "$i" "$i" >> "$mate/data/backlog.md"
    i=$((i + 1))
  done
}

# This is the Domain Alpha failure shape exactly: the structured home says Phase 7 is Done
# and no child is active, so the stale parent event must never become Underway.
test_domain_alpha_stale_parent_event_does_not_become_current_work() {
  local home mate fakebin json canonical
  home=$(make_home domain-alpha-parent)
  mate="$TMP_ROOT/domain-alpha-home"
  write_domain_alpha_fixture "$home" "$mate"
  fakebin=$(make_fakebin "$home"); : > "$home/net.log"
  json=$(FAKE_GH_FAIL=1 run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.in_flight | any(.[]; .id == "domain-alpha") | not)
      and (.daemons | any(.[];
        .id == "domain-alpha"
          and .state == "externally_held"
          and .provenance == "structured-home"
          and .freshness == "fresh"
          and .contradiction == true))
      and (.gates | any(.[]; .id == "legal-release" and .owner == "domain-alpha"))
      and (.landed | any(.[]; .id == "phase7" and .owner == "domain-alpha"))
  ' >/dev/null || fail "stale parent Phase 7 event overrode authoritative Domain Alpha state: $json"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_NOW_EPOCH=1783792800 MX_SNAPSHOT_TERMINAL_LINES=2 MX_SNAPSHOT_TERMINAL_BYTES=64 \
    NET_LOG="$home/net.log" FAKE_GH_FAIL=1 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "domain-alpha")
    | .provenance.selected == "structured-home"
      and .freshness.status == "fresh"
      and .terminal_evidence.provenance == "parent-direct-report-terminal"
      and .terminal_evidence.trust == "untrusted-supplement"
      and .terminal_evidence.captured == true
      and .terminal_evidence.lines == 2
      and .terminal_evidence.bytes <= 64
      and (.terminal_evidence | has("content") | not)
      and .terminal_evidence.event_note_seen == true
      and .terminal_evidence.contradiction == true
      and .contradiction == true
  ' >/dev/null || fail "bounded terminal contradiction evidence was not labeled and subordinate: $canonical"
  [ ! -s "$home/net.log" ] || fail "Domain Alpha structured-home read made a network call: $(cat "$home/net.log")"
  pass "Domain Alpha structured state overrides a stale parent Phase 7 event"
}

test_gnu_stat_uses_file_formats_without_bsd_fallback_pollution() {
  local home mate fakebin canonical stat_log
  home=$(make_home gnu-stat-parent)
  mate="$TMP_ROOT/gnu-stat-home"
  write_domain_alpha_fixture "$home" "$mate"
  fakebin=$(make_fakebin "$home")
  stat_log="$home/stat.log"
  cat > "$fakebin/uname" <<'SH'
#!/usr/bin/env bash
printf 'Linux\n'
SH
  cat > "$fakebin/stat" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$STAT_LOG"
case "$1 $2" in
  '-c %a') printf '600\n' ;;
  '-c %Y') printf '1783792800\n' ;;
  '-c %s') LC_ALL=C wc -c < "$3" | tr -d ' ' ;;
  -f\ *)
    printf '  File: "%s"\nBlocks: Total: 1\n' "$2"
    exit 1
    ;;
  *) exit 2 ;;
esac
SH
  chmod +x "$fakebin/uname" "$fakebin/stat"
  canonical=$(PATH="$fakebin:$PATH" STAT_LOG="$stat_log" MX_HOME="$home" \
    MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z MX_SNAPSHOT_NOW_EPOCH=1783792800 \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "domain-alpha")
    | .provenance.selected == "structured-home"
      and .parent_event.activity_scan.available == true
  ' >/dev/null || fail "GNU stat fixture corrupted the authoritative daemon summary: $canonical"
  assert_contains "$(cat "$stat_log")" '-c %a' "GNU registry mode must use stat -c"
  assert_contains "$(cat "$stat_log")" '-c %Y' "GNU parent-event mtime must use stat -c"
  assert_contains "$(cat "$stat_log")" '-c %s' "GNU parent-event size must use stat -c"
  if grep -q '^-f ' "$stat_log"; then
    fail "GNU snapshot invoked BSD stat -f before its GNU file reads: $(cat "$stat_log")"
  fi
  pass "GNU stat file reads select -c without BSD filesystem-report pollution"
}

test_parent_activity_evidence_is_bounded_and_disclosed() {
  local home mate fakebin canonical json i
  home=$(make_home bounded-parent-activity)
  mate="$TMP_ROOT/bounded-parent-activity-home"
  write_domain_alpha_fixture "$home" "$mate"
  : > "$home/state/domain-alpha.status"
  i=1
  while [ "$i" -le 6 ]; do
    printf 'working [key=phase%s]: Phase %s started\n' "$i" "$i" >> "$home/state/domain-alpha.status"
    i=$((i + 1))
  done
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_PARENT_ACTIVITY_LINES=4 MX_SNAPSHOT_PARENT_ACTIVITY_BYTES=4096 \
    MX_SNAPSHOT_PARENT_ACTIVITIES=2 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "domain-alpha")
    | .parent_event.activity_scan.available == true
      and .parent_event.activity_scan.input_truncated == true
      and .parent_event.activity_scan.retained_truncated == true
      and .parent_event.activity_scan.lines_in_window == 4
      and .parent_event.activity_scan.records_in_window == 4
      and .parent_event.activity_scan.reasons == ["line_limit", "activity_limit"]
      and (.parent_event.open_activities | map(.key)) == ["phase5", "phase6"]
  ' >/dev/null || fail "parent activity evidence was not bounded and disclosed: $canonical"
  json=$(MX_SNAPSHOT_PARENT_ACTIVITY_LINES=4 MX_SNAPSHOT_PARENT_ACTIVITY_BYTES=4096 \
    MX_SNAPSHOT_PARENT_ACTIVITIES=2 run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    .omitted | any(.surface == "daemon parent activity evidence truncated for 1 record(s)")
  ' >/dev/null || fail "catchup did not disclose bounded parent activity evidence: $json"
  pass "parent activity evidence is bounded and disclosed"
}

test_active_child_overrides_old_parent_event() {
  local home mate fakebin json canonical
  home=$(make_home active-child-parent)
  mate="$TMP_ROOT/active-child-home"
  write_domain_alpha_fixture "$home" "$mate"
  mkdir -p "$mate/projects/phase8"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight
- [ ] phase8 - Sample rollout Phase 8 (repo: sample) (kind: delivery) (since 2026-07-13)

## Queued

## Done
- [x] phase7 - Sample rollout Phase 7 (repo: sample) (kind: delivery) (done 2026-07-12)
EOF
  mx_write_meta "$mate/state/phase8.meta" \
    "window=broker:mx-phase8" "worktree=$mate/projects/phase8" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'working [key=phase8]: implementing Phase 8 parity\nneeds-decision [key=release]: choose release A or B\n' \
    > "$mate/state/phase8.status"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.daemons | any(.[]; .id == "domain-alpha" and .state != "maintainer_decision"
      and (.doing | contains("release A or B") | not)))
      and (.decisions_open | any(.owner == "domain-alpha") | not)
  ' >/dev/null || fail "status-only child decision leaked into Catchup: $json"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "domain-alpha") | .endpoints[] | select(.id == "phase8")
    | .endpoint.status == "unknown"
      and .endpoint.exists == true
      and .endpoint.freshness == "fresh"
      and .endpoint.observed_at == "2026-07-11T18:00:00Z"
  ' >/dev/null || fail "child endpoint observation lacked bounded current freshness: $canonical"
  pass "Catchup excludes a status-only child decision"
}

test_structured_child_decision_reaches_maintainers_call() {
  local home mate fakebin json
  home=$(make_home child-decision-parent)
  mate="$TMP_ROOT/child-decision-home"
  write_domain_alpha_fixture "$home" "$mate"
  mkdir -p "$mate/projects/phase8"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight
- [ ] phase8 - Sample rollout Phase 8 (repo: sample) (kind: delivery) (since 2026-07-13)

## Queued
- [ ] phase8-decision-release - Choose sample release (repo: sample) (kind: maintainer) (hold: maintainer release choice pending) (hold-kind: maintainer)

## Done
- [x] phase7 - Sample rollout Phase 7 (repo: sample) (kind: delivery) (done 2026-07-12)
EOF
  mx_write_meta "$mate/state/phase8.meta" \
    "window=broker:mx-phase8" "worktree=$mate/projects/phase8" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'needs-decision [key=release]: choose release A or B\n' > "$mate/state/phase8.status"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.daemons | any(.[]; .id == "domain-alpha" and .state == "maintainer_decision"))
      and (.decisions_open | any(.[]; .id == "domain-alpha/phase8-decision-release"
        and .key == "phase8-decision-release" and .verb == "maintainer-hold"))
      and (.in_flight | any(.[]; .id == "domain-alpha") | not)
  ' >/dev/null || fail "structured child maintainer hold did not reach Maintainer Call: $json"
  pass "a structured child maintainer hold reaches Maintainer's Call"
}

make_valid_daemon_home() {  # <id> <home>
  local id=$1 home=$2
  mkdir -p "$home/state" "$home/data" "$home/config" "$home/projects" "$home/bin"
  printf '# Multplx fixture\n' > "$home/AGENTS.md"
  printf '%s\n' "$id" > "$home/.mx-daemon-home"
  cat > "$home/data/backlog.md" <<'EOF'
## In flight

## Queued

## Done
EOF
}

append_daemon_registry() {  # <parent> <id> <home>
  printf -- '- %s - fixture domain (home: %s; scope: fixture; projects: sample; added 2026-07-13)\n' \
    "$2" "$3" >> "$1/data/daemons.md"
}

append_landed_row() {  # <daemon-home> <id> <title> <date>
  printf -- '- [x] %s - %s (repo: broker) (kind: delivery) (merged %s)\n' \
    "$2" "$3" "$4" >> "$1/data/backlog.md"
}

make_landed_daemon() {  # <parent> <id>
  local parent=$1 id=$2 mate
  mate="$TMP_ROOT/$(basename "$parent")-$id-home"
  make_valid_daemon_home "$id" "$mate"
  append_daemon_registry "$parent" "$id" "$mate"
  printf '%s\n' "$mate"
}

write_parent_daemon_event() {  # <parent> <id> <home> <note>
  mx_write_daemon_meta "$1/state/$2.meta" "$3" "broker:mx-$2" sample
  printf 'working [key=%s]: %s\n' "$2" "$4" > "$1/state/$2.status"
}

test_bad_daemon_homes_never_revive_parent_work() {
  local home fakebin missing invalid unreadable malformed timedout wt json
  home=$(make_home bad-homes)
  : > "$home/data/daemons.md"
  missing="$TMP_ROOT/missing-home"
  invalid="$TMP_ROOT/invalid-home"
  unreadable="$TMP_ROOT/unreadable-home"
  malformed="$TMP_ROOT/malformed-home"
  timedout="$TMP_ROOT/timedout-home"

  append_daemon_registry "$home" missing "$missing"

  make_valid_daemon_home invalid "$invalid"
  printf 'someone-else\n' > "$invalid/.mx-daemon-home"
  append_daemon_registry "$home" invalid "$invalid"
  write_parent_daemon_event "$home" invalid "$invalid" "old invalid work"

  make_valid_daemon_home unreadable "$unreadable"
  chmod 000 "$unreadable/data"
  append_daemon_registry "$home" unreadable "$unreadable"
  write_parent_daemon_event "$home" unreadable "$unreadable" "old unreadable work"

  make_valid_daemon_home malformed "$malformed"
  printf '## In flight\nthis current row is not structured\n' > "$malformed/data/backlog.md"
  append_daemon_registry "$home" malformed "$malformed"
  write_parent_daemon_event "$home" malformed "$malformed" "old malformed work"

  make_valid_daemon_home timedout "$timedout"
  wt="$timedout/projects/slow"
  mx_git_init_commit "$wt"
  git -C "$wt" checkout -q -b mx/slow
  printf '## In flight\n- [ ] slow - Slow child (repo: sample) (kind: delivery) (since 2026-07-13)\n\n## Queued\n\n## Done\n' > "$timedout/data/backlog.md"
  mx_write_meta "$timedout/state/slow.meta" \
    "window=broker:mx-slow" "worktree=$wt" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  append_daemon_registry "$home" timedout "$timedout"
  write_parent_daemon_event "$home" timedout "$timedout" "old timed work"

  fakebin=$(make_fakebin "$home")
  json=$(FAKE_NM_SLEEP=1 MX_SNAPSHOT_DAEMON_TIMEOUT=1 run "$home" "$fakebin" --json)
  chmod 700 "$unreadable/data"
  printf '%s' "$json" | jq -e '
    (.daemons | length) == 5
      and all(.daemons[]; .state == "unknown")
      and (.in_flight | map(.id) | all(. != "invalid" and . != "unreadable" and . != "malformed" and . != "timedout"))
      and (.daemons | any(.[]; .id == "missing" and .provenance == "unknown"
        and .freshness == "unknown" and (.reason | contains("invalid home"))))
      and ([.daemons[] | select(.id != "missing")]
        | all(.provenance == "parent-event-fallback" and .freshness == "historical-event"))
      and (.daemons | any(.[]; .id == "invalid" and (.reason | contains("marked for"))))
      and (.daemons | any(.[]; .id == "unreadable" and (.reason | test("invalid home|unreadable"))))
      and (.daemons | any(.[]; .id == "malformed" and (.reason | contains("unstructured current backlog row"))))
      and (.daemons | any(.[]; .id == "timedout" and (.reason | contains("timed out"))))
  ' >/dev/null || fail "bad home outcomes revived stale work or lacked provenance: $json"
  pass "missing, invalid, unreadable, malformed, and timed-out homes stay explicit unknowns"
}

test_oversized_daemon_summary_stays_strict_unknown() {
  local home mate fakebin json i
  home=$(make_home oversized-home)
  mate="$TMP_ROOT/oversized-daemon-home"
  make_valid_daemon_home oversized "$mate"
  append_daemon_registry "$home" oversized "$mate"
  mx_write_daemon_meta "$home/state/oversized.meta" "$mate" "broker:mx-oversized" sample
  printf 'working [key=old]: stale parent activity\n' > "$home/state/oversized.status"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight

## Queued

## Done
EOF
  i=1
  while [ "$i" -le 30 ]; do
    printf -- '- [x] landed-%02d - Bounded landed fixture %02d (repo: sample) (kind: delivery) (done 2026-07-01)\n' \
      "$i" "$i" >> "$mate/data/backlog.md"
    i=$((i + 1))
  done
  fakebin=$(make_fakebin "$home")
  json=$(MX_SNAPSHOT_DAEMON_MAX_BYTES=512 run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.daemons | any(.id == "oversized" and .state == "unknown"
      and .provenance == "parent-event-fallback"
      and (.reason | contains("exceeded byte limit"))))
      and (.in_flight | any(.id == "oversized") | not)
      and (.decisions_open | any(.owner == "oversized") | not)
      and (.landed | any(.owner == "oversized") | not)
  ' >/dev/null || fail "oversized summary revived or retained unvalidated surfaces: $json"
  pass "an oversized daemon summary retains the strict empty unknown fallback"
}

test_daemon_and_child_bounds_are_disclosed() {
  local home fakebin id mate child json expanded canonical i
  home=$(make_home daemon-bounds)
  : > "$home/data/daemons.md"
  for id in a b c; do
    mate="$TMP_ROOT/bounds-$id"
    make_valid_daemon_home "$id" "$mate"
    append_daemon_registry "$home" "$id" "$mate"
  done
  mate="$TMP_ROOT/bounds-a"
  : > "$mate/data/backlog.md"
  printf '## In flight\n' >> "$mate/data/backlog.md"
  i=1
  while [ "$i" -le 3 ]; do
    child="child-$i"
    mkdir -p "$mate/projects/$child"
    printf -- '- [ ] %s - Active %s (repo: sample) (kind: delivery) (since 2026-07-13)\n' "$child" "$child" >> "$mate/data/backlog.md"
    mx_write_meta "$mate/state/$child.meta" \
      "window=broker:mx-$child" "worktree=$mate/projects/$child" "project=sample" \
      "harness=codex" "kind=delivery" "mode=no-mistakes"
    printf 'working [key=%s]: active child %s\n' "$child" "$i" > "$mate/state/$child.status"
    i=$((i + 1))
  done
  printf '\n## Queued\n\n## Done\n' >> "$mate/data/backlog.md"
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_DAEMONS=2 MX_SNAPSHOT_DAEMON_CHILDREN=2 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.total_registered == 3
      and .daemon_current.shown == 2
      and .daemon_current.truncated == 1
      and (.daemon_current.records[] | select(.id == "a")
        | .counts.active_children == 3 and (.active_children | length) == 2
          and (.omitted | any(.surface == "active_children" and .count == 1)))
      and (.daemon_current.records | any(.id == "b" and .current.state == "no_active_work"))
  ' >/dev/null || fail "canonical daemon or child bounds were not enforced: $canonical"
  json=$(MX_SNAPSHOT_DAEMONS=2 MX_SNAPSHOT_DAEMON_CHILDREN=2 MX_STATUS_DAEMONS=1 \
    run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.daemons | length) == 1
      and ([.omitted[].surface] | any(test("daemons showing 1 of 2")))
      and ([.omitted[].surface] | any(test("registered daemons omitted by snapshot bound: 1")))
  ' >/dev/null || fail "catchup daemon bound was not disclosed: $json"
  expanded=$(MX_SNAPSHOT_DAEMON_CHILDREN=2 MX_STATUS_DAEMONS=1 \
    run "$home" "$fakebin" --json --all-daemons)
  printf '%s' "$expanded" | jq -e '
    (.daemons | length) == 3
      and ([.omitted[].surface] | any(test("daemons showing|registered daemons omitted")) | not)
  ' >/dev/null || fail "--all-daemons did not expand the canonical and catchup bounds: $expanded"
  pass "daemon and per-home child counts are bounded, disclosed, and explicitly expandable"
}

test_parent_decision_is_untrusted_contradiction_only() {
  local home mate fakebin canonical json
  home=$(make_home parent-decision-only)
  mate="$TMP_ROOT/parent-decision-only-home"
  make_valid_daemon_home authority "$mate"
  append_daemon_registry "$home" authority "$mate"
  mx_write_daemon_meta "$home/state/authority.meta" "$mate" "broker:mx-authority" sample
  printf 'needs-decision [key=stale]: old parent question\n' > "$home/state/authority.status"
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "authority")
    | .current.state == "no_active_work"
      and .decisions_open == []
      and .contradiction == true
      and (.parent_event.open_decisions | any(.key == "stale" and .verb == "needs-decision"))
  ' >/dev/null || fail "parent decision crossed structured-home authority boundary: $canonical"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.daemons | any(.[]; .id == "authority" and .state == "no_active_work" and .contradiction == true))
      and (.decisions_open | any(.[]; .id == "authority") | not)
  ' >/dev/null || fail "catchup promoted a stale parent decision: $json"
  pass "parent decisions remain untrusted contradiction evidence"
}

test_parent_evidence_reconciles_by_verb_and_key() {
  local home hold blocked decision fakebin canonical mate child
  home=$(make_home keyed-parent-evidence)
  hold="$TMP_ROOT/keyed-parent-hold-home"
  blocked="$TMP_ROOT/keyed-parent-blocked-home"
  decision="$TMP_ROOT/keyed-parent-decision-home"
  make_valid_daemon_home hold "$hold"
  make_valid_daemon_home blocked "$blocked"
  make_valid_daemon_home decision "$decision"
  append_daemon_registry "$home" hold "$hold"
  append_daemon_registry "$home" blocked "$blocked"
  append_daemon_registry "$home" decision "$decision"
  mx_write_daemon_meta "$home/state/hold.meta" "$hold" "broker:mx-hold" sample
  mx_write_daemon_meta "$home/state/blocked.meta" "$blocked" "broker:mx-blocked" sample
  mx_write_daemon_meta "$home/state/decision.meta" "$decision" "broker:mx-decision" sample
  printf 'working [key=stale-work]: old work still running\n' > "$home/state/hold.status"
  printf 'paused [key=legal-release]: waiting for legal release\n' >> "$home/state/hold.status"
  printf 'paused: legacy pause without an identity\n' >> "$home/state/hold.status"
  printf 'blocked [key=vendor-release]: waiting for vendor release\n' > "$home/state/blocked.status"
  printf 'blocked: legacy block without an identity\n' >> "$home/state/blocked.status"
  printf 'needs-decision [key=stale-route]: choose the old route\n' > "$home/state/decision.status"
  printf 'working: legacy work without an identity\n' >> "$home/state/decision.status"
  cat > "$hold/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] legal-release - Legal release blocked-by: external-legal - legal review (repo: sample) (kind: delivery)

## Done
EOF
  cat > "$blocked/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] vendor-release - Vendor release blocked-by: external-vendor - vendor review (repo: sample) (kind: delivery)

## Done
EOF
  child='decision-child'
  mkdir -p "$decision/projects/$child"
  cat > "$decision/data/backlog.md" <<EOF
## In flight
- [ ] $child - Decision child (repo: sample) (kind: delivery) (since 2026-07-11)

## Queued

## Done
EOF
  mx_write_meta "$decision/state/$child.meta" \
    "window=broker:mx-$child" "worktree=$decision/projects/$child" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'needs-decision [key=live-route]: choose the current route\n' > "$decision/state/$child.status"
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    (.daemon_current.records[] | select(.id == "hold")
      | .current.state == "externally_held"
        and .contradiction == true
        and .terminal_evidence.captured == false
        and (.parent_event.reconciliation.activities
          | any(.verb == "paused" and .key == "legal-release" and .verdict == "corroborates"))
        and (.parent_event.reconciliation.activities
          | any(.verb == "paused" and .key == "default" and .verdict == "inconclusive" and .matched == null))
        and (.parent_event.reconciliation.activities
          | any(.verb == "working" and .key == "stale-work" and .verdict == "contradicts")))
      and (.daemon_current.records[] | select(.id == "blocked")
        | .current.state == "externally_held"
          and .contradiction == false
          and (.parent_event.reconciliation.decisions
            | any(.verb == "blocked" and .key == "vendor-release" and .verdict == "corroborates"))
          and (.parent_event.reconciliation.decisions
            | any(.verb == "blocked" and .key == "default" and .verdict == "inconclusive" and .matched == null)))
      and (.daemon_current.records[] | select(.id == "decision")
        | .current.state == "maintainer_decision"
          and .contradiction == true
          and .terminal_evidence.captured == false
          and (.parent_event.reconciliation.activities
            | any(.verb == "working" and .key == "default" and .verdict == "inconclusive" and .matched == null))
          and (.parent_event.reconciliation.decisions
            | any(.verb == "needs-decision" and .key == "stale-route" and .verdict == "contradicts")))
  ' >/dev/null || fail "parent evidence was not reconciled by verb and key: $canonical"
  pass "parent evidence reconciliation distinguishes matching holds, blocks, and decisions"
}

test_nonprogressing_child_states_are_explicit() {
  local home mate fakebin canonical
  home=$(make_home child-state-classification)
  mate="$TMP_ROOT/child-state-classification-home"
  make_valid_daemon_home states "$mate"
  append_daemon_registry "$home" states "$mate"
  mkdir -p "$mate/projects/parked" "$mate/projects/done" "$mate/projects/failed"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight
- [ ] parked - Parked child (repo: sample) (kind: delivery) (since 2026-07-11)

## Queued

## Done
EOF
  mx_write_meta "$mate/state/parked.meta" \
    "window=broker:mx-parked" "worktree=$mate/projects/parked" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'needs-decision [key=parked]: choose a route\n' > "$mate/state/parked.status"
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "states")
    | .current.state == "maintainer_decision"
      and .active_children == []
      and (.holds | any(.id == "parked" and .source == "child-state"))
  ' >/dev/null || fail "parked child was classified as active work: $canonical"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight

## Queued

## Done
EOF
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "states")
    | .current.state == "unknown"
      and (.current.reason | contains("live child state has no in-flight backlog item"))
      and (.current.reason | contains("parked=parked"))
  ' >/dev/null || fail "unowned held child was silently dropped: $canonical"
  cat > "$mate/data/backlog.md" <<'EOF'
## In flight
- [ ] done - Done child still in flight (repo: sample) (kind: delivery) (since 2026-07-11)
- [ ] failed - Failed child still in flight (repo: sample) (kind: delivery) (since 2026-07-11)

## Queued

## Done
EOF
  mx_write_meta "$mate/state/done.meta" \
    "window=broker:mx-done" "worktree=$mate/projects/done" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  mx_write_meta "$mate/state/failed.meta" \
    "window=broker:mx-failed" "worktree=$mate/projects/failed" "project=sample" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'done: complete\n' > "$mate/state/done.status"
  printf 'failed: stopped\n' > "$mate/state/failed.status"
  rm "$mate/state/parked.meta" "$mate/state/parked.status"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "states")
    | .current.state == "unknown"
      and (.current.reason | contains("terminal child state"))
      and (.current.reason | contains("done=done"))
      and (.current.reason | contains("failed=failed"))
  ' >/dev/null || fail "terminal in-flight child states were silently dropped: $canonical"
  pass "nonprogressing child states are explicit and inconsistent terminal rows invalidate"
}

test_registry_unavailability_and_bounds_are_explicit() {
  local home fakebin json canonical id mate boundary
  home=$(make_home registry-unavailable)
  mate="$TMP_ROOT/registry-hidden"
  make_valid_daemon_home hidden "$mate"
  printf -- '- hidden - fixture (home: %s; scope: fixture; projects: sample; added 2026-07-11)\n' "$mate" > "$home/data/daemons.md"
  mx_write_daemon_meta "$home/state/hidden.meta" "$mate" "broker:mx-hidden" sample
  chmod 000 "$home/data/daemons.md"
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  json=$(run "$home" "$fakebin" --json)
  chmod 600 "$home/data/daemons.md"
  printf '%s' "$canonical" | jq -e '
    .daemon_current.registry.complete == false
      and (.daemon_current.records[] | select(.id == "hidden")
        | .registered == null
          and (.current.reason | contains("registration is unknown")))
  ' >/dev/null || fail "unavailable registry produced false unregistered provenance: $canonical"
  printf '%s' "$json" | jq -e '
    (.daemons | any(.[]; .id == "(registry)" and .state == "unknown"
      and .provenance == "registered-table" and .freshness == "unavailable"))
      and (.omitted | any(.surface | contains("daemon registry unavailable")))
  ' >/dev/null || fail "unreadable registry disappeared from catchup: $json"
  home=$(make_home registry-bounds)
  : > "$home/data/daemons.md"
  for id in one two three; do
    mate="$TMP_ROOT/registry-$id"
    make_valid_daemon_home "$id" "$mate"
    append_daemon_registry "$home" "$id" "$mate"
  done
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_REGISTRY_RECORDS=2 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.registry
    | .available == true and .provenance == "registered-table"
      and .freshness.status == "fresh" and .records_truncated == true
      and .records_in_window == 3 and (.records | length) == 2
      and (.reasons | index("record_limit") != null)
  ' >/dev/null || fail "registry record bound was not enforced or disclosed: $canonical"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_REGISTRY_LINES=2 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.registry
    | .input_truncated == true and .records_truncated == false
      and .lines_in_window == 2 and (.records | length) == 2
      and .reasons == ["line_limit"]
  ' >/dev/null || fail "registry line bound was not enforced or disclosed: $canonical"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_REGISTRY_BYTES=100 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.registry
    | .input_truncated == true and (.reasons | index("byte_limit") != null)
      and .records_in_window < 3
  ' >/dev/null || fail "registry byte bound was not enforced or disclosed: $canonical"
  boundary=$(LC_ALL=C head -n 1 "$home/data/daemons.md" | wc -c | tr -d ' ')
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_REGISTRY_BYTES="$((boundary - 1))" "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.registry
    | .input_truncated == true and .complete == false
      and (.reasons | index("byte_limit") != null)
  ' >/dev/null || fail "registry newline byte boundary hid truncation: $canonical"
  json=$(MX_SNAPSHOT_REGISTRY_RECORDS=2 run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    .omitted | any(.surface == "daemon registry records omitted by bounded read")
  ' >/dev/null || fail "catchup omitted registry truncation disclosure: $json"
  mate="$TMP_ROOT/registry-z-hidden"
  make_valid_daemon_home z-hidden "$mate"
  append_daemon_registry "$home" z-hidden "$mate"
  mx_write_daemon_meta "$home/state/z-hidden.meta" "$mate" "broker:mx-z-hidden" sample
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    MX_SNAPSHOT_REGISTRY_RECORDS=3 "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.registry.complete == false
      and (.daemon_current.records[] | select(.id == "z-hidden")
        | .registered == null
          and (.current.reason | contains("registration is unknown")))
  ' >/dev/null || fail "truncated registry produced false unregistered provenance: $canonical"
  pass "registry unavailability and bounded truncation remain explicit"
}

test_current_landed_baseline_is_repeatable_and_prior_report_independent() {
  local home fakebin one two
  home=$(make_home standalone-baseline); write_fixture "$home"
  cat > "$home/data/status-report-2026-07-10.md" <<'EOF'
# Misleading old report

## Recently Landed
- fake-old-item

## Underway
- Phase 7 started
EOF
  fakebin=$(make_fakebin "$home")
  one=$(run "$home" "$fakebin" --json)
  two=$(run "$home" "$fakebin" --json)
  [ "$(printf '%s' "$one" | jq -c '.landed')" = "$(printf '%s' "$two" | jq -c '.landed')" ] \
    || fail "the same structured state produced different recent-completion baselines"
  printf '%s' "$two" | jq -e '
    (.landed | any(.id == "done-a"))
      and (.landed | any(.id == "mate-landed"))
      and (.landed | any(.id == "fake-old-item") | not)
      and (.in_flight | any(.doing == "Phase 7 started") | not)
  ' >/dev/null || fail "prior status report influenced the standalone snapshot: $two"
  pass "repeated snapshots keep the same current landed baseline and ignore prior reports"
}

test_default_is_bounded_and_local_only() {
  local home fakebin toon json
  home=$(make_home bounded); write_fixture "$home"
  fakebin=$(make_fakebin "$home"); : > "$home/net.log"
  toon=$(run "$home" "$fakebin")
  json=$(run "$home" "$fakebin" --json)
  # Bound: well under the ~50 KB tool-display limit.
  [ "${#toon}" -lt 50000 ] || fail "default TOON must stay under the display bound, got ${#toon}"
  # TOON is materially smaller than the canonical snapshot it projects.
  local canon; canon=$(PATH="$fakebin:$PATH" MX_HOME="$home" "$ROOT/bin/mx-system-snapshot.sh" --json)
  [ "${#toon}" -lt "${#canon}" ] || fail "projection must be smaller than the canonical snapshot"
  # Local-only: no GitHub/network call on the default path.
  [ ! -s "$home/net.log" ] || fail "default run must make no gh call, got: $(cat "$home/net.log")"
  # Definitive not-requested PR state, never a silent omission.
  assert_contains "$toon" 'prs: "not_requested' "default must state PR checks were not requested"
  assert_contains "$toon" "live PR discovery + checks,\"--include-prs\"" "omitted must mark the dropped live-PR surface"
  # Valid JSON, correct schema.
  printf '%s' "$json" | jq -e '.schema == "mx-catchup.v1"' >/dev/null || fail "json schema wrong"
  pass "default output is bounded, local-only, and marks omitted surfaces"
}

test_toon_json_parity() {
  local home fakebin toon json keys k
  home=$(make_home parity); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  toon=$(run "$home" "$fakebin")
  json=$(run "$home" "$fakebin" --json)
  # Same top-level keys in both representations.
  keys=$(printf '%s' "$json" | jq -r 'keys_unsorted[]')
  for k in $keys; do
    if printf '%s' "$json" | jq -e --arg k "$k" '.[$k] | type == "array"' >/dev/null; then
      local n hdr
      n=$(printf '%s' "$json" | jq --arg k "$k" '.[$k] | length')
      if [ "$n" = 0 ]; then
        assert_contains "$toon" "$k: []" "empty array $k must render as 'key: []'"
      else
        # Header must declare the same count and the same field set.
        hdr=$(printf '%s' "$toon" | grep -E "^$k\[[0-9]+\]\{" || true)
        [ -n "$hdr" ] || fail "TOON missing tabular header for $k"
        assert_contains "$hdr" "[$n]" "TOON $k row count must equal JSON length $n"
        local jfields tfields
        jfields=$(printf '%s' "$json" | jq -r --arg k "$k" '.[$k][0] | keys_unsorted | join(",")')
        tfields=$(printf '%s' "$hdr" | sed -E 's/^[^{]*\{//; s/\}:.*$//; s/"//g')
        [ "$jfields" = "$tfields" ] || fail "TOON $k fields ($tfields) must equal JSON fields ($jfields)"
      fi
    else
      # Scalar: the key must appear as a "key: value" line.
      assert_contains "$toon" "$k: " "TOON must carry scalar field $k"
    fi
  done
  pass "TOON and JSON are parity representations of the same model"
}

test_open_decision_surfaces_end_to_end() {
  local home fakebin json
  home=$(make_home e2e-decision); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    .decisions_open | any(.[]; .id == "mate/mate-decision-race"
      and .key == "mate-decision-race" and .verb == "maintainer-hold")
  ' >/dev/null || fail "an authoritative maintainer hold must surface in decisions_open: $json"
  pass "an authoritative maintainer hold surfaces end-to-end"
}

test_report_pointers_surface() {
  local home fakebin json
  home=$(make_home reports); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e --arg p "$home/data/scout-x/report.md" '
    .reports | any(.[]; .id == "scout-x" and .path == $p)
  ' >/dev/null || fail "current scout report pointer must surface: $json"
  pass "current report pointers surface"
}

test_superseded_queued_item_dropped_by_default() {
  local home fakebin json
  home=$(make_home superseded); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.gates | any(.[]; .id == "live-gate")) and (.gates | any(.[]; .id == "dead-gate") | not)
  ' >/dev/null || fail "default gates must include live and drop superseded: $json"
  json=$(run "$home" "$fakebin" --json --all-queued)
  printf '%s' "$json" | jq -e '.gates | any(.[]; .id == "dead-gate")' >/dev/null \
    || fail "--all-queued must restore the superseded item"
  pass "superseded queued items are dropped by default and restored with --all-queued"
}

test_include_prs_is_the_only_fetch_path() {
  local home fakebin json
  home=$(make_home prs); write_fixture "$home"
  fakebin=$(make_fakebin "$home"); : > "$home/net.log"
  json=$(run "$home" "$fakebin" --include-prs --json)
  # Now gh WAS called, exactly for pr list.
  grep -q '^gh pr list ' "$home/net.log" || fail "--include-prs must call gh pr list"
  printf '%s' "$json" | jq -e '
    .prs | startswith("checked")
  ' >/dev/null || fail "--include-prs must report checked PR state"
  printf '%s' "$json" | jq -e '
    .candidate_prs | any(.[]; .num == "9" and .task == "delivery-task" and .checks == "passing" and .review == "APPROVED")
  ' >/dev/null || fail "candidate_prs must carry the fetched PR cross-referenced to its task: $json"
  pass "--include-prs is the only path that fetches, and it enriches correctly"
}

test_partial_github_failure_degrades() {
  local home fakebin json rc
  home=$(make_home partial); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  json=$(FAKE_GH_FAIL=1 run "$home" "$fakebin" --include-prs --json); rc=$?
  expect_code 0 "$rc" "a PR-fetch failure must not crash the view"
  printf '%s' "$json" | jq -e '
    .schema == "mx-catchup.v1"
      and (.candidate_prs | length) == 0
      and (.prs | test("unavailable"))
      and (.in_flight | length) > 0
  ' >/dev/null || fail "on gh failure the view must still emit, with an unavailable note: $json"
  pass "a partial GitHub failure degrades gracefully"
}

test_perl_fallback_bounds_github_call() {
  local home fakebin toolbin cmd json started elapsed
  home=$(make_home perl-timeout); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  toolbin="$home/toolbin"
  mkdir -p "$toolbin"
  for cmd in bash dirname basename jq date sed git grep tail cut tr head sort wc perl sleep cat find; do
    ln -s "$(command -v "$cmd")" "$toolbin/$cmd"
  done
  started=$(date +%s)
  json=$(PATH="$fakebin:$toolbin" MX_HOME="$home" MX_STATUS_NOW=2026-07-11T18:00:00Z \
    MX_STATUS_PR_TIMEOUT=1 NET_LOG="$home/net.log" FAKE_GH_SLEEP=1 "$STATUS" --include-prs --json)
  elapsed=$(( $(date +%s) - started ))
  [ "$elapsed" -lt 10 ] || fail "Perl fallback did not bound a stalled gh call (${elapsed}s)"
  printf '%s' "$json" | jq -e '.prs | test("unavailable")' >/dev/null \
    || fail "timed-out gh call did not fail soft: $json"
  pass "Perl fallback bounds stalled GitHub calls without coreutils timeout"
}

write_large_fixture() {  # <home> <count>
  local home=$1 count=$2 i id
  : > "$home/data/backlog.md"
  printf '## Queued\n' >> "$home/data/backlog.md"
  i=1
  while [ "$i" -le "$count" ]; do
    id="dead-$i"
    mkdir -p "$home/projects/$id" "$home/data/$id"
    printf '# Report\n' > "$home/data/$id/report.md"
    printf -- '- [ ] gate-%s - Gate %s blocked-by: task-%s (repo: repo-%s) (kind: delivery)\n' "$i" "$i" "$i" "$i" >> "$home/data/backlog.md"
    printf -- '- [ ] decision-%s - Decision %s (repo: repo-%s) (kind: maintainer) (hold: maintainer choice pending) (hold-kind: maintainer)\n' "$i" "$i" "$i" >> "$home/data/backlog.md"
    mx_write_meta "$home/state/$id.meta" \
      "window=broker:mx-$id" \
      "worktree=$home/projects/$id" \
      "project=repo-$i" \
      "harness=codex" \
      "kind=scout" \
      "mode=scout" \
      "pr=https://github.com/acme/repo-$i/pull/$i"
    printf 'needs-decision [key=q%s]: choose %s\n' "$i" "$i" > "$home/state/$id.status"
    i=$((i + 1))
  done
}

test_section_caps_and_expansion_flags() {
  local home fakebin json expanded
  home=$(make_home caps); write_large_fixture "$home" 5
  fakebin=$(make_fakebin "$home")
  json=$(MX_STATUS_IN_FLIGHT=2 MX_STATUS_DECISIONS=2 MX_STATUS_GATES=2 \
    MX_STATUS_REPORTS=2 MX_STATUS_RECORDED_PRS=2 MX_STATUS_UNHEALTHY=2 \
    run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.in_flight|length) == 2 and (.decisions_open|length) == 2 and (.gates|length) == 2
    and (.reports|length) == 2 and (.recorded_prs|length) == 2 and (.unhealthy_endpoints|length) == 2
    and ([.omitted[].surface] | index("in_flight showing 2 of 5") != null)
    and ([.omitted[].surface] | index("decisions_open showing 2 of 5") != null)
    and ([.omitted[].surface] | index("gates showing 2 of 5") != null)
    and ([.omitted[].surface] | index("reports showing 2 of 5") != null)
    and ([.omitted[].surface] | index("recorded_prs showing 2 of 5") != null)
    and ([.omitted[].surface] | index("unhealthy_endpoints showing 2 of 5") != null)
  ' >/dev/null || fail "section caps or counted omissions are wrong: $json"
  expanded=$(MX_STATUS_IN_FLIGHT=2 MX_STATUS_DECISIONS=2 MX_STATUS_GATES=2 \
    MX_STATUS_REPORTS=2 MX_STATUS_RECORDED_PRS=2 MX_STATUS_UNHEALTHY=2 \
    run "$home" "$fakebin" --json --all-in-flight --all-decisions --all-queued \
      --all-reports --all-recorded-prs --all-unhealthy)
  printf '%s' "$expanded" | jq -e '
    (.in_flight|length) == 5 and (.decisions_open|length) == 5 and (.gates|length) == 5
    and (.reports|length) == 5 and (.recorded_prs|length) == 5 and (.unhealthy_endpoints|length) == 5
  ' >/dev/null || fail "section expansion flags did not reveal full sets: $expanded"
  pass "all system-sized sections are capped with counted opt-in expansion"
}

test_pr_repository_cap_and_expansion() {
  local home fakebin json expanded
  home=$(make_home repo-caps); write_large_fixture "$home" 5
  fakebin=$(make_fakebin "$home"); : > "$home/net.log"
  json=$(MX_STATUS_PR_REPOS=2 run "$home" "$fakebin" --include-prs --json)
  [ "$(grep -c '^gh pr list ' "$home/net.log")" = 2 ] || fail "default PR repository cap was not enforced"
  printf '%s' "$json" | jq -e '
    [.omitted[] | select(.surface == "PR repositories showing 2 of 5" and .reveal == "--all-pr-repos")] | length == 1
  ' >/dev/null || fail "PR repository truncation was not recorded: $json"
  : > "$home/net.log"
  expanded=$(MX_STATUS_PR_REPOS=2 run "$home" "$fakebin" --include-prs --all-pr-repos --json)
  [ "$(grep -c '^gh pr list ' "$home/net.log")" = 5 ] || fail "--all-pr-repos did not reveal every repository"
  printf '%s' "$expanded" | jq -e '.candidate_prs | length == 5' >/dev/null \
    || fail "expanded PR repository set did not enrich every repository: $expanded"
  pass "live PR enrichment caps repositories with counted expansion"
}

test_per_repository_pr_cap_is_disclosed() {
  local home fakebin json toon
  home=$(make_home pr-row-cap); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  json=$(MX_STATUS_PR_LIMIT=2 FAKE_GH_MANY=1 run "$home" "$fakebin" --include-prs --json)
  toon=$(MX_STATUS_PR_LIMIT=2 FAKE_GH_MANY=1 run "$home" "$fakebin" --include-prs)
  printf '%s' "$json" | jq -e '
    (.candidate_prs | length) == 2
    and (.prs | test("2 shown, at least 3 open; capped in 1 repo"))
    and ([.omitted[] | select(.surface == "candidate_prs showing 2 of at least 3; capped in 1 repo(s)" and .reveal == "raise MX_STATUS_PR_LIMIT")] | length) == 1
  ' >/dev/null || fail "per-repository PR truncation was not disclosed: $json"
  assert_contains "$toon" 'candidate_prs showing 2 of at least 3' "TOON did not preserve PR truncation disclosure"
  pass "per-repository open-PR caps are disclosed with an expansion knob"
}

install_failing_jq() {  # <fakebin> <model|toon>
  local fakebin=$1 phase=$2 real
  real=$(command -v jq)
  cat > "$fakebin/jq" <<SH
#!/usr/bin/env bash
case "\$*" in
  *'def trunc'*) [ "$phase" = model ] && exit 9 ;;
  *'def q:'*) [ "$phase" = toon ] && exit 9 ;;
esac
exec "$real" "\$@"
SH
  chmod +x "$fakebin/jq"
}

test_projection_and_toon_fail_closed() {
  local home fakebin out err rc
  home=$(make_home fail-closed); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  install_failing_jq "$fakebin" model
  err="$home/model.err"
  out=$(run "$home" "$fakebin" --json 2> "$err"); rc=$?
  [ "$rc" -ne 0 ] || fail "projection failure exited successfully"
  [ -z "$out" ] || fail "projection failure emitted output"
  grep -F 'projection failed' "$err" >/dev/null || fail "projection failure lacked a diagnostic"
  install_failing_jq "$fakebin" toon
  err="$home/toon.err"
  out=$(run "$home" "$fakebin" 2> "$err"); rc=$?
  [ "$rc" -ne 0 ] || fail "TOON rendering failure exited successfully"
  [ -z "$out" ] || fail "TOON rendering failure emitted output"
  grep -F 'TOON rendering failed' "$err" >/dev/null || fail "TOON failure lacked a diagnostic"
  pass "projection and TOON rendering failures exit nonzero with diagnostics"
}

# The Lavish-103 defect, end to end: a COMPLETED scout that raised a decision and
# then finished (done), whose report body reads like that decision, must surface as
# a report POINTER only - never in decisions_open. Report prose must never open or
# reopen a pending decision; only the keyed durable state does.
test_completed_scout_report_not_pending() {
  local home fakebin json
  home=$(make_home completed-scout); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  mkdir -p "$home/projects/lav-wt" "$home/data/lavish-103"
  mx_write_meta "$home/state/lavish-103.meta" \
    "window=broker:mx-lavish-103" \
    "worktree=$home/projects/lav-wt" \
    "project=broker" \
    "harness=codex" \
    "kind=scout" \
    "mode=scout"
  printf 'needs-decision: adopt approach A or B for Lavish issue 103\n' > "$home/state/lavish-103.status"
  printf 'done: report ready at data/lavish-103/report.md\n' >> "$home/state/lavish-103.status"
  printf '# Lavish 103\nThe open question is whether to adopt approach A or B; this needs a maintainer decision.\n' > "$home/data/lavish-103/report.md"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.decisions_open | any(.[]; .id == "lavish-103") | not)
      and (.reports | any(.[]; .id == "lavish-103"))
  ' >/dev/null || fail "completed scout must be a report pointer, never a pending decision: $json"
  pass "a completed scout with decision-like report prose is a pointer, not pending"
}

# Recently Landed must include merges a daemon managed. Those completion records
# live in the daemon home's OWN backlog, not the main one, so the projection must
# roll them up. Local, deterministic, no GitHub call.
test_landed_includes_daemon_home_merges() {
  local home fakebin json
  home=$(make_home mate-landed); write_fixture "$home"
  fakebin=$(make_fakebin "$home"); : > "$home/net.log"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.landed | any(.[]; .id == "mate-landed" and (.artifact | test("/pull/50"))))
      and (.landed | any(.[]; .id == "done-a"))
  ' >/dev/null || fail "landed must merge daemon-home Done with main-home Done: $json"
  # Still zero network on this default path.
  [ ! -s "$home/net.log" ] || fail "landed roll-up must make no gh call, got: $(cat "$home/net.log")"
  pass "landed includes daemon-managed merges alongside main-home merges"
}

test_landed_default_balances_dominant_and_sparse_homes() {
  local home dominant sparse_a sparse_b sparse_c fakebin json i actual expected
  home=$(make_home landed-balanced-default)
  : > "$home/data/daemons.md"
  printf '## Done\n' > "$home/data/backlog.md"
  dominant=$(make_landed_daemon "$home" dominant)
  sparse_a=$(make_landed_daemon "$home" sparse-a)
  sparse_b=$(make_landed_daemon "$home" sparse-b)
  sparse_c=$(make_landed_daemon "$home" sparse-c)
  i=1
  while [ "$i" -le 12 ]; do
    append_landed_row "$dominant" "$(printf 'dominant-landed-%02d' "$i")" \
      "$(printf 'Dominant landed %02d' "$i")" "$(printf '2026-07-%02d' "$((31 - i))")"
    i=$((i + 1))
  done
  i=1
  while [ "$i" -le 2 ]; do
    append_landed_row "$sparse_a" "$(printf 'sparse-a-landed-%02d' "$i")" \
      "$(printf 'Sparse A landed %02d' "$i")" "$(printf '2026-07-%02d' "$((12 - i))")"
    append_landed_row "$sparse_b" "$(printf 'sparse-b-landed-%02d' "$i")" \
      "$(printf 'Sparse B landed %02d' "$i")" "$(printf '2026-07-%02d' "$((10 - i))")"
    append_landed_row "$sparse_c" "$(printf 'sparse-c-landed-%02d' "$i")" \
      "$(printf 'Sparse C landed %02d' "$i")" "$(printf '2026-07-%02d' "$((8 - i))")"
    i=$((i + 1))
  done
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  actual=$(printf '%s' "$json" | jq -r '.landed[] | "\(.owner)/\(.id)"')
  expected='dominant/dominant-landed-01
sparse-a/sparse-a-landed-01
sparse-b/sparse-b-landed-01
sparse-c/sparse-c-landed-01
dominant/dominant-landed-02
sparse-a/sparse-a-landed-02'
  [ "$actual" = "$expected" ] || fail "default landed selection was not balanced across homes: $actual"
  printf '%s' "$json" | jq -e '
    (.landed | length) == 6
      and ([.landed[].owner] | unique | length) == 4
      and ([.omitted[].surface] | any(test("landed showing 6 of 12")))
  ' >/dev/null || fail "balanced landed default did not preserve cap disclosure: $json"
  pass "default landed selection balances one dominant home with sparse homes"
}

test_landed_default_refills_capacity_after_sparse_homes_exhaust() {
  local home dominant sparse fakebin json actual expected i
  home=$(make_home landed-sparse-refill)
  : > "$home/data/daemons.md"
  printf '## Done\n' > "$home/data/backlog.md"
  dominant=$(make_landed_daemon "$home" dominant)
  sparse=$(make_landed_daemon "$home" sparse)
  i=1
  while [ "$i" -le 5 ]; do
    append_landed_row "$dominant" "$(printf 'dominant-landed-%02d' "$i")" \
      "$(printf 'Dominant landed %02d' "$i")" "$(printf '2026-07-%02d' "$((20 - i))")"
    i=$((i + 1))
  done
  append_landed_row "$sparse" sparse-landed-01 "Sparse landed 01" 2026-07-01
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  actual=$(printf '%s' "$json" | jq -r '.landed[] | "\(.owner)/\(.id)"')
  expected='dominant/dominant-landed-01
sparse/sparse-landed-01
dominant/dominant-landed-02
dominant/dominant-landed-03
dominant/dominant-landed-04
dominant/dominant-landed-05'
  [ "$actual" = "$expected" ] || fail "sparse homes wasted landed capacity: $actual"
  pass "landed selection refills capacity after sparse homes exhaust"
}

test_landed_default_uses_deterministic_home_order_when_homes_exceed_cap() {
  local home mate fakebin json actual expected i id
  home=$(make_home landed-home-order)
  : > "$home/data/daemons.md"
  printf '## Done\n' > "$home/data/backlog.md"
  i=1
  while [ "$i" -le 8 ]; do
    id=$(printf 'home-%02d' "$i")
    mate=$(make_landed_daemon "$home" "$id")
    append_landed_row "$mate" "$id-landed-01" "$id landed 01" 2026-07-10
    i=$((i + 1))
  done
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  actual=$(printf '%s' "$json" | jq -r '.landed[] | "\(.owner)/\(.id)"')
  expected='home-01/home-01-landed-01
home-02/home-02-landed-01
home-03/home-03-landed-01
home-04/home-04-landed-01
home-05/home-05-landed-01
home-06/home-06-landed-01'
  [ "$actual" = "$expected" ] || fail "landed home-order tie was not deterministic: $actual"
  printf '%s' "$json" | jq -e '
    ([.omitted[].surface] | any(test("landed showing 6 of 8")))
  ' >/dev/null || fail "more-homes-than-cap omission was not disclosed: $json"
  pass "landed selection uses deterministic home order when homes exceed the cap"
}

test_landed_default_preserves_internal_order_for_ties() {
  local home tie_a tie_b fakebin json actual expected
  home=$(make_home landed-ties)
  : > "$home/data/daemons.md"
  printf '## Done\n' > "$home/data/backlog.md"
  tie_a=$(make_landed_daemon "$home" tie-a)
  tie_b=$(make_landed_daemon "$home" tie-b)
  append_landed_row "$tie_b" tie-b-a "Tie B A" 2026-07-10
  append_landed_row "$tie_b" tie-b-z "Tie B Z" 2026-07-10
  append_landed_row "$tie_a" tie-a-a "Tie A A" 2026-07-10
  append_landed_row "$tie_a" tie-a-z "Tie A Z" 2026-07-10
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  actual=$(printf '%s' "$json" | jq -r '.landed[] | "\(.owner)/\(.id)"')
  expected='tie-a/tie-a-z
tie-b/tie-b-z
tie-a/tie-a-a
tie-b/tie-b-a'
  [ "$actual" = "$expected" ] || fail "landed tie ordering changed: $actual"
  pass "landed selection preserves deterministic home and internal tie ordering"
}

test_landed_default_handles_no_landed_items() {
  local home fakebin json
  home=$(make_home landed-empty)
  : > "$home/data/daemons.md"
  printf '## Done\n' > "$home/data/backlog.md"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.landed | length) == 0
      and ([.omitted[].surface] | any(test("landed")) | not)
  ' >/dev/null || fail "empty landed set was not handled cleanly: $json"
  pass "landed selection handles no landed items"
}

test_all_landed_keeps_complete_global_order() {
  local home alpha beta fakebin json actual expected
  home=$(make_home landed-all-order)
  : > "$home/data/daemons.md"
  printf '## Done\n' > "$home/data/backlog.md"
  alpha=$(make_landed_daemon "$home" alpha)
  beta=$(make_landed_daemon "$home" beta)
  append_landed_row "$alpha" alpha-old "Alpha old" 2026-07-01
  append_landed_row "$alpha" alpha-new "Alpha new" 2026-07-09
  append_landed_row "$beta" beta-new "Beta new" 2026-07-10
  append_landed_row "$beta" beta-mid "Beta mid" 2026-07-05
  fakebin=$(make_fakebin "$home")
  json=$(MX_STATUS_LANDED=1 run "$home" "$fakebin" --json --all-landed)
  actual=$(printf '%s' "$json" | jq -r '.landed[] | "\(.owner)/\(.id)"')
  expected='beta/beta-new
alpha/alpha-new
beta/beta-mid
alpha/alpha-old'
  [ "$actual" = "$expected" ] || fail "--all-landed global order changed: $actual"
  printf '%s' "$json" | jq -e '
    (.landed | length) == 4
      and ([.omitted[].surface] | any(test("landed|snapshot layer")) | not)
  ' >/dev/null || fail "--all-landed no longer revealed the complete landed set: $json"
  pass "--all-landed keeps the complete global landed output"
}

# The roll-up stays bounded: a per-home cap and an overall cap, both disclosed in
# omitted[], with --all-landed as the counted expansion knob. This also covers the
# previously-silent main-home landed truncation.
test_landed_bounded_and_disclosed() {
  local home mate fakebin json i expected actual
  home=$(make_home mate-landed-caps); write_fixture "$home"
  mate=$(fixture_mate_home "$home")
  {
    printf '## In flight\n'
    printf '%s\n\n' '- [ ] mate - Decide subscription order (repo: broker) (kind: delivery) (since 2026-07-11)'
    printf '## Done\n'
  } > "$mate/data/backlog.md"
  i=1
  while [ "$i" -le 12 ]; do
    printf -- '- [x] mate-landed-%02d - Daemon fix %02d (repo: broker) (kind: delivery) (merged 2026-06-%02d)\n' \
      "$i" "$i" "$((13 - i))" >> "$mate/data/backlog.md"
    i=$((i + 1))
  done
  fakebin=$(make_fakebin "$home")
  json=$(MX_STATUS_LANDED=20 run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    ([.landed[].id | select(startswith("mate-landed-"))] | length) == 10
      and ([.omitted[].surface] | any(test("snapshot layer")))
  ' >/dev/null || fail "default landed path must retain and disclose the snapshot per-home cap: $json"
  json=$(MX_STATUS_LANDED=1 run "$home" "$fakebin" --json --all-landed)
  expected=done-a
  i=1
  while [ "$i" -le 12 ]; do
    expected="$expected
$(printf 'mate-landed-%02d' "$i")"
    i=$((i + 1))
  done
  expected=$(printf '%s\n' "$expected" | LC_ALL=C sort)
  actual=$(printf '%s' "$json" | jq -r '.landed[].id' | LC_ALL=C sort)
  [ "$actual" = "$expected" ] || fail "--all-landed returned wrong identities: $actual"
  printf '%s' "$json" | jq -e '
    (.landed | length) == 13
      and ([.omitted[].surface] | any(test("landed|snapshot layer")) | not)
  ' >/dev/null || fail "--all-landed must reveal the exact full landed set: $json"
  pass "landed stays bounded with per-home + overall caps and omitted[] disclosure"
}

# Catchup projects authoritative structured state rather than inventing return
# policy. A live blocked child remains a live in-flight record with state=blocked
# and an open blocker; it must never be converted into a queued `gates` record.
# The return-catch-up owner prevents this state from reaching ordinary rendering
# during an away return, while this test pins Catchup' own projection boundary.
test_live_blocker_is_not_charted_queue_work() {
  local home fakebin json
  home=$(make_home live-blocker); write_fixture "$home"
  printf 'blocked [key=synthetic-dependency]: broker can refresh the synthetic token\n' > "$home/state/delivery-task.status"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.in_flight | any(.[]; .id == "delivery-task" and .state == "blocked"))
      and (.decisions_open | any(.[]; .id == "delivery-task") | not)
      and (.gates | any(.[]; .id == "delivery-task") | not)
  ' >/dev/null || fail "live blocked work was projected as queued/deferred work: $json"
  pass "Catchup keeps a live blocker in structured live state and never converts it to Charted Next queue work"
}

# Maintainer's Call is populated only from the durable keyed open-decision set. The
# anti-leak guard: action-free highlights - a working task, a completed scout,
# queued/gated items, landed work - must never surface as an open decision, so they
# cannot leak into Maintainer's Call. The standard fixture has exactly one genuine open
# decision (the daemon's structured maintainer hold).
test_maintainers_call_anti_leak() {
  local home fakebin json canonical
  home=$(make_home anti-leak); write_fixture "$home"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" "$ROOT/bin/mx-system-snapshot.sh" --json)
  jq -n -e --argjson catchup "$json" --argjson canonical "$canonical" '
    ([$catchup.decisions_open[].id] == ["mate/mate-decision-race"])
      and ($canonical.daemon_current.records[] | select(.id == "mate")
        | (.decisions_open | any(.source == "status"))
          and (.decisions_open | any(.source == "backlog")))
      and ([$catchup.decisions_open[].id] | index("delivery-task") | not)
      and ([$catchup.decisions_open[].id] | index("scout-x") | not)
      and ([$catchup.decisions_open[].id] | index("external-wait") | not)
      and ([$catchup.decisions_open[].id] | index("done-a") | not)
      and ([$catchup.decisions_open[].id] | index("mate-landed") | not)
      and ([$catchup.decisions_open[].id] | index("live-gate") | not)
      and ([$catchup.decisions_open[].id] | index("dead-gate") | not)
  ' >/dev/null || fail "only genuine open decisions may feed Maintainer's Call: $json"
  pass "action-free items (working/done/queued/landed) do not leak into Maintainer's Call"
}

# R1: main-home orphan in-flight and unstructured current rows must not vanish
# silently. Meta remains the sole live-work inventory; disclosure is via
# main_inventory + omitted[] + a Charted Next gate line, never fake Underway.
test_main_orphan_in_flight_is_disclosed_not_invented() {
  local home fakebin json canonical
  home=$(make_home main-orphan)
  : > "$home/data/daemons.md"
  cat > "$home/data/backlog.md" <<'EOF'
## In flight
- [ ] only-orphan - Structured in flight without meta (repo: broker) (kind: delivery) (since 2026-07-11)

## Queued

## Done
EOF
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .main_inventory.valid == false
      and .main_inventory.reason == "in-flight backlog item has no child metadata"
      and (.main_inventory.orphan_in_flight == ["only-orphan"])
      and .main_inventory.unstructured_current_count == 0
      and (.tasks | length) == 0
  ' >/dev/null || fail "canonical main inventory missed orphan: $canonical"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.in_flight | length) == 0
      and ([.in_flight[].id] | index("only-orphan") | not)
      and ([.decisions_open[].id] | index("only-orphan") | not)
      and (.gates | any(.id == "(main-inventory)"
        and (.title | contains("in-flight backlog item has no child metadata"))))
      and (.omitted | any(.surface == "main in-flight backlog item(s) have no child metadata: 1"))
  ' >/dev/null || fail "orphan in-flight was invented or not disclosed: $json"
  pass "main orphan in-flight stays out of Underway and is disclosed in omitted/gates"
}

test_main_unstructured_current_is_disclosed_with_structured_sibling() {
  local home fakebin json canonical
  home=$(make_home main-unstructured)
  : > "$home/data/daemons.md"
  mkdir -p "$home/projects/structured-delivery"
  cat > "$home/data/backlog.md" <<'EOF'
## In flight
this current row is not structured
- [ ] structured-delivery - Visible structured sibling (repo: broker) (kind: delivery) (since 2026-07-11)

## Queued
another free-form note without checkbox
- [ ] structured-queued - Structured queued (repo: broker) (kind: delivery)

## Done
EOF
  mx_write_meta "$home/state/structured-delivery.meta" \
    "window=broker:mx-structured-delivery" \
    "worktree=$home/projects/structured-delivery" \
    "project=broker" \
    "harness=codex" \
    "kind=delivery" \
    "mode=no-mistakes"
  printf 'working: structured sibling still projects\n' > "$home/state/structured-delivery.status"
  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .main_inventory.valid == false
      and .main_inventory.reason == "unstructured current backlog row"
      and .main_inventory.unstructured_current_count == 2
      and (.main_inventory.orphan_in_flight | length) == 0
  ' >/dev/null || fail "canonical main inventory missed unstructured current: $canonical"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    ([.in_flight[].id] == ["structured-delivery"])
      and ([.gates[].id] | index("structured-queued") != null)
      and (.gates | any(.id == "(main-inventory)"
        and (.title | contains("unstructured current backlog row"))))
      and (.omitted | any(.surface == "main unstructured current backlog row(s): 2"))
      and ([.decisions_open[].id] | index("(main-inventory)") | not)
  ' >/dev/null || fail "unstructured current not disclosed or structured sibling lost: $json"
  pass "main unstructured current is disclosed while structured siblings still project"
}

test_main_orphan_counterfactual_meta_clears_inventory_warning() {
  local home fakebin json_before json_after
  home=$(make_home main-orphan-counterfactual)
  : > "$home/data/daemons.md"
  mkdir -p "$home/projects/orphan-delivery"
  cat > "$home/data/backlog.md" <<'EOF'
## In flight
- [ ] orphan-delivery - Gains meta in counterfactual (repo: broker) (kind: delivery) (since 2026-07-11)
- [ ] visible-delivery - Already live (repo: broker) (kind: delivery) (since 2026-07-11)

## Queued
- [ ] queued-delivery - Ordinary queue (repo: broker) (kind: delivery)

## Done
EOF
  mx_write_meta "$home/state/visible-delivery.meta" \
    "window=broker:mx-visible-delivery" \
    "worktree=$home/projects/orphan-delivery" \
    "project=broker" \
    "harness=codex" \
    "kind=delivery" \
    "mode=no-mistakes"
  printf 'working: visible sibling\n' > "$home/state/visible-delivery.status"
  fakebin=$(make_fakebin "$home")
  json_before=$(run "$home" "$fakebin" --json)
  printf '%s' "$json_before" | jq -e '
    ([.in_flight[].id] == ["visible-delivery"])
      and ([.in_flight[].id] | index("orphan-delivery") | not)
      and (.omitted | any(.surface == "main in-flight backlog item(s) have no child metadata: 1"))
      and (.gates | any(.id == "(main-inventory)"))
      and ([.gates[].id] | index("queued-delivery") != null)
  ' >/dev/null || fail "pre-meta orphan fixture failed: $json_before"
  mx_write_meta "$home/state/orphan-delivery.meta" \
    "window=broker:mx-orphan-delivery" \
    "worktree=$home/projects/orphan-delivery" \
    "project=broker" \
    "harness=codex" \
    "kind=delivery" \
    "mode=no-mistakes"
  printf 'working: orphan now has meta\n' > "$home/state/orphan-delivery.status"
  json_after=$(run "$home" "$fakebin" --json)
  printf '%s' "$json_after" | jq -e '
    ([.in_flight[].id] | sort) == ["orphan-delivery", "visible-delivery"]
      and ([.omitted[].surface] | any(test("main in-flight backlog item")) | not)
      and ([.gates[].id] | index("(main-inventory)") | not)
      and ([.decisions_open[].id] | index("orphan-delivery") | not)
  ' >/dev/null || fail "adding meta did not clear inventory warning or project orphan: $json_after"
  pass "counterfactual meta clears main inventory warning and projects the live task"
}

test_mixed_daemon_roles_partial_state_and_maintainer_readiness() {
  local home fakebin hibit wheel sshhip ha canonical json
  home=$(make_home mixed-domain-regressions)
  : > "$home/data/daemons.md"
  hibit="$TMP_ROOT/mixed-hibit-home"
  wheel="$TMP_ROOT/mixed-wheel-home"
  sshhip="$TMP_ROOT/mixed-sshhip-home"
  ha="$TMP_ROOT/mixed-ha-home"
  make_valid_daemon_home hibit "$hibit"
  make_valid_daemon_home wheel "$wheel"
  make_valid_daemon_home sshhip "$sshhip"
  make_valid_daemon_home home-assistant "$ha"
  append_daemon_registry "$home" hibit "$hibit"
  append_daemon_registry "$home" wheel "$wheel"
  append_daemon_registry "$home" sshhip "$sshhip"
  append_daemon_registry "$home" home-assistant "$ha"

  mkdir -p "$hibit/projects/worker" "$wheel/projects/worker" "$sshhip/projects/child" "$ha/projects/prep"
  cat > "$hibit/data/backlog.md" <<'EOF'
## In flight
- [ ] dogfood-program - Long-lived dogfood program (repo: hibit) (kind: program)
- [ ] hibit-worker - Finalize progress (repo: hibit) (kind: delivery)

## Queued

## Done
EOF
  mx_write_meta "$hibit/state/hibit-worker.meta" \
    "window=broker:mx-hibit-worker" "worktree=$hibit/projects/worker" "project=hibit" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'working: finalizing progress\n' > "$hibit/state/hibit-worker.status"

  cat > "$wheel/data/backlog.md" <<'EOF'
## In flight
- [ ] production-observation - Observe production (repo: wheelhouse) (kind: scout) (hold: documented no live worker) (hold-kind: external)
- [ ] wheel-worker - Initial triage (repo: wheelhouse) (kind: delivery)

## Queued

## Done
EOF
  mx_write_meta "$wheel/state/wheel-worker.meta" \
    "window=broker:mx-wheel-worker" "worktree=$wheel/projects/worker" "project=wheelhouse" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'working: active validation\n' > "$wheel/state/wheel-worker.status"

  cat > "$sshhip/data/backlog.md" <<'EOF'
## In flight
- [ ] unreadable-child - Submit App Store build (repo: sshhip) (kind: delivery)

## Queued
- [ ] reviewer-decision - Choose reviewer remediation (repo: sshhip) (kind: maintainer) (hold: choose reviewer remediation A or B) (hold-kind: maintainer)

## Done
- [x] prior-release - Prior release (repo: sshhip) (kind: delivery) (done 2026-07-21)
EOF
  mx_write_meta "$sshhip/state/unreadable-child.meta" \
    "window=broker:dead-sshhip-child" "worktree=$sshhip/projects/child" "project=sshhip" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"

  cat > "$ha/data/backlog.md" <<'EOF'
## In flight
- [ ] prep - Prepare canary (repo: home-assistant) (kind: delivery)

## Queued
- [ ] security - Security review (repo: home-assistant) (kind: delivery)
- [ ] maintainer-run - Run maintainer canary blocked-by: prep blocked-by: security (repo: home-assistant) (kind: maintainer) (hold: maintainer runs canary) (hold-kind: maintainer)

## Done
EOF
  mx_write_meta "$ha/state/prep.meta" \
    "window=broker:mx-prep" "worktree=$ha/projects/prep" "project=home-assistant" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'working: preparing canary\n' > "$ha/state/prep.status"

  fakebin=$(make_fakebin "$home")
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    (.daemon_current.records[] | select(.id == "hibit")
      | .current.state == "active_child_work"
        and [.active_children[].id] == ["hibit-worker"]
        and ([.endpoints[].id] | index("dogfood-program") | not))
      and (.daemon_current.records[] | select(.id == "wheel")
        | .current.state == "active_child_work"
          and [.active_children[].id] == ["wheel-worker"]
          and [.queued[].id] == ["production-observation"]
          and [.holds[].id] == ["production-observation"])
      and (.daemon_current.records[] | select(.id == "sshhip")
        | .current.state == "unknown"
          and (.current.reason | contains("child current state unavailable: unreadable-child"))
          and .provenance.selected == "structured-home"
          and .provenance.summary_valid == false
          and .provenance.trust == "partial-structured"
          and .invalidity == {kind:"child_current_unavailable",ids:["unreadable-child"]}
          and [.decisions_open[].id] == ["reviewer-decision"]
          and [.holds[].id] == ["reviewer-decision"]
          and [.queued[].id] == ["reviewer-decision"]
          and [.landed[].id] == ["prior-release"]
          and [.endpoints[].id] == ["unreadable-child"]
          and .counts.decisions_open == 1
          and .counts.holds == 1
          and .counts.queued == 1
          and .counts.landed == 1
          and .counts.endpoints == 1)
      and (.daemon_landed.partial | length) == 1
      and (.daemon_landed.partial[0] | endswith("/mixed-sshhip-home"))
      and (.daemon_landed.unreadable | length) == 0
      and (.daemon_current.records[] | select(.id == "home-assistant")
        | .current.state == "active_child_work"
          and .decisions_open == []
          and [.active_children[].id] == ["prep"]
          and (.queued[] | select(.id == "maintainer-run")
            | .blocked_by_ids == ["prep", "security"]
              and .unresolved_blocker_ids == ["prep", "security"]
              and .maintainer_actionable == false))
  ' >/dev/null || fail "canonical mixed-domain classification was wrong: $canonical"
  json=$(run "$home" "$fakebin" --json --fields bodies --all-landed)
  printf '%s' "$json" | jq -e '
    ([.in_flight[].id] | sort) == ["hibit", "home-assistant", "wheel"]
      and (.decisions_open | any(.id == "sshhip/reviewer-decision"))
      and (.decisions_open | any(.id == "home-assistant/maintainer-run") | not)
      and (.gates | any(.id == "production-observation" and .owner == "wheel"
        and .reason == "documented no live worker"))
      and (.gates | any(.id == "maintainer-run" and .owner == "home-assistant"
        and .blocked_by == "prep,security"))
      and (.daemons | any(.id == "sshhip" and .state == "unknown"
        and (.reason | contains("unreadable-child"))))
  ' >/dev/null || fail "end-to-end mixed-domain projection was wrong: $json"

  sed '/unreadable-child/a\
- [ ] ordinary-orphan - Unowned release task (repo: sshhip) (kind: delivery)' \
    "$sshhip/data/backlog.md" > "$sshhip/data/backlog.next"
  mv "$sshhip/data/backlog.next" "$sshhip/data/backlog.md"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "sshhip")
    | .current.state == "unknown"
      and (.current.reason | contains("in-flight backlog item has no child metadata: ordinary-orphan"))
      and .provenance.selected != "structured-home"
      and .invalidity == null
      and .active_children == []
      and .decisions_open == []
      and .holds == []
      and .queued == []
      and .landed == []
      and .endpoints == []
  ' >/dev/null || fail "an unknown child masked a simultaneous ordinary orphan: $canonical"
  sed '/ordinary-orphan/d' "$sshhip/data/backlog.md" > "$sshhip/data/backlog.next"
  mv "$sshhip/data/backlog.next" "$sshhip/data/backlog.md"

  sed '/unreadable-child/d' "$sshhip/data/backlog.md" > "$sshhip/data/backlog.next"
  mv "$sshhip/data/backlog.next" "$sshhip/data/backlog.md"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "sshhip")
    | .current.state == "unknown"
      and (.current.reason | contains("live child state has no in-flight backlog item: unreadable-child=unknown"))
      and .provenance.selected != "structured-home"
      and .invalidity == null
      and .active_children == []
      and .decisions_open == []
      and .holds == []
      and .queued == []
      and .landed == []
      and .endpoints == []
  ' >/dev/null || fail "an unowned unknown child received partial structured projection: $canonical"
  sed '/## In flight/a\
- [ ] unreadable-child - Submit App Store build (repo: sshhip) (kind: delivery)' \
    "$sshhip/data/backlog.md" > "$sshhip/data/backlog.next"
  mv "$sshhip/data/backlog.next" "$sshhip/data/backlog.md"

  mx_write_meta "$wheel/state/production-observation.meta" \
    "window=broker:mx-production-observation" "worktree=$wheel/projects/worker" "project=wheelhouse" \
    "harness=codex" "kind=scout" "mode=scout"
  printf 'paused: observation is deliberately held\n' > "$wheel/state/production-observation.status"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "wheel")
    | ([.queued[] | select(.id == "production-observation")] | length) == 1
      and ([.holds[] | select(.id == "production-observation")] | length) == 1
      and ([.endpoints[] | select(.id == "production-observation")] | length) == 1
  ' >/dev/null || fail "held metadata plus a real child duplicated or discarded the record: $canonical"

  mx_write_meta "$sshhip/state/unreadable-child.meta" \
    "window=broker:mx-unreadable-child" "worktree=$sshhip/projects/child" "project=sshhip" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'working: app store submission restored\n' > "$sshhip/state/unreadable-child.status"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.daemons | any(.id == "sshhip" and .state == "maintainer_decision" and .reason == "-"))
      and ([.decisions_open[] | select(.id == "sshhip/reviewer-decision")] | length) == 1
  ' >/dev/null || fail "restoring the SSHHIP child did not clear only its narrow warning: $json"

  cat > "$ha/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] security - Security review (repo: home-assistant) (kind: delivery)
- [ ] maintainer-run - Run maintainer canary blocked-by: prep blocked-by: security (repo: home-assistant) (kind: maintainer) (hold: maintainer runs canary) (hold-kind: maintainer)

## Done
- [x] prep - Prepare canary (repo: home-assistant) (kind: delivery) (done 2026-07-22)
EOF
  rm "$ha/state/prep.meta" "$ha/state/prep.status"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.decisions_open | any(.id == "home-assistant/maintainer-run") | not)
      and (.gates | any(.id == "maintainer-run" and .owner == "home-assistant" and .blocked_by == "security"))
  ' >/dev/null || fail "one remaining Home Assistant blocker became actionable: $json"

  cat > "$ha/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] maintainer-run - Run maintainer canary blocked-by: prep blocked-by: security (repo: home-assistant) (kind: maintainer) (hold: maintainer runs canary) (hold-kind: maintainer)

## Done
- [x] prep - Prepare canary (repo: home-assistant) (kind: delivery) (done 2026-07-22)
- [x] security - Security review (repo: home-assistant) (kind: delivery) (done 2026-07-22)
EOF
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    ([.decisions_open[] | select(.id == "home-assistant/maintainer-run")] | length) == 1
      and (.gates | any(.id == "maintainer-run" and .owner == "home-assistant") | not)
  ' >/dev/null || fail "zero Home Assistant blockers did not yield exactly one maintainer action: $json"

  sed 's/blocked-by: security/blocked-by: missing/' "$ha/data/backlog.md" > "$ha/data/backlog.next"
  mv "$ha/data/backlog.next" "$ha/data/backlog.md"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.decisions_open | any(.id == "home-assistant/maintainer-run") | not)
      and (.gates | any(.id == "maintainer-run" and .owner == "home-assistant" and .blocked_by == "missing"))
  ' >/dev/null || fail "a missing Home Assistant blocker was treated as Done: $json"

  sed 's/(kind: program)/(kind: mystery)/' "$hibit/data/backlog.md" > "$hibit/data/backlog.next"
  mv "$hibit/data/backlog.next" "$hibit/data/backlog.md"
  canonical=$(PATH="$fakebin:$PATH" MX_HOME="$home" MX_SNAPSHOT_NOW=2026-07-11T18:00:00Z \
    "$ROOT/bin/mx-system-snapshot.sh" --json)
  printf '%s' "$canonical" | jq -e '
    .daemon_current.records[] | select(.id == "hibit")
    | .current.state == "unknown"
      and (.current.reason | contains("in-flight backlog item has no child metadata: dogfood-program"))
      and .provenance.selected != "structured-home"
      and .active_children == []
      and .decisions_open == []
      and .holds == []
      and .queued == []
      and .landed == []
      and .endpoints == []
  ' >/dev/null || fail "an unrecognized worker kind no longer stayed strict: $canonical"
  pass "mixed daemon roles, partial state, and maintainer readiness project independently"
}

test_main_maintainer_readiness_matches_daemon_projection() {
  local home fakebin json
  home=$(make_home main-maintainer-readiness)
  : > "$home/data/daemons.md"
  mkdir -p "$home/projects/prep" "$home/projects/observation"
  cat > "$home/data/backlog.md" <<'EOF'
## In flight
- [ ] observation - Held observation (repo: broker) (kind: scout) (hold: watch production) (hold-kind: external)
- [ ] prep - Prepare canary (repo: broker) (kind: delivery)

## Queued
- [ ] review - Security review (repo: broker) (kind: delivery)
- [ ] maintainer-run - Run maintainer canary blocked-by: prep blocked-by: review (repo: broker) (kind: maintainer) (hold: maintainer runs canary) (hold-kind: maintainer)

## Done
EOF
  mx_write_meta "$home/state/prep.meta" \
    "window=broker:mx-prep" "worktree=$home/projects/prep" "project=broker" \
    "harness=codex" "kind=delivery" "mode=no-mistakes"
  printf 'working: preparing main canary\n' > "$home/state/prep.status"
  mx_write_meta "$home/state/observation.meta" \
    "window=broker:mx-observation" "worktree=$home/projects/observation" "project=broker" \
    "harness=codex" "kind=scout" "mode=scout"
  printf 'paused: observation is deliberately held\n' > "$home/state/observation.status"
  fakebin=$(make_fakebin "$home")
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.in_flight | any(.id == "prep"))
      and (.in_flight | any(.id == "observation") | not)
      and ([.gates[] | select(.id == "observation" and .reason == "watch production")] | length) == 1
      and (.decisions_open | any(.id == "maintainer-run") | not)
      and (.gates | any(.id == "maintainer-run" and .blocked_by == "prep,review"))
  ' >/dev/null || fail "main blocked maintainer action or held-child projection was wrong: $json"

  cat > "$home/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] review - Security review (repo: broker) (kind: delivery)
- [ ] maintainer-run - Run maintainer canary blocked-by: prep blocked-by: review (repo: broker) (kind: maintainer) (hold: maintainer runs canary) (hold-kind: maintainer)

## Done
- [x] prep - Prepare canary (repo: broker) (kind: delivery) (done 2026-07-22)
EOF
  rm "$home/state/prep.meta" "$home/state/prep.status" \
    "$home/state/observation.meta" "$home/state/observation.status"
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    (.decisions_open | any(.id == "maintainer-run") | not)
      and (.gates | any(.id == "maintainer-run" and .blocked_by == "review"))
  ' >/dev/null || fail "main one-blocker maintainer action became premature: $json"

  cat > "$home/data/backlog.md" <<'EOF'
## In flight

## Queued
- [ ] maintainer-run - Run maintainer canary blocked-by: prep blocked-by: review (repo: broker) (kind: maintainer) (hold: maintainer runs canary) (hold-kind: maintainer)

## Done
- [x] prep - Prepare canary (repo: broker) (kind: delivery) (done 2026-07-22)
- [x] review - Security review (repo: broker) (kind: delivery) (done 2026-07-22)
EOF
  json=$(run "$home" "$fakebin" --json)
  printf '%s' "$json" | jq -e '
    ([.decisions_open[] | select(.id == "maintainer-run")] | length) == 1
      and (.gates | any(.id == "maintainer-run") | not)
  ' >/dev/null || fail "main zero-blocker maintainer action was not projected exactly once: $json"
  pass "main and daemon maintainer actionability use the same blocker readiness"
}

# The /catchup skill is the one owner of the four-section chat-response contract.
# Assert it states exactly the four fixed sections in order, each with its explicit
# empty-state sentence, documents the At Anchor exclusion, and mandates a chat that is
# materially shorter than and links to the report file.
test_chat_contract_four_sections() {
  local skill body headings report_headings expected
  skill="$ROOT/.agents/skills/catchup/SKILL.md"
  [ -f "$skill" ] || fail "catchup SKILL.md missing at $skill"
  body=$(awk '/^## Chat-response contract$/{capture=1; next} capture && /^## /{exit} capture' "$skill")
  headings=$(printf '%s\n' "$body" | sed -nE "s/^[0-9]+\. \*\*([^*]+)\*\*.*/\1/p")
  expected=$(printf '%s\n' "Maintainer's Call" "Recently Landed" "Underway" "Charted Next")
  [ "$headings" = "$expected" ] || fail "chat contract must contain exactly four numbered sections in fixed order, got: $headings"
  assert_contains "$body" "Nothing needs your action right now" "Maintainer's Call empty-state sentence"
  assert_contains "$body" "No recent completions are in the current baseline" "Recently Landed empty-state sentence"
  assert_contains "$body" "Nothing is underway" "Underway empty-state sentence"
  assert_contains "$body" "Nothing is queued" "Charted Next empty-state sentence"
  report_headings=$(sed -nE 's/^   - \*\*(Maintainer.s Call|Recently Landed|Underway|Charted Next)\*\*.*/\1/p' "$skill")
  [ "$report_headings" = "$expected" ] || fail "detailed report contract must contain the same four complete sections, got: $report_headings"
  grep -Eq 'since the (prior|last) report|Nothing has landed since|unchanged delta' "$skill" \
    && fail "catchup contract still contains prior-report delta wording"
  # shellcheck disable=SC2016 # Backticks are literal Markdown in the expected text.
  assert_contains "$(cat "$skill")" 'Never read an earlier `data/status-report-*.md`' "prior reports must not influence current output"
  assert_contains "$(cat "$skill")" "bounded current recent-completions baseline" "Recently Landed must be a current baseline"
  assert_contains "$body" "no At Anchor section" "the At Anchor exclusion must be documented"
  assert_contains "$body" "materially shorter" "the chat must be materially shorter than the report file"
  assert_contains "$body" "links to" "the chat must link to the report file"
  pass "the /catchup skill states the four-section chat contract in order, with empty-states and the At Anchor exclusion"
}

case "${MX_TEST_CASE_GROUP:-all}" in
  projection-reconciliation)
    test_domain_alpha_stale_parent_event_does_not_become_current_work
    test_gnu_stat_uses_file_formats_without_bsd_fallback_pollution
    test_parent_activity_evidence_is_bounded_and_disclosed
    test_active_child_overrides_old_parent_event
    test_structured_child_decision_reaches_maintainers_call
    test_bad_daemon_homes_never_revive_parent_work
    test_oversized_daemon_summary_stays_strict_unknown
    test_daemon_and_child_bounds_are_disclosed
    test_parent_decision_is_untrusted_contradiction_only
    test_parent_evidence_reconciles_by_verb_and_key
    test_nonprogressing_child_states_are_explicit
    test_registry_unavailability_and_bounds_are_explicit
    test_live_blocker_is_not_charted_queue_work
    test_maintainers_call_anti_leak
    test_main_orphan_in_flight_is_disclosed_not_invented
    test_main_unstructured_current_is_disclosed_with_structured_sibling
    test_main_orphan_counterfactual_meta_clears_inventory_warning
    test_mixed_daemon_roles_partial_state_and_maintainer_readiness
    test_main_maintainer_readiness_matches_daemon_projection
    ;;
  landed-bounds)
    test_current_landed_baseline_is_repeatable_and_prior_report_independent
    test_default_is_bounded_and_local_only
    test_toon_json_parity
    test_landed_includes_daemon_home_merges
    test_landed_default_balances_dominant_and_sparse_homes
    test_landed_default_refills_capacity_after_sparse_homes_exhaust
    test_landed_default_uses_deterministic_home_order_when_homes_exceed_cap
    test_landed_default_preserves_internal_order_for_ties
    test_landed_default_handles_no_landed_items
    test_all_landed_keeps_complete_global_order
    test_landed_bounded_and_disclosed
    test_section_caps_and_expansion_flags
    ;;
  catchup-forge)
    test_chat_contract_four_sections
    test_completed_scout_report_not_pending
    test_open_decision_surfaces_end_to_end
    test_report_pointers_surface
    test_superseded_queued_item_dropped_by_default
    test_include_prs_is_the_only_fetch_path
    test_partial_github_failure_degrades
    test_perl_fallback_bounds_github_call
    test_pr_repository_cap_and_expansion
    test_per_repository_pr_cap_is_disclosed
    test_projection_and_toon_fail_closed
    ;;
  all)
    test_domain_alpha_stale_parent_event_does_not_become_current_work
    test_gnu_stat_uses_file_formats_without_bsd_fallback_pollution
    test_parent_activity_evidence_is_bounded_and_disclosed
    test_active_child_overrides_old_parent_event
    test_structured_child_decision_reaches_maintainers_call
    test_bad_daemon_homes_never_revive_parent_work
    test_oversized_daemon_summary_stays_strict_unknown
    test_daemon_and_child_bounds_are_disclosed
    test_parent_decision_is_untrusted_contradiction_only
    test_parent_evidence_reconciles_by_verb_and_key
    test_nonprogressing_child_states_are_explicit
    test_registry_unavailability_and_bounds_are_explicit
    test_current_landed_baseline_is_repeatable_and_prior_report_independent
    test_default_is_bounded_and_local_only
    test_toon_json_parity
    test_landed_includes_daemon_home_merges
    test_landed_default_balances_dominant_and_sparse_homes
    test_landed_default_refills_capacity_after_sparse_homes_exhaust
    test_landed_default_uses_deterministic_home_order_when_homes_exceed_cap
    test_landed_default_preserves_internal_order_for_ties
    test_landed_default_handles_no_landed_items
    test_all_landed_keeps_complete_global_order
    test_landed_bounded_and_disclosed
    test_live_blocker_is_not_charted_queue_work
    test_maintainers_call_anti_leak
    test_main_orphan_in_flight_is_disclosed_not_invented
    test_main_unstructured_current_is_disclosed_with_structured_sibling
    test_main_orphan_counterfactual_meta_clears_inventory_warning
    test_mixed_daemon_roles_partial_state_and_maintainer_readiness
    test_main_maintainer_readiness_matches_daemon_projection
    test_chat_contract_four_sections
    test_completed_scout_report_not_pending
    test_open_decision_surfaces_end_to_end
    test_report_pointers_surface
    test_superseded_queued_item_dropped_by_default
    test_include_prs_is_the_only_fetch_path
    test_partial_github_failure_degrades
    test_perl_fallback_bounds_github_call
    test_section_caps_and_expansion_flags
    test_pr_repository_cap_and_expansion
    test_per_repository_pr_cap_is_disclosed
    test_projection_and_toon_fail_closed
    ;;
  *)
    fail "unknown status snapshot case group: ${MX_TEST_CASE_GROUP}"
    ;;
esac
