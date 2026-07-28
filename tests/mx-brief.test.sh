#!/usr/bin/env bash
# Behavior tests for bin/mx-brief.sh.
#
# Regression coverage for the heredoc-in-command-substitution parse bug (issue
# #166): each delivery-mode branch builds its Definition-of-done text with
# `VAR=$(cat <<EOF ... EOF)`. Bash's lexer tracks quote state through the
# heredoc body while it scans for the matching `)` of the command
# substitution, so a single unescaped apostrophe anywhere in that body breaks
# parsing of the *entire rest of the script* - `bash -n` fails, not just the
# generated brief. A plain `cat > file <<EOF ... EOF` (not wrapped in `$(...)`)
# is unaffected, so the daemon charter block does not need this guard.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mx_test_tmproot mx-brief)
BRIEF_HOME="$TMP_ROOT/home"
mkdir -p "$BRIEF_HOME/data"

# The script itself must always parse. This is the direct regression test for
# issue #166: a stray apostrophe in any of the three DOD heredoc bodies
# (no-mistakes/direct-PR/local-only) breaks `bash -n` on the whole file.
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

# Registry with one project per delivery mode, so each delivery-mode DOD branch is
# exercised. A project absent from the registry defaults to no-mistakes.
write_registry() {
  local home=$1
  mkdir -p "$home/data"
  cat > "$home/data/projects.md" <<'EOF'
- direct-proj [direct-PR] - fixture for direct-PR mode (added 2026-07-01)
- local-proj [local-only] - fixture for local-only mode (added 2026-07-01)
EOF
}

