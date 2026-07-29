#!/usr/bin/env bash
# Owned backlog parser, query, mutation, retention, and transaction coverage.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

BACKLOG="$ROOT/bin/mx-backlog.sh"
TMP_ROOT=$(mx_test_tmproot mx-backlog-lib)

scaffold() {
  local file=$1
  mkdir -p "$(dirname "$file")"
  printf '## In flight\n\n## Queued\n\n## Done\n' > "$file"
}

test_list_show_add_hold_ready_and_unblock() {
  local file="$TMP_ROOT/crud/backlog.md" out
  scaffold "$file"
  "$BACKLOG" add active "Active work" --file "$file" --repo broker --kind delivery --start \
    --body $'First paragraph.\n\n## Intent\nBody survives.' >/dev/null
  "$BACKLOG" add blocker "Finished prerequisite" --file "$file" --repo broker --kind delivery >/dev/null
  "$BACKLOG" done blocker --file "$file" --note ready >/dev/null
  "$BACKLOG" add dependent "Dependent work" --file "$file" --repo broker --kind scout \
    --blocked-by blocker >/dev/null
  "$BACKLOG" add held "Maintainer choice" --file "$file" --repo broker --kind maintainer >/dev/null
  "$BACKLOG" hold held --file "$file" --reason "choose route" --kind maintainer >/dev/null

  out=$("$BACKLOG" list --file "$file" --limit 2)
  assert_contains "$out" 'tasks[2]{id,state,kind,repo,title,blocked_by,hold_kind,hold_reason}:' \
    "list did not emit the compact field contract"
  assert_contains "$out" 'active,in_flight,delivery,broker,Active work,none,-,-' \
    "list omitted the in-flight identity fields"
  assert_contains "$out" '(truncated 2 item(s))' "list limit did not report truncation"
  assert_not_contains "$out" 'First paragraph' "compact list leaked a body"

  out=$("$BACKLOG" show active --file "$file" --full)
  assert_contains "$out" 'state: in_flight' "show lost state"
  assert_contains "$out" 'body: "First paragraph.\n\n## Intent\nBody survives."' \
    "show did not preserve the full body including an indented pseudo-heading"

  out=$("$BACKLOG" ready --file "$file")
  assert_contains "$out" dependent "ready omitted a queued task whose blocker is Done"
  assert_not_contains "$out" held "ready included a maintainer-held item"
  "$BACKLOG" unblock dependent --file "$file" --by blocker >/dev/null
  out=$("$BACKLOG" show dependent --file "$file")
  assert_contains "$out" 'blocked_by: ' "unblock did not clear the structured edge"

  pass "backlog list/show/add/hold/ready/unblock preserve the owned format"
}

test_done_artifacts_retention_and_archive() {
  local file="$TMP_ROOT/done/backlog.md" archive="$TMP_ROOT/done/done-archive.md"
  scaffold "$file"
  "$BACKLOG" add report-item "Report item" --file "$file" --body "report body" >/dev/null
  "$BACKLOG" add note-item "Note item" --file "$file" >/dev/null
  "$BACKLOG" add pr-item "PR item" --file "$file" >/dev/null
  MX_BACKLOG_DONE_KEEP=2 "$BACKLOG" done report-item --file "$file" --report data/report.md >/dev/null
  MX_BACKLOG_DONE_KEEP=2 "$BACKLOG" done note-item --file "$file" --note "local main" >/dev/null
  MX_BACKLOG_DONE_KEEP=2 "$BACKLOG" done pr-item --file "$file" --pr https://github.com/o/r/pull/1 >/dev/null

  assert_grep '(note: local main)' "$file" "done --note did not record its artifact"
  assert_grep '(pr: https://github.com/o/r/pull/1)' "$file" "done --pr did not record its artifact"
  assert_no_grep 'report-item' "$file" "done retention kept more than two recent items"
  assert_grep 'report-item' "$archive" "done retention did not archive the overflow item"
  assert_grep '(report: data/report.md)' "$archive" "archived Done block lost the report artifact"
  assert_grep '  report body' "$archive" "archived Done block lost its body"

  pass "done records every artifact kind and archives retention overflow"
}

