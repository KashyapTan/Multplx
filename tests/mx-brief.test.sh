#!/usr/bin/env bash
# Lean assignment scaffolds, retained isolation, status and compatibility contracts.
# shellcheck disable=SC2016
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
TMP_ROOT=$(mx_test_tmproot mx-brief)

test_script_parses() {
  local out rc
  out=$(bash -n "$ROOT/bin/mx-brief.sh" 2>&1); rc=$?
  expect_code 0 "$rc" "bash -n bin/mx-brief.sh must parse cleanly (got: $out)"
  [ -z "$out" ] || fail "bash -n bin/mx-brief.sh emitted unexpected output: $out"
  pass "mx-brief.sh: bash -n succeeds"
}

test_help_includes_entire_header() {
  local help
  help=$("$ROOT/bin/mx-brief.sh" --help)
  assert_contains "$help" "Refuses to overwrite an existing brief." "mx-brief.sh --help omitted its header terminator"
  pass "mx-brief.sh: --help renders the complete header"
}

write_registry() {
  local home=$1
  mkdir -p "$home/data"
  cat > "$home/data/projects.md" <<'EOF'
- direct-proj [direct-PR] - fixture for direct-PR mode (added 2026-07-01)
- local-proj [local-only] - fixture for local-only mode (added 2026-07-01)
EOF
}

test_herdr_lab_contract_is_explicit_and_complete() {
  local home id brief
  home="$TMP_ROOT/herdr-lab-home"
  mkdir -p "$home/data"
  id="brief-herdr-lab-d1"
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" broker --herdr-lab >/dev/null 2>&1
  brief="$home/data/$id/brief.md"
  assert_present "$brief" "Herdr lab brief was not scaffolded"
  assert_grep "# Herdr isolation - HARD SAFETY CONTRACT" "$brief" \
    "Herdr lab brief missing its hard safety contract"
  assert_grep "HERDR_LAB_HELPER='$ROOT/bin/mx-herdr-lab.sh'" "$brief" \
    "Herdr lab brief must bind the absolute Multplx helper path"
  assert_grep "HERDR_LAB_SESSION=\$(\"\$HERDR_LAB_HELPER\" name $id)" "$brief" \
    "Herdr lab brief missing helper-owned session naming"
  assert_grep "\"\$HERDR_LAB_HELPER\" provision \"\$HERDR_LAB_SESSION\"" "$brief" \
    "Herdr lab brief missing helper-owned provisioning"
  assert_grep "\"\$HERDR_LAB_HELPER\" teardown \"\$HERDR_LAB_SESSION\"" "$brief" \
    "Herdr lab brief missing helper-owned teardown"
  assert_grep "required trailing \`--session \"\$HERDR_LAB_SESSION\"\`" "$brief" \
    "Herdr lab brief missing the per-call trailing session contract"
  assert_grep "direct \`herdr server stop\`" "$brief" \
    "Herdr lab brief missing the forbidden server-global command list"
  assert_grep "records the live default session before provisioning" "$brief" \
    "Herdr lab brief missing the before tripwire"
  assert_grep "verifies the identical system state after teardown" "$brief" \
    "Herdr lab brief missing the after tripwire"
  assert_no_grep "Herdr lifecycle declaration - NOT ENABLED" "$brief" \
    "Herdr lab brief retained the unguarded declaration"
  pass "mx-brief.sh: --herdr-lab emits the complete hard safety contract"
}