# mx-brief.sh must exit 0 and produce a brief with no unreplaced shell
# metacharacter corruption for every delivery mode. This also guards
# against any *new* unescaped apostrophe or unbalanced quote later added to
# one of these DOD blocks, since a broken heredoc corrupts or empties the
# generated brief content, not just the script's own syntax.
test_ship_modes_generate_clean_briefs() {
  local home id brief status
  home="$TMP_ROOT/delivery-home"
  write_registry "$home"

  for id_proj in "brief-nomistakes-a1:no-registry-proj" "brief-directpr-a2:direct-proj" "brief-localonly-a3:local-proj"; do
    id=${id_proj%%:*}
    proj=${id_proj##*:}
    MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" "$proj" >/dev/null 2>&1; status=$?
    expect_code 0 "$status" "mx-brief.sh $id $proj should exit 0"
    brief="$home/data/$id/brief.md"
    assert_present "$brief" "$id: brief was not scaffolded"
    assert_grep "# Definition of done" "$brief" "$id: brief missing Definition of done section"
    assert_grep "{TASK}" "$brief" "$id: brief missing the {TASK} placeholder"
    assert_grep "mid-task \`working:\` line (including setup complete) is nonterminal" "$brief" \
      "$id: brief missing nonterminal working:/setup-complete gate protection"
    assert_no_grep "EOF" "$brief" "$id: brief leaked a heredoc EOF marker (unterminated heredoc)"
  done
  pass "mx-brief.sh: no-mistakes/direct-PR/local-only briefs generate cleanly"
}

test_faster_paths_use_configured_authority_without_stacked_review() {
  local home id brief
  home="$TMP_ROOT/configured-authority-home"
  write_registry "$home"
  id="brief-direct-authority-a4"
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" direct-proj >/dev/null 2>&1
  brief="$home/data/$id/brief.md"
  assert_grep "The configured merge authority decides whether to merge the PR; broker relays the outcome." "$brief" \
    "direct-PR brief lost configured merge authority"
  assert_no_grep "The maintainer reviews and merges the PR" "$brief" \
    "direct-PR brief hard-coded maintainer-only authority"
  id="brief-local-authority-a4"
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" local-proj >/dev/null 2>&1
  brief="$home/data/$id/brief.md"
  assert_grep "The configured merge authority approves the ready branch, then broker merges it into local \`main\` through the guarded fast-forward path." "$brief" \
    "local-only brief lost configured merge authority and guarded landing"
  assert_no_grep "The maintainer approves the ready branch" "$brief" \
    "local-only brief hard-coded maintainer-only authority"
  assert_no_grep "Multplx then reviews your branch diff" "$brief" \
    "local-only brief retained a personal review stacked on the selected delivery path"
  pass "mx-brief.sh: faster paths use configured authority without stacked review"
}

# Pin the specific line the bug lived on: the no-mistakes DOD's no-mistakes
# reference must render as plain prose with no dangling apostrophe artifact.
test_no_mistakes_dod_wording() {
  local home id brief
  home="$TMP_ROOT/wording-home"
  mkdir -p "$home/data"
  id="brief-wording-b1"
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" some-proj >/dev/null 2>&1
  brief="$home/data/$id/brief.md"
  assert_present "$brief" "brief was not scaffolded"
  assert_grep "no-mistakes itself provides for the mechanics" "$brief" \
    "no-mistakes DOD lost its guidance-reference sentence"
  # shellcheck disable=SC2016  # single quotes are deliberate: the backticks must stay literal
  assert_grep '`no-mistakes axi run --help`' "$brief" \
    "no-mistakes DOD must render literal backticks around the help command"
  # shellcheck disable=SC2016  # single quotes are deliberate: the backticks must stay literal
  assert_grep '`help`' "$brief" \
    "no-mistakes DOD must render literal backticks around help"
  assert_no_grep "no-mistakes' own guidance" "$brief" \
    "no-mistakes DOD regressed to the apostrophe form that breaks bash -n"
  pass "mx-brief.sh: no-mistakes DOD wording avoids the apostrophe regression"
}

test_ship_project_memory_wording() {
  local home id brief
  home="$TMP_ROOT/project-memory-home"
  mkdir -p "$home/data"
  id="brief-memory-c1"
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" "$id" some-proj >/dev/null 2>&1
  brief="$home/data/$id/brief.md"
  assert_present "$brief" "brief was not scaffolded"
  assert_grep "Record only project knowledge useful to almost every future session." "$brief" \
    "project-memory contract lost the durable-knowledge bar"
  assert_grep "prefer a pointer to the authoritative file, command, or doc over copying the detail" "$brief" \
    "project-memory contract lost pointer-over-copy guidance"
  assert_grep "lacks \`## Maintaining this file\`, add that short self-governance section" "$brief" \
    "project-memory contract lost the self-governance add-in-same-pass rule"
  pass "mx-brief.sh: delivery project-memory wording carries the AGENTS.md authoring bar"
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

test_daemon_no_projects_charter() {
  local home brief status
  home="$TMP_ROOT/no-projects-home"
  mkdir -p "$home/data"

  # The deliberate --no-projects signal scaffolds a valid project-less charter for
  # a domain whose subject is the Multplx repo itself (no clones needed).
  MX_HOME="$home" MX_DAEMON_CHARTER='broker self-development' \
    MX_DAEMON_SCOPE='Multplx repo work' \
    "$ROOT/bin/mx-brief.sh" fdev --daemon --no-projects >/dev/null 2>&1; status=$?
  expect_code 0 "$status" "--no-projects daemon brief should exit 0"
  brief="$home/data/fdev/brief.md"
  assert_present "$brief" "project-less charter was not scaffolded"
  assert_grep "# Project clones" "$brief" "project-less charter dropped the Project clones heading"
  assert_grep "None. This is a project-less domain" "$brief" \
    "project-less charter did not render a sensible no-clones note"
  assert_grep "its actors take pooled worktrees of that repo" "$brief" \
    "project-less charter operating model lost the pooled-worktree note"
  assert_no_grep "The projects above are local clones" "$brief" \
    "project-less charter kept the with-projects operating-model line"
  assert_grep 'working [key=<work-slug>]' "$brief" \
    "daemon charter did not key material routed-work phases"
  assert_grep 'report `resolved` with the same `--key <work-slug>`' "$brief" \
    "daemon charter did not close a quietly ended routed-work phase"
  assert_grep 'use the same key on its later' "$brief" \
    "daemon charter did not supersede working phases with later states"
  if grep -nE '^-[[:space:]]*$' "$brief" >/dev/null; then
    fail "project-less charter left a stray empty project bullet"
  fi

  # Accidental omission (no projects, no signal) still fails loudly, writing nothing.
  MX_HOME="$home" MX_DAEMON_CHARTER='x' "$ROOT/bin/mx-brief.sh" oops --daemon >/dev/null 2>&1; status=$?
  expect_code 1 "$status" "daemon brief with no projects and no --no-projects must fail"
  assert_absent "$home/data/oops/brief.md" "loud-failure daemon brief still wrote a file"

  # --no-projects is mutually exclusive with a project list.
  MX_HOME="$home" MX_DAEMON_CHARTER='x' "$ROOT/bin/mx-brief.sh" oops2 --daemon --no-projects alpha >/dev/null 2>&1; status=$?
  expect_code 1 "$status" "--no-projects combined with a project list must fail"

  # --no-projects applies only to daemon charters, never a delivery/scout brief.
  MX_HOME="$home" "$ROOT/bin/mx-brief.sh" oops3 somerepo --no-projects >/dev/null 2>&1; status=$?
  expect_code 1 "$status" "--no-projects on a delivery brief must fail"

  pass "mx-brief.sh: --no-projects scaffolds a project-less charter and guards misuse"
}

test_daemon_marked_request_reporting_contract() {
  local home brief
  home="$TMP_ROOT/marked-request-reporting-home"
  mkdir -p "$home/data"
  MX_HOME="$home" MX_CLASSIFY_PAUSED_VERB=paused \
    MX_DAEMON_CHARTER='Handle routed domain work.' \
    "$ROOT/bin/mx-brief.sh" marked-request-reporting --daemon --no-projects >/dev/null 2>&1
  brief="$home/data/marked-request-reporting/brief.md"

  assert_grep 'A marked request requires one correlated answer after the work' "$brief" \
    "daemon charter did not require the correlated answer after the work"
  assert_grep 'does not require a separate receipt or start acknowledgement' "$brief" \
    "daemon charter did not reject a separate receipt/start acknowledgement"
  assert_grep "Never report \`working\` merely to acknowledge receipt or announce that a marked request has started." "$brief" \
    "daemon charter did not forbid a generic working acknowledgement"
  assert_no_grep "Give every routed-work phase a stable key: open it with \`working" "$brief" \
    "daemon charter retained the unconditional working opener"
  assert_grep 'When a routed-work phase has a supervisor-actionable material change worth reporting under the rule above' "$brief" \
    "daemon charter did not limit keyed phases to reportable material changes"
  assert_grep "If its first reportable event is \`working [key=<work-slug>]: {material phase}\`" "$brief" \
    "daemon charter lost keyed working syntax for a reportable material phase"
  assert_grep "use the same key on its later \`paused\`, \`done\`, \`failed\`, \`needs-decision\`, or \`blocked\` event" "$brief" \
    "daemon charter lost same-key closure for a reportable material phase"
  assert_grep 'report `resolved` with the same `--key <work-slug>`' "$brief" \
    "daemon charter lost resolved closure for a keyed material phase"

  assert_grep 'include that exact token in your parent status reply' "$brief" \
    "daemon charter lost correlated parent results"
  assert_grep 'For a terse result, a status line is the whole answer.' "$brief" \
    "daemon charter lost terse result reporting"
  assert_grep 'report a status that points to that doc' "$brief" \
    "daemon charter lost detailed document pointers"
  assert_grep 'Report only true maintainer-relevant outcomes or a declared external wait' "$brief" \
    "daemon charter lost declared external waits"
  assert_grep 'a maintainer decision, a real blocker, a failure, or work ready for review' "$brief" \
    "daemon charter lost decisions, blockers, failures, or ready outcomes"
  assert_grep 'States: working, paused, needs-decision, blocked, done, failed, resolved.' "$brief" \
    "daemon charter does not expose the validated status vocabulary"
  assert_grep 'report_status' "$brief" \
    "daemon charter does not prefer the MCP status tool"
  assert_grep 'bin/mx-report --id marked-request-reporting' "$brief" \
    "daemon charter does not carry the universal validated fallback"
  assert_no_grep 'echo "{state}:' "$brief" \
    "daemon charter still instructs a raw status append"
  pass "mx-brief.sh: marked requests avoid generic acknowledgements and preserve material reporting"
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

test_validated_status_vocabulary_renders_all_brief_scaffolds() {
  local home kind id brief
  home="$TMP_ROOT/pause-verb-home"
  mkdir -p "$home/data"

  for kind in delivery scout daemon; do
    id="brief-pause-verb-$kind"
    case "$kind" in
      delivery)
        MX_HOME="$home" MX_CLASSIFY_PAUSED_VERB=awaiting \
          "$ROOT/bin/mx-brief.sh" "$id" broker >/dev/null 2>&1
        ;;
      scout)
        MX_HOME="$home" MX_CLASSIFY_PAUSED_VERB=awaiting \
          "$ROOT/bin/mx-brief.sh" "$id" broker --scout >/dev/null 2>&1
        ;;
      daemon)
        MX_HOME="$home" MX_CLASSIFY_PAUSED_VERB=awaiting \
          "$ROOT/bin/mx-brief.sh" "$id" --daemon --no-projects >/dev/null 2>&1
        ;;
    esac
    brief="$home/data/$id/brief.md"
    assert_grep "States: working, paused, needs-decision, blocked, done, failed, resolved." "$brief" \
      "$kind brief did not render the fixed validated state vocabulary"
    # shellcheck disable=SC2016 # Literal backticks and braces must remain unexpanded.
    assert_grep 'Use `paused: {why}`' "$brief" \
      "$kind brief did not instruct the validated pause state"
    # shellcheck disable=SC2016 # Literal backticks and braces must remain unexpanded.
    assert_no_grep '`awaiting: {why}`' "$brief" \
      "$kind brief leaked the read-side pause compatibility override into the writer contract"
    assert_grep 'report_status' "$brief" \
      "$kind brief does not prefer the report_status tool"
    assert_grep "bin/mx-report --id $id" "$brief" \
      "$kind brief does not include the task-bound shell fallback"
    assert_no_grep 'echo "{state}:' "$brief" \
      "$kind brief still instructs a raw status append"
    assert_grep 'Never write to `' "$brief" \
      "$kind brief does not forbid direct status-file writes"
    assert_grep 'or a blocker clears' "$brief" \
      "$kind brief did not require durable resolution when a blocker clears"
  done
  pass "mx-brief.sh: every scaffold uses the fixed validated status vocabulary and write path"
}