test_update_archive_body_recoverability() {
  local file="$TMP_ROOT/update/backlog.md" archive="$TMP_ROOT/update/done-archive.md"
  local body_file="$TMP_ROOT/update/replacement.txt" out
  scaffold "$file"
  "$BACKLOG" add note "Considered note" --file "$file" --body $'Old body.\nSecond line.' >/dev/null
  printf '%s\n' "Replacement body." > "$body_file"
  "$BACKLOG" update note --file "$file" --body-file "$body_file" --archive-body >/dev/null

  out=$("$BACKLOG" show note --file "$file")
  assert_contains "$out" 'body: "Replacement body."' "update did not replace the considered body"
  assert_grep 'Superseded body: note' "$archive" "update --archive-body omitted archive identity"
  assert_grep '  Old body.' "$archive" "update --archive-body lost the old body"
  assert_grep '  Second line.' "$archive" "update --archive-body lost a multiline body"

  pass "update --archive-body keeps superseded notes recoverable"
}

test_atomic_connected_move_and_strand_refusal() {
  local source="$TMP_ROOT/mv/source.md" destination="$TMP_ROOT/mv/destination.md"
  local source_before destination_before out
  scaffold "$source"
  scaffold "$destination"
  "$BACKLOG" add blocker "Blocker" --file "$source" --body $'Paragraph one.\n\n## Intent\nKeep all body lines.' >/dev/null
  "$BACKLOG" add dependent "Dependent" --file "$source" --blocked-by blocker >/dev/null
  "$BACKLOG" add resident "Resident" --file "$destination" >/dev/null
  source_before=$(cat "$source")
  destination_before=$(cat "$destination")

  if out=$("$BACKLOG" mv blocker --file "$source" --to "$destination" 2>&1); then
    fail "move accepted a blocker while leaving its dependent behind"
  fi
  assert_contains "$out" 'strand dependent dependent' "strand refusal did not identify the dependent"
  [ "$source_before" = "$(cat "$source")" ] || fail "strand refusal changed the source"
  [ "$destination_before" = "$(cat "$destination")" ] || fail "strand refusal changed the destination"

  "$BACKLOG" mv blocker dependent --file "$source" --to "$destination" >/dev/null
  assert_no_grep 'blocker' "$source" "connected move left blocker in source"
  assert_no_grep 'dependent' "$source" "connected move left dependent in source"
  assert_grep 'blocked-by: blocker' "$destination" "connected move lost dependency metadata"
  assert_grep '  ## Intent' "$destination" "move treated an indented pseudo-heading as a section"
  assert_grep 'Keep all body lines.' "$destination" "move orphaned a post-blank body line"
  assert_grep 'resident' "$destination" "move disturbed an existing destination item"

  pass "multi-ID move is all-or-nothing and refuses stranded dependencies"
}

test_malformed_inputs_refuse_without_writes() {
  local dir="$TMP_ROOT/malformed" file out before
  mkdir -p "$dir"

  printf '## Queued\n\n## Done\n' > "$dir/missing.md"
  if out=$("$BACKLOG" validate --file "$dir/missing.md" 2>&1); then
    fail "validator accepted a missing section"
  fi
  assert_contains "$out" 'missing backlog section "## In flight"' "missing-section diagnostic was vague"

  printf '## In flight\n\n## Queued\n- [ ] tabs - Invalid\n\tbody\n\n## Done\n' > "$dir/tab.md"
  if out=$("$BACKLOG" validate --file "$dir/tab.md" 2>&1); then
    fail "validator accepted a tab-indented body"
  fi
  assert_contains "$out" 'non-2-space continuation' "tab-body diagnostic was vague"

  printf '## In flight\n\n## Queued\n- [ ] duplicate - One\n- [ ] duplicate - Two\n\n## Done\n' > "$dir/duplicate.md"
  if out=$("$BACKLOG" validate --file "$dir/duplicate.md" 2>&1); then
    fail "validator accepted duplicate ids"
  fi
  assert_contains "$out" 'duplicate backlog item id' "duplicate-id diagnostic was vague"

  printf '## In flight\n\n## Queued\n- [ ] truncated - Item\nunindented fragment\n\n## Done\n' > "$dir/truncated.md"
  before=$(cat "$dir/truncated.md")
  if out=$("$BACKLOG" done truncated --file "$dir/truncated.md" 2>&1); then
    fail "mutation accepted truncated item content"
  fi
  assert_contains "$out" 'non-2-space continuation' "truncated diagnostic was vague"
  [ "$before" = "$(cat "$dir/truncated.md")" ] || fail "malformed mutation changed the file"

  pass "missing sections, tabs, duplicates, and truncated blocks refuse safely"
}

test_list_show_add_hold_ready_and_unblock
test_done_artifacts_retention_and_archive
test_update_archive_body_recoverability
test_atomic_connected_move_and_strand_refusal
test_malformed_inputs_refuse_without_writes

echo "ALL TESTS PASSED"