test_herdr_lab_contract_quotes_foreign_broker_path() {
  local home id brief foreign_root helper
  home="$TMP_ROOT/herdr-lab-foreign-home"
  foreign_root="$TMP_ROOT/broker helper's root"
  mkdir -p "$home/data"
  id="brief-herdr-lab-foreign-d2"
  helper=$(printf '%s' "$foreign_root/bin/mx-herdr-lab.sh" | sed "s/'/'\\\\''/g")
  helper="'$helper'"
  MX_HOME="$home" MX_ROOT_OVERRIDE="$foreign_root" "$ROOT/bin/mx-brief.sh" "$id" foreign --scout --herdr-lab >/dev/null 2>&1
  brief="$home/data/$id/brief.md"
  assert_grep "HERDR_LAB_HELPER=$helper" "$brief" \
    "Herdr lab brief must shell-quote an absolute Multplx helper path"
  assert_no_grep "bin/mx-herdr-lab.sh name $id" "$brief" \
    "Herdr lab brief must not invoke a worktree-relative helper"
  pass "mx-brief.sh: --herdr-lab uses its quoted Multplx-owned helper path"
}

test_herdr_lab_omission_is_loud_for_ship_and_scout() {
  local home id brief
  home="$TMP_ROOT/herdr-gate-home"
  mkdir -p "$home/data"
  for kind in delivery scout; do
    id="brief-herdr-gate-$kind"
    if [ "$kind" = scout ]; then
      MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" broker --scout >/dev/null 2>&1
    else
      MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" broker >/dev/null 2>&1
    fi
    brief="$home/data/$id/brief.md"
    assert_grep "# Herdr lifecycle declaration - NOT ENABLED" "$brief" \
      "$kind brief silently omitted the Herdr declaration"
    assert_grep "regenerate the brief with \`--herdr-lab\` before dispatch" "$brief" \
      "$kind brief missing the fail-visible regeneration instruction"
  done
  pass "mx-brief.sh: delivery and scout scaffolds make omitted Herdr intent fail-visible"
}

test_herdr_lab_contract_applies_to_scouts_but_not_daemons() {
  local home brief status=0
  home="$TMP_ROOT/herdr-kind-home"
  mkdir -p "$home/data"
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" herdr-scout broker --scout --herdr-lab >/dev/null 2>&1
  brief="$home/data/herdr-scout/brief.md"
  assert_grep "# Herdr isolation - HARD SAFETY CONTRACT" "$brief" \
    "scout --herdr-lab brief missing the contract"

  MX_HOME="$home" MX_DAEMON_CHARTER=ops "$ROOT/bin/mx-brief.sh" herdr-daemon --daemon broker --herdr-lab >/dev/null 2>&1 || status=$?
  expect_code 1 "$status" "daemon --herdr-lab must be rejected"
  assert_absent "$home/data/herdr-daemon/brief.md" \
    "rejected daemon --herdr-lab still wrote a brief"
  pass "mx-brief.sh: Herdr lab contract covers scouts and rejects daemon misuse"
}

test_same_named_self_repo() {
  local home="$TMP_ROOT/identity-home" source="$TMP_ROOT/identity/Multplx" output
  mkdir -p "$home/data" "$home/projects/Multplx" "$source"
  printf '%s\n' '- Multplx [local-only +yolo] - unrelated clone' > "$home/data/projects.md"
  output=$(MX_HOME="$home" MX_ROOT_OVERRIDE="$source" "$ROOT/bin/mx-brief.sh" self "$source") || fail 'self brief refused'
  assert_contains "$output" 'mode=direct-PR, yolo=off' 'self brief did not use the ordinary publication default'
  assert_grep 'open or update its PR' "$home/data/self/brief.md" 'self completion instructions use wrong mode'
  output=$(MX_HOME="$home" MX_ROOT_OVERRIDE="$source" "$ROOT/bin/mx-brief.sh" clone "$home/projects/Multplx") || fail 'clone brief refused'
  assert_contains "$output" 'mode=local-only, yolo=off' 'clone authority lost'
  output=$(MX_HOME="$home" MX_ROOT_OVERRIDE="$source" "$ROOT/bin/mx-brief.sh" override "$source" --mode direct-PR --yolo on) || fail 'self override refused'
  assert_contains "$output" 'mode=direct-PR, yolo=off' 'self override lost'
  pass 'explicit self-repository and same-named clone briefs retain distinct authority'
}