test_scout_and_daemon_load_decision_hold_policy() {
  local home scout charter
  home="$TMP_ROOT/decision-policy-home"
  mkdir -p "$home/data"
  MX_HOME="$home" MX_ROOT_OVERRIDE="$ROOT" \
    "$ROOT/bin/mx-brief.sh" sample-investigation sample --scout >/dev/null 2>&1
  scout="$home/data/sample-investigation/brief.md"
  assert_grep "$ROOT/.agents/skills/decision-hold-lifecycle/SKILL.md" "$scout" \
    "scout brief did not load the unresolved-decision policy before done"
  assert_grep "pass its shared completion gate for the report and any visual review" "$scout" \
    "scout brief did not cross-reference visual-review completion"
  MX_HOME="$home" MX_ROOT_OVERRIDE="$ROOT" MX_DAEMON_CHARTER='sample reviews' \
    "$ROOT/bin/mx-brief.sh" sample-mate --daemon --no-projects >/dev/null 2>&1
  charter="$home/data/sample-mate/brief.md"
  assert_grep "load \`decision-hold-lifecycle\`" "$charter" \
    "daemon charter did not load the shared decision policy for detailed investigations"
  pass "mx-brief.sh: investigation and visual-review completions load the shared decision policy"
}

# Scout and daemon paths still scaffold well-formed briefs.
test_scout_and_daemon_scaffold() {
  local brief
  MX_HOME="$BRIEF_HOME" "$ROOT/bin/mx-brief.sh" brief-scout-q6 alpha --scout >/dev/null 2>&1 \
    || fail "mx-brief.sh scout scaffold exited non-zero"
  brief="$BRIEF_HOME/data/brief-scout-q6/brief.md"
  assert_present "$brief" "scout brief was not scaffolded"
  assert_grep "SCOUT task" "$brief" "scout brief must declare itself a scout task"
  assert_grep "report.md" "$brief" "scout brief must point at the report deliverable"

  MX_DAEMON_CHARTER='Supervise the alpha domain.' \
    MX_HOME="$BRIEF_HOME" "$ROOT/bin/mx-brief.sh" brief-sm-q6 --daemon alpha >/dev/null 2>&1 \
    || fail "mx-brief.sh daemon scaffold exited non-zero"
  brief="$BRIEF_HOME/data/brief-sm-q6/brief.md"
  assert_present "$brief" "daemon charter was not scaffolded"
  assert_grep "persistent daemon" "$brief" \
    "daemon charter must declare its role"
  pass "mx-brief: scout and daemon code paths still scaffold well-formed briefs"
}

test_script_parses
test_help_includes_entire_header
test_ship_modes_generate_clean_briefs
test_faster_paths_use_configured_authority_without_stacked_review
test_no_mistakes_dod_wording
test_ship_project_memory_wording
test_herdr_lab_contract_is_explicit_and_complete
test_herdr_lab_contract_quotes_foreign_broker_path
test_herdr_lab_omission_is_loud_for_ship_and_scout
test_herdr_lab_contract_applies_to_scouts_but_not_daemons
test_daemon_no_projects_charter
test_daemon_marked_request_reporting_contract
test_validated_status_vocabulary_renders_all_brief_scaffolds
test_scout_and_daemon_load_decision_hold_policy
test_scout_and_daemon_scaffold
