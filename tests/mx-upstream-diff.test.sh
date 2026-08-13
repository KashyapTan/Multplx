#!/usr/bin/env bash
# Behavior coverage for relevance filtering, report stability, write boundaries,
# review-cursor transitions, retirement, and the tracked workflow definition.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SCRIPT="$ROOT/bin/mx-upstream-diff.sh"
RECORD_TEMPLATE="$ROOT/tests/fixtures/upstream-sync/record.md"
REPORT_GOLDEN="$ROOT/tests/fixtures/upstream-sync/report.golden.md"
mx_test_tmproot_into TMP_ROOT mx-upstream-diff

UPSTREAM_REPO="$TMP_ROOT/upstream"
CONTROL_DIR="$TMP_ROOT/control"
RECORD_FILE="$CONTROL_DIR/upstream.md"
OUTPUT_DIR="$TMP_ROOT/output"
mkdir -p "$UPSTREAM_REPO" "$CONTROL_DIR"

mx_git_identity "Upstream Fixture" "upstream@example.invalid"
git -C "$UPSTREAM_REPO" init -q -b main
git -C "$UPSTREAM_REPO" config user.name "Upstream Fixture"
git -C "$UPSTREAM_REPO" config user.email "upstream@example.invalid"

fixture_commit() { # <timestamp> <subject>
  local timestamp=$1 subject=$2
  git -C "$UPSTREAM_REPO" add -A
  GIT_AUTHOR_DATE="$timestamp" GIT_COMMITTER_DATE="$timestamp" \
    git -C "$UPSTREAM_REPO" commit -qm "$subject"
  git -C "$UPSTREAM_REPO" rev-parse HEAD
}

mkdir -p "$UPSTREAM_REPO/bin" "$UPSTREAM_REPO/docs"
printf 'fixture upstream\n' >"$UPSTREAM_REPO/README.md"
BASE_SHA=$(fixture_commit "2026-07-01T00:00:00Z" "base")

printf 'watch v1\n' >"$UPSTREAM_REPO/bin/fm-watch.sh"
RELEVANT_SHA=$(fixture_commit "2026-07-01T00:00:01Z" "fix watch race")

printf 'removed relay\n' >"$UPSTREAM_REPO/bin/fm-x-poll.sh"
DELETED_SHA=$(fixture_commit "2026-07-01T00:00:02Z" "change removed relay")

printf 'release notes\n' >"$UPSTREAM_REPO/docs/release-notes.md"
IRRELEVANT_SHA=$(fixture_commit "2026-07-01T00:00:03Z" "update release notes")

printf 'removed provider\n' >"$UPSTREAM_REPO/bin/fm-pr-glab.sh"
GLAB_SHA=$(fixture_commit "2026-07-01T00:00:04Z" "fix removed provider")

printf 'unmapped\n' >"$UPSTREAM_REPO/docs/new-area.md"
FLAGGED_SHA=$(fixture_commit "2026-07-01T00:00:05Z" "add unmapped area")
HEAD_SHA=$FLAGGED_SHA
UPSTREAM_URL="file://$UPSTREAM_REPO"

node - "$RECORD_TEMPLATE" "$RECORD_FILE" \
  "$UPSTREAM_URL" "$BASE_SHA" <<'NODE'
const fs = require('fs');
const [source, destination, repo, fork] = process.argv.slice(2);
const rendered = fs.readFileSync(source, 'utf8')
  .replace('@UPSTREAM_REPO@', repo)
  .replace('@FORK_POINT@', fork)
  .replace('@LAST_REVIEWED@', fork);
fs.writeFileSync(destination, rendered);
NODE

run_diff() {
  MX_UPSTREAM_RECORD_FILE="$RECORD_FILE" "$SCRIPT" "$@"
}

tree_snapshot() { # <root>
  local root=$1 path
  find "$root" -type f -print | LC_ALL=C sort | while IFS= read -r path; do
    printf '%s  %s\n' "$(shasum -a 256 "$path" | awk '{print $1}')" \
      "${path#"$root"/}"
  done
}

assert_contains_file() { # <file> <literal> <message>
  grep -F -- "$2" "$1" >/dev/null 2>&1 || fail "$3"
}

