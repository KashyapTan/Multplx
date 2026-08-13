#!/usr/bin/env bash
# Unit coverage for the constrained workflow schema and substitutions.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

mx_test_tmproot_into TMP_ROOT workflow-lib

wf_definition_json() {
  "$ROOT/target/release/mx" authority mx-workflow.sh parse-json "$1"
}

wf_substitute() {
  "$ROOT/target/release/mx" authority mx-workflow.sh substitute "$1" "$2" "$3" "$4"
}

wf_output_path() {
  "$ROOT/target/release/mx" authority mx-workflow.sh output-path "$1" "$2" "$3"
}

assert_eq() {
  [ "$1" = "$2" ] || fail "$3: expected '$1', got '$2'"
}

assert_file_contains() {
  grep -F "$2" "$1" >/dev/null 2>&1 || fail "$3"
}

write_definition() {
  local file=$1 version=${2:-1} type=${3:-agent} gate=${4:-auto} contract=${5:-output}
  cat >"$file" <<EOF
---
workflow_version: $version
name: schema-case
description: Schema fixture.
stages:
  - id: make
    title: Make output
    type: $type
    executor: broker
    gate: $gate
    output: data/{run}/result.md
    contract: $contract
---

## make

Handle {input} and write {output} for {run}.
EOF
}

expect_invalid() {
  local file=$1 expected=$2 output
  output=$(wf_definition_json "$file" 2>&1) && fail "invalid definition passed: $expected"
  assert_contains "$output" "$expected" "validator did not explain $expected"
}

test_valid_definition_and_substitution() {
  local file="$TMP_ROOT/valid.workflow" json rendered
  write_definition "$file"
  json=$(wf_definition_json "$file") || fail "valid workflow was rejected"
  assert_eq "schema-case" "$(printf '%s\n' "$json" | jq -r '.name')" \
    "normalized workflow name changed"
  assert_eq "agent" "$(printf '%s\n' "$json" | jq -r '.stages[0].type')" \
    "normalized stage type changed"
  rendered=$(wf_substitute 'run={run}; input={input}; output={output}' \
    run-1 'repair it' '/tmp/result.md')
  assert_eq 'run=run-1; input=repair it; output=/tmp/result.md' "$rendered" \
    "fixed substitutions were not resolved"
  pass "valid workflow normalizes and fixed substitutions resolve"
}

test_closed_enums_and_version() {
  local file="$TMP_ROOT/invalid.workflow"
  write_definition "$file" 2 agent auto output
  expect_invalid "$file" "unsupported workflow_version"
  write_definition "$file" 1 mystery auto output
  expect_invalid "$file" "unknown type"
  write_definition "$file" 1 agent maybe output
  expect_invalid "$file" "unknown gate"
  write_definition "$file" 1 agent auto network-proof
  expect_invalid "$file" "unknown contract"
  pass "version and stage enums are closed"
}

test_auto_requires_contract() {
  local file="$TMP_ROOT/auto-without-contract.workflow"
  cat >"$file" <<'EOF'
---
workflow_version: 1
name: unsafe-auto
description: Invalid automatic stage.
stages:
  - id: think
    title: Think
    type: agent
    executor: broker
    gate: auto
---

## think

Think about {input}.
EOF
  expect_invalid "$file" "auto gate without a verifiable contract"
  pass "automatic agent stages require deterministic contracts"
}

test_stage_body_and_command_trust_validation() {
  local file="$TMP_ROOT/body.workflow" json
  cat >"$file" <<'EOF'
---
workflow_version: 1
name: body-check
description: Invalid body mapping.
stages:
  - id: expected
    title: Expected
    type: command
    gate: auto
    run: printf safe
---

## other

Unexpected body.
EOF
  expect_invalid "$file" "has no matching markdown body"

  cat >"$file" <<'EOF'
---
workflow_version: 1
name: command-input
description: Invalid command substitution.
stages:
  - id: command
    title: Command
    type: command
    gate: auto
    run: printf {input}
---

## command

Run the command.
EOF
  expect_invalid "$file" "unknown substitution {input}"

  cat >"$file" <<'EOF'
---
workflow_version: 1
name: quoted-command
description: Preserve internal shell quotes.
stages:
  - id: command
    title: Command
    type: command
    gate: auto
    run: bash "$MX_WORKFLOW_HOME/bin/check.sh" {run}
---

## command

Run the command.
EOF
  json=$(wf_definition_json "$file") || fail "plain command with internal quotes was rejected"
  assert_eq 'bash "$MX_WORKFLOW_HOME/bin/check.sh" {run}' \
    "$(printf '%s\n' "$json" | jq -r '.stages[0].run')" \
    "internal command quotes changed during parsing"

  sed 's#^    run:.*#    run: "$MX_WORKFLOW_HOME/bin/check.sh" {run}"#' \
    "$file" >"$TMP_ROOT/malformed-quote.workflow"
  expect_invalid "$TMP_ROOT/malformed-quote.workflow" \
    "unescaped quote inside a quoted scalar"

  cat >"$file" <<'EOF'
---
workflow_version: 1
name: control-output
description: Invalid control-state output.
stages:
  - id: overwrite
    title: Overwrite state
    type: agent
    executor: broker
    gate: auto
    output: state/{run}.workflow/definition.json
---

## overwrite

Write {output}.
EOF
  expect_invalid "$file" "output cannot target workflow control state"
  pass "body ids match and launch input cannot become shell code"
}

test_output_path_rejects_symlink_escape() {
  local home="$TMP_ROOT/output-home" outside="$TMP_ROOT/outside" output
  mkdir -p "$home" "$outside"
  ln -s "$outside" "$home/data"
  output=$(wf_output_path "$home" 'data/{run}/result.md' run-1 2>&1) \
    && fail "output path followed a symlink outside the home"
  assert_contains "$output" "output escapes the Multplx home" \
    "symlink output refusal was unclear"
  pass "output contracts cannot escape the Multplx home through symlinks"
}

test_create_workflow_golden_output() {
  local golden="$ROOT/tests/fixtures/create-workflow-golden.workflow"
  local interview="$ROOT/tests/fixtures/create-workflow-interview.fixture"
  [ -s "$interview" ] || fail "golden interview fixture is absent"
  wf_definition_json "$golden" >/dev/null || fail "create-workflow golden definition is invalid"
  assert_file_contains "$ROOT/.agents/skills/create-workflow/SKILL.md" \
    "bin/mx-workflow.sh validate" "skill no longer validates its draft"
  assert_file_contains "$ROOT/.agents/skills/create-workflow/SKILL.md" \
    "Never generate a per-workflow script" "skill can generate enforcement scripts"
  pass "golden interview output remains valid against the schema"
}

test_valid_definition_and_substitution
test_closed_enums_and_version
test_auto_requires_contract
test_stage_body_and_command_trust_validation
test_output_path_rejects_symlink_escape
test_create_workflow_golden_output