test_lean_assignments() {
  local home="$TMP_ROOT/lean" kind id brief arg
  write_registry "$home"
  for kind in implementation research review persistent; do
    id="lean-$kind"
    case "$kind" in
      implementation) set -- "$id" direct-proj ;;
      research) set -- "$id" direct-proj --scout ;;
      review) set -- "$id" direct-proj --review ;;
      persistent) set -- "$id" --daemon --no-projects ;;
    esac
    MX_HOME="$home" MX_CLASSIFY_PAUSED_VERB=awaiting "$ROOT/bin/mx-brief.sh" "$@" >/dev/null || fail "$kind generation"
    brief="$home/data/$id/brief.md"
    assert_grep '{TASK}' "$brief" 'missing assignment placeholder'
    assert_grep '# Definition of done' "$brief" 'missing outcome contract'
    assert_grep 'States: working, paused, needs-decision, blocked, done, failed, resolved.' "$brief" 'status vocabulary changed'
    assert_grep "--id $id --state" "$brief" 'task report binding lost'
    assert_grep 'report_status' "$brief" 'MCP status fallback lost'
    assert_grep 'Never write to `' "$brief" 'raw status writes allowed'
    assert_grep 'When a decision is answered or a blocker clears' "$brief" 'keyed resolution lost'
    assert_grep 'same `--key <slug>`' "$brief" 'decision identity lost'
    assert_grep 'Only humans merge PRs' "$brief" 'merge boundary lost'
    assert_grep 'available native tools' "$brief" 'nested delegation omitted'
    assert_grep 'explicitly requested' "$brief" 'optional-tool choice missing'
    for arg in 'same obstacle twice' 'credentialed delivery' 'decision-hold-lifecycle' 'Project memory' 'treehouse' 'read-only GitHub' 'awaiting: {why}' 'Never invoke Multplx lifecycle' 'must drive its local validation'; do
      assert_no_grep "$arg" "$brief" "retired policy leaked: $arg"
    done
  done
  assert_grep 'researcher sub-agent' "$home/data/lean-research/brief.md" 'research assignment lost'
  assert_grep 'reviewer sub-agent' "$home/data/lean-review/brief.md" 'review assignment lost'
  assert_grep 'exact revision assessed' "$home/data/lean-review/brief.md" 'review evidence revision lost'
  assert_grep 'report.md' "$home/data/lean-research/brief.md" 'report artifact lost'
  assert_grep 'Delegate project implementation, code fixes and test-code changes' "$home/data/lean-persistent/brief.md" 'child coordinator coding boundary lost'
  assert_grep 'include that exact token in your parent status reply' "$home/data/lean-persistent/brief.md" 'parent correlation lost'
  assert_grep "status owner \`$home/state\`" "$home/data/lean-persistent/brief.md" 'parent home lost'
  assert_grep 'project-less domain; bind an explicit repository' "$home/data/lean-persistent/brief.md" 'idea scope narrowed to runtime repo'
  assert_grep 'empty queue means idle' "$home/data/lean-persistent/brief.md" 'persistence behavior lost'
  pass 'lean implementation, research, review and persistent templates preserve coordination'
}

test_modes_do_not_request_review_or_merge() {
  local home="$TMP_ROOT/modes" mode output brief
  mkdir -p "$home/data"
  for mode in deep-review direct-PR local-only; do
    output=$(MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "mode-$mode" repo --mode "$mode" --yolo on) || fail 'mode scaffold failed'
    assert_contains "$output" "mode=$mode, yolo=off" 'compatibility resolution changed'
    brief="$home/data/mode-$mode/brief.md"
    assert_no_grep 'mx-deep-review.sh' "$brief" 'mode automatically requested deep-review'
    assert_grep 'Only humans merge PRs' "$brief" 'yolo granted merge authority'
    if [ "$mode" = local-only ]; then
      assert_grep 'without a remote push or PR' "$brief" 'local-only destination lost'
    else
      assert_grep 'You may commit, push your task branch' "$brief" 'ordinary delivery forbidden'
    fi
  done
  pass 'legacy modes preserve destination without implicit review or merge authority'
}