test_status_and_read_only_report() {
  local before="$TMP_ROOT/control.before" after="$TMP_ROOT/control.after" output
  tree_snapshot "$CONTROL_DIR" >"$before"
  output=$(run_diff --status) || fail "active status failed"
  assert_contains "$output" "status=active" "active status was not reported"
  output=$(run_diff --out "$OUTPUT_DIR") || fail "fixture report run failed"
  assert_contains "$output" "head=$HEAD_SHA" "report did not expose fixture HEAD"
  tree_snapshot "$CONTROL_DIR" >"$after"
  cmp -s "$before" "$after" \
    || fail "plain report run wrote outside its output directory"
  [ "$(cat "$OUTPUT_DIR/head-sha")" = "$HEAD_SHA" ] \
    || fail "head-sha does not contain exact upstream HEAD"
  [ "$(git -C "$OUTPUT_DIR/.upstream" remote)" = upstream ] \
    || fail "scratch clone has an unexpected remote"
  [ "$(git -C "$OUTPUT_DIR/.upstream" remote get-url --push upstream)" = /dev/null ] \
    || fail "scratch clone retained a usable push URL"
  ! git -C "$OUTPUT_DIR/.upstream" push --dry-run upstream >/dev/null 2>&1 \
    || fail "scratch clone accepted a push attempt"
  pass "status and report runs preserve the record and configure a fetch-only scratch clone"
}

test_filtering_and_golden_report() {
  local report="$OUTPUT_DIR/report-input.md" expected="$TMP_ROOT/report.expected.md"
  assert_contains_file "$report" "Relevant commits: 1" \
    "relevant commit count is wrong"
  assert_contains_file "$report" "Flagged commits: 1" \
    "flagged commit count is wrong"
  assert_contains_file "$report" "Mechanically skipped commits: 3" \
    "mechanical skip count is wrong"
  assert_contains_file "$report" "docs/new-area.md" \
    "unmapped path did not reach needs-mapping output"
  assert_contains_file "$report" "diff --git a/bin/fm-watch.sh b/bin/fm-watch.sh" \
    "relevant full diff is missing"
  ! grep -F "diff --git a/bin/fm-x-poll.sh" "$report" >/dev/null 2>&1 \
    || fail "deleted-only commit appeared in a full-diff section"
  assert_contains_file "$report" "\`$(printf '%.12s' "$DELETED_SHA")\` change removed relay" \
    "deleted-only commit is absent from the mechanical-skip appendix"
  assert_contains_file "$report" "\`$(printf '%.12s' "$GLAB_SHA")\` fix removed provider" \
    "provider-only deleted path was not skipped mechanically"

  UPSTREAM_URL="$UPSTREAM_URL" BASE_SHA="$BASE_SHA" HEAD_SHA="$HEAD_SHA" \
  RELEVANT_SHA="$RELEVANT_SHA" DELETED_SHA="$DELETED_SHA" \
  IRRELEVANT_SHA="$IRRELEVANT_SHA" GLAB_SHA="$GLAB_SHA" \
  FLAGGED_SHA="$FLAGGED_SHA" \
    node - "$REPORT_GOLDEN" "$expected" <<'NODE'
const fs = require('fs');
const [source, destination] = process.argv.slice(2);
let rendered = fs.readFileSync(source, 'utf8');
for (const key of [
  'UPSTREAM_URL', 'BASE_SHA', 'HEAD_SHA', 'RELEVANT_SHA', 'DELETED_SHA',
  'IRRELEVANT_SHA', 'GLAB_SHA', 'FLAGGED_SHA'
]) {
  rendered = rendered.replaceAll(`@${key}@`, process.env[key]);
  rendered = rendered.replaceAll(`@${key}_SHORT@`, process.env[key].slice(0, 12));
}
fs.writeFileSync(destination, rendered);
NODE
  if ! cmp -s "$expected" "$report"; then
    diff -u "$expected" "$report" >&2 || true
    fail "report-input.md changed from the golden fixture"
  fi
  pass "path classes produce the byte-stable filtered report"
}