test_handoff_context_and_three_repositories() {
  local home="$TMP_ROOT/multi" n brief repo context output
  mkdir -p "$home/data"
  for n in 1 2 3; do
    repo="$TMP_ROOT/repos/group-$n/shared-name"
    mkdir -p "$repo"
    context="$TMP_ROOT/context-$n.md"
    cat > "$context" <<EOF
Accepted revision: brief-$n
Attempt: unknown (Phase 02 binding pending)
Checkout: $repo
Starting commit: recorded-base-$n
Acceptance criteria: outcome-$n
Original research: /artifacts/research-$n.md
Repository instructions: $repo/AGENTS.md
EOF
    case "$n" in
      1) set -- ;;
      2) set -- --scout ;;
      3) set -- --review ;;
    esac
    output=$(cd "$TMP_ROOT" && MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "repo-$n" "$repo" "$@" --context-file "$context") || fail 'outside-cwd scaffold failed'
    brief="$home/data/repo-$n/brief.md"
    assert_grep "Project/checkout reference: \`$repo\`" "$brief" 'selected checkout lost'
    python3 - "$brief" "$context" <<'CHECK'
from pathlib import Path
import sys
body, context = [Path(p).read_text() for p in sys.argv[1:]]
assert body.endswith(context), 'handoff content changed'
CHECK
    [ "$?" -eq 0 ] || fail 'original handoff altered'
    assert_no_grep "group-$((n % 3 + 1))/shared-name" "$brief" 'other repo instructions leaked'
  done
  pass 'three task-scoped handoffs preserve original context from an arbitrary cwd'
}

test_invalid_input_and_no_clobber() {
  local home="$TMP_ROOT/errors" rc before after
  mkdir -p "$home/data"
  printf 'context
' > "$home/context.md"
  for mode in no-projects mixed-kind missing-context empty-context extra-project unknown duplicate-context; do
    case "$mode" in
      no-projects) set -- invalid --daemon ;;
      mixed-kind) set -- invalid repo --scout --review ;;
      missing-context) set -- invalid repo --context-file "$home/missing" ;;
      empty-context) : > "$home/empty"; set -- invalid repo --context-file "$home/empty" ;;
      extra-project) set -- invalid repo other ;;
      unknown) set -- invalid repo --typo ;;
      duplicate-context) set -- invalid repo --context-file "$home/context.md" --context-file "$home/context.md" ;;
    esac
    rc=0
    MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$@" >/dev/null 2>&1 || rc=$?
    expect_code 1 "$rc" "$mode should refuse"
    assert_absent "$home/data/invalid/brief.md" 'invalid input wrote brief'
  done
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" once repo >/dev/null || fail 'initial scaffold'
  cp "$home/data/once/brief.md" "$home/before"
  rc=0
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" once repo --review >/dev/null 2>&1 || rc=$?
  expect_code 1 "$rc" 'existing brief overwrite must refuse'
  cmp "$home/before" "$home/data/once/brief.md" || fail 'existing brief changed'
  pass 'invalid or ambiguous requests refuse and existing briefs remain byte-identical'
}

test_script_parses
test_help_includes_entire_header
test_herdr_lab_contract_is_explicit_and_complete
test_herdr_lab_contract_quotes_foreign_broker_path
test_herdr_lab_omission_is_loud_for_ship_and_scout
test_herdr_lab_contract_applies_to_scouts_but_not_daemons
test_same_named_self_repo
test_lean_assignments
test_modes_do_not_request_review_or_merge
test_handoff_context_and_three_repositories
test_invalid_input_and_no_clobber