test_record_transitions_and_retirement() {
  local output rc cursor_hash before="$TMP_ROOT/retired.before" after="$TMP_ROOT/retired.after"
  output=$(MX_UPSTREAM_REVIEW_DATE=2026-07-31 \
    run_diff --record-reviewed "$OUTPUT_DIR/head-sha") \
    || fail "forward record update failed"
  assert_contains "$output" "last_reviewed=$HEAD_SHA" \
    "record update did not print the new cursor"
  [ "$(awk '/^last_reviewed:/ {print $2}' "$RECORD_FILE")" = "$HEAD_SHA" ] \
    || fail "record cursor did not advance to fixture HEAD"
  assert_contains_file "$RECORD_FILE" \
    "- 2026-07-31: reviewed through \`$HEAD_SHA\` via the upstream-sync workflow." \
    "completed review log did not receive the cursor advancement"
  [ "$(grep -c 'reviewed through' "$RECORD_FILE")" -eq 1 ] \
    || fail "cursor advancement wrote duplicate completed-review entries"
  cursor_hash=$(shasum -a 256 "$RECORD_FILE" | awk '{print $1}')
  output=$(run_diff --record-reviewed "$OUTPUT_DIR/head-sha") \
    || fail "idempotent record retry failed"
  assert_contains "$output" "unchanged=true" \
    "idempotent record retry did not report its no-op"
  [ "$(shasum -a 256 "$RECORD_FILE" | awk '{print $1}')" = "$cursor_hash" ] \
    || fail "idempotent record retry rewrote the record"

  mkdir "$CONTROL_DIR/.upstream-record.lock"
  printf '%s\n' "$$" >"$CONTROL_DIR/.upstream-record.lock/pid"
  output=$(run_diff --record-reviewed "$HEAD_SHA" 2>&1)
  rc=$?
  [ "$rc" -ne 0 ] || fail "concurrent record update was accepted"
  assert_contains "$output" "record update is already running" \
    "concurrent record refusal was unclear"
  rm -f "$CONTROL_DIR/.upstream-record.lock/pid"
  rmdir "$CONTROL_DIR/.upstream-record.lock"

  output=$(run_diff --record-reviewed "$BASE_SHA" 2>&1)
  rc=$?
  [ "$rc" -ne 0 ] || fail "backward record movement was accepted"
  assert_contains "$output" "refusing to move last_reviewed backwards" \
    "backward refusal was unclear"

  git -C "$UPSTREAM_REPO" checkout -q --orphan unrelated
  git -C "$UPSTREAM_REPO" rm -qrf .
  printf 'unrelated\n' >"$UPSTREAM_REPO/unrelated"
  UNRELATED_SHA=$(fixture_commit "2026-07-01T00:00:06Z" "unrelated")
  git -C "$UPSTREAM_REPO" checkout -q main
  output=$(run_diff --record-reviewed "$UNRELATED_SHA" 2>&1)
  rc=$?
  [ "$rc" -ne 0 ] || fail "unrelated record movement was accepted"
  assert_contains "$output" "refusing to move last_reviewed backwards" \
    "unrelated refusal was unclear"

  sed -e 's/^status: active$/status: retired/' \
    -e 's/^retired_reason:$/retired_reason: fixture retirement/' \
    "$RECORD_FILE" >"$TMP_ROOT/retired.md"
  mv "$TMP_ROOT/retired.md" "$RECORD_FILE"
  tree_snapshot "$CONTROL_DIR" >"$before"
  output=$(run_diff --status 2>&1)
  rc=$?
  [ "$rc" -eq 3 ] || fail "retired status did not use exit 3"
  assert_contains "$output" "retired_reason=fixture retirement" \
    "retired status omitted its reason"
  output=$(run_diff --out "$TMP_ROOT/retired-output" 2>&1)
  rc=$?
  [ "$rc" -eq 3 ] || fail "retired report run did not use exit 3"
  [ ! -e "$TMP_ROOT/retired-output" ] \
    || fail "retired report run touched its output path"
  output=$(run_diff --record-reviewed "$HEAD_SHA" 2>&1)
  rc=$?
  [ "$rc" -eq 3 ] || fail "retired record update did not use exit 3"
  tree_snapshot "$CONTROL_DIR" >"$after"
  cmp -s "$before" "$after" || fail "retired commands changed the record"

  sed -e 's/^status: retired$/status: active/' \
    -e 's/^retired_reason: fixture retirement$/retired_reason:/' \
    "$RECORD_FILE" >"$TMP_ROOT/active.md"
  mv "$TMP_ROOT/active.md" "$RECORD_FILE"
  run_diff --out "$TMP_ROOT/resumed-output" >/dev/null \
    || fail "reactivated report run failed"
  assert_contains_file "$TMP_ROOT/resumed-output/report-input.md" "Commits: 0" \
    "reactivation did not resume from the preserved cursor"
  pass "cursor updates are forward-only and retirement is a reversible fail-closed state"
}

test_workflow_definition_contract() {
  local definition="$ROOT/workflows/upstream-sync.workflow.md" normalized
  normalized=$("$ROOT/bin/mx-workflow.sh" validate "$definition") \
    || fail "upstream-sync workflow does not validate"
  assert_contains "$normalized" "valid: upstream-sync (6 stages)" \
    "workflow validation returned the wrong definition"
  normalized=$("$ROOT/target/release/mx" authority mx-workflow.sh parse-json "$definition") \
    || fail "workflow normalization failed"
  jq -e '
    ([.stages[] | select(.gate == "auto") |
      (.type == "command" or (.output | type == "string"))] | all) and
    ([.stages[] | .id] ==
      ["fetch","triage","review","port","record","advance"]) and
    (.stages[] | select(.id == "record") |
      .type == "interactive" and .gate == "approve") and
    (.stages[] | select(.id == "advance") |
      .type == "command" and .gate == "auto")
  ' <<<"$normalized" >/dev/null \
    || fail "workflow gate or contract choices changed after normalization"
  pass "tracked workflow preserves deterministic contracts and approval-before-advance"
}

test_status_and_read_only_report
test_filtering_and_golden_report
test_record_transitions_and_retirement
test_workflow_definition_contract
