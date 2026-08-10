#!/usr/bin/env bash
# Shared implementation for the Multplx linear workflow engine.
#
# docs/workflows.md is the single owner of the workflow-definition format.
# This library owns its constrained frontmatter parser, launch-time definition
# snapshot, run records, deterministic contracts, and stage executors.
# Sourcing this file has no side effects.
#
# Runtime seams used by focused tests:
#   MX_WORKFLOW_AGENT_COMMAND       headless one-shot adapter
#   MX_WORKFLOW_SPAWN_COMMAND       actor spawn entrypoint
#   MX_WORKFLOW_ACTOR_STATE_COMMAND actor reconciliation entrypoint
#
# The production headless path delegates to dr_agent_oneshot from
# mx-deep-review-lib.sh so claude, codex, and pi keep one verified adapter.

wf_error() {
  printf 'mx-workflow: %s\n' "$*" >&2
  return 1
}

wf_require_tools() {
  command -v node >/dev/null 2>&1 || {
    wf_error "node is required"
    return 1
  }
  command -v jq >/dev/null 2>&1 || {
    wf_error "jq is required"
    return 1
  }
}

wf_atomic_write() { # <destination> [mode]
  local destination=$1 mode=${2:-600} directory temporary
  directory=$(dirname "$destination")
  mkdir -p "$directory" || return 1
  temporary=$(mktemp "$directory/.workflow.tmp.XXXXXX") || return 1
  if ! cat >"$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  chmod "$mode" "$temporary" || {
    rm -f "$temporary"
    return 1
  }
  mv -f "$temporary" "$destination"
}

wf_sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    wf_error "shasum or sha256sum is required"
    return 1
  fi
}

wf_now() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

wf_slug_valid() {
  case "${1:-}" in
    ''|*[!A-Za-z0-9._-]*) return 1 ;;
    *) return 0 ;;
  esac
}

# Parse and validate the deliberately constrained workflow format.
# The normalized JSON is the engine's only machine representation.
wf_definition_json() { # <definition-file>
  local definition=$1
  wf_require_tools || return 1
  [ -f "$definition" ] && [ ! -L "$definition" ] || {
    wf_error "definition must be a regular non-symlink file: $definition"
    return 1
  }
  node - "$definition" <<'WF_NODE'
'use strict';

const fs = require('fs');
const file = process.argv[2];

function fail(message) {
  process.stderr.write(`mx-workflow: ${message}\n`);
  process.exit(1);
}

function stripComment(value) {
  let quote = null;
  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];
    if (quote) {
      if (char === quote && value[index - 1] !== '\\') quote = null;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (char === '#' && (index === 0 || /\s/.test(value[index - 1]))) {
      return value.slice(0, index).trimEnd();
    }
  }
  return value.trimEnd();
}

function scalar(raw, label) {
  const value = stripComment(raw).trim();
  if (!value) fail(`${label} must not be empty`);
  if (value.startsWith('"') || value.startsWith("'")) {
    const quote = value[0];
    if (value.length < 2 || !value.endsWith(quote)) {
      fail(`${label} has an unterminated quoted scalar`);
    }
    const interior = value.slice(1, -1);
    for (let index = 0; index < interior.length; index += 1) {
      if (interior[index] === quote && interior[index - 1] !== '\\') {
        fail(`${label} has an unescaped quote inside a quoted scalar`);
      }
    }
    return value.slice(1, -1);
  }
  if (value === 'true') return true;
  if (value === 'false') return false;
  if (/^[0-9]+$/.test(value)) return Number(value);
  return value;
}

function list(raw, label) {
  const value = stripComment(raw).trim();
  if (!value.startsWith('[') || !value.endsWith(']')) {
    fail(`${label} must use an inline list`);
  }
  const interior = value.slice(1, -1).trim();
  if (!interior) return [];
  return interior.split(',').map((entry, index) =>
    scalar(entry.trim(), `${label}[${index}]`));
}

function oneLine(value, label) {
  if (typeof value !== 'string' || !value || /[\r\n]/.test(value)) {
    fail(`${label} must be one non-empty line`);
  }
}

function slug(value, label) {
  oneLine(value, label);
  if (!/^[A-Za-z0-9._-]+$/.test(value)) {
    fail(`${label} must be a privacy-safe slug`);
  }
}

function substitutions(value, label, allowed, requiresOutput) {
  if (typeof value !== 'string') return;
  for (const match of value.matchAll(/\{([^{}]+)\}/g)) {
    if (!allowed.includes(match[1])) {
      fail(`${label} uses unknown substitution {${match[1]}}`);
    }
    if (match[1] === 'output' && !requiresOutput) {
      fail(`${label} uses {output} without declaring output`);
    }
  }
}

let text;
try {
  text = fs.readFileSync(file, 'utf8');
} catch (error) {
  fail(`cannot read definition: ${error.message}`);
}
const lines = text.replace(/\r\n/g, '\n').split('\n');
if (lines[0] !== '---') fail('definition must begin with YAML frontmatter');
let closing = -1;
for (let index = 1; index < lines.length; index += 1) {
  if (lines[index] === '---') {
    closing = index;
    break;
  }
}
if (closing < 0) fail('definition frontmatter has no closing ---');

const topAllowed = new Set(['workflow_version', 'name', 'description', 'stages']);
const stageAllowed = new Set([
  'id', 'title', 'type', 'gate', 'output', 'executor',
  'fresh_session', 'brief_from', 'contract', 'run'
]);
const top = {};
const stages = [];
let current = null;
let inStages = false;

for (let index = 1; index < closing; index += 1) {
  const original = lines[index];
  const clean = stripComment(original);
  if (!clean.trim()) continue;
  if (clean.includes('\t')) fail(`frontmatter line ${index + 1} contains a tab`);
  let match;
  if ((match = clean.match(/^([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$/))) {
    const key = match[1];
    if (!topAllowed.has(key)) fail(`unknown top-level field '${key}'`);
    if (key === 'stages') {
      if (match[2].trim()) fail('stages must be a block list');
      inStages = true;
      continue;
    }
    if (inStages) fail(`top-level field '${key}' appears after stages`);
    if (Object.hasOwn(top, key)) fail(`duplicate top-level field '${key}'`);
    top[key] = scalar(match[2], key);
    continue;
  }
  if ((match = clean.match(/^  - ([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$/))) {
    if (!inStages) fail(`stage appears before stages at line ${index + 1}`);
    const key = match[1];
    if (key !== 'id') fail(`each stage must begin with id at line ${index + 1}`);
    current = {id: scalar(match[2], `stage ${stages.length + 1} id`)};
    stages.push(current);
    continue;
  }
  if ((match = clean.match(/^    ([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$/))) {
    if (!current) fail(`stage field appears before a stage id at line ${index + 1}`);
    const key = match[1];
    if (!stageAllowed.has(key)) fail(`unknown stage field '${key}'`);
    if (Object.hasOwn(current, key)) fail(`duplicate field '${key}' in stage ${current.id}`);
    current[key] = key === 'brief_from'
      ? list(match[2], `stage ${current.id} brief_from`)
      : scalar(match[2], `stage ${current.id} ${key}`);
    continue;
  }
  fail(`unsupported frontmatter syntax at line ${index + 1}`);
}

if (top.workflow_version !== 1) {
  fail(`unsupported workflow_version '${top.workflow_version ?? ''}'`);
}
slug(top.name, 'name');
oneLine(top.description, 'description');
if (!inStages || !stages.length) fail('stages must contain at least one stage');

const bodies = new Map();
let bodyId = null;
let bodyLines = [];
function finishBody() {
  if (bodyId === null) return;
  while (bodyLines.length && bodyLines[0].trim() === '') bodyLines.shift();
  while (bodyLines.length && bodyLines[bodyLines.length - 1].trim() === '') bodyLines.pop();
  const body = bodyLines.join('\n');
  if (!body.trim()) fail(`stage body '${bodyId}' must not be empty`);
  if (bodies.has(bodyId)) fail(`duplicate stage body '${bodyId}'`);
  bodies.set(bodyId, body);
}
for (let index = closing + 1; index < lines.length; index += 1) {
  const match = lines[index].match(/^## ([A-Za-z0-9._-]+)\s*$/);
  if (match) {
    finishBody();
    bodyId = match[1];
    bodyLines = [];
  } else if (bodyId !== null) {
    bodyLines.push(lines[index]);
  } else if (lines[index].trim()) {
    fail(`content before first stage body at line ${index + 1}`);
  }
}
finishBody();

const ids = new Set();
const prior = new Set();
for (const [index, stage] of stages.entries()) {
  const prefix = `stage ${index + 1}`;
  slug(stage.id, `${prefix} id`);
  if (ids.has(stage.id)) fail(`duplicate stage id '${stage.id}'`);
  ids.add(stage.id);
  oneLine(stage.title, `stage ${stage.id} title`);
  if (!['interactive', 'agent', 'command'].includes(stage.type)) {
    fail(`stage ${stage.id} has unknown type '${stage.type ?? ''}'`);
  }
  if (!['approve', 'auto'].includes(stage.gate)) {
    fail(`stage ${stage.id} has unknown gate '${stage.gate ?? ''}'`);
  }
  if (!bodies.has(stage.id)) fail(`stage ${stage.id} has no matching markdown body`);
  stage.body = bodies.get(stage.id);
  stage.brief_from = stage.brief_from || [];
  if (!Array.isArray(stage.brief_from) ||
      stage.brief_from.some(value => typeof value !== 'string')) {
    fail(`stage ${stage.id} brief_from must contain stage ids`);
  }
  for (const source of stage.brief_from) {
    slug(source, `stage ${stage.id} brief_from entry`);
    if (!prior.has(source)) {
      fail(`stage ${stage.id} brief_from references non-prior stage '${source}'`);
    }
    const sourceStage = stages.find(candidate => candidate.id === source);
    if (!sourceStage.output) {
      fail(`stage ${stage.id} brief_from source '${source}' declares no output`);
    }
  }

  if (stage.output !== undefined) {
    oneLine(stage.output, `stage ${stage.id} output`);
    if (stage.output.startsWith('/') || stage.output.includes('\\') ||
        !/^[A-Za-z0-9._/{}/-]+$/.test(stage.output) ||
        stage.output.split('/').includes('..')) {
      fail(`stage ${stage.id} output must be a safe relative path`);
    }
    if (/^state\/\{run\}\.workflow(?:\/|$)/.test(stage.output)) {
      fail(`stage ${stage.id} output cannot target workflow control state`);
    }
    substitutions(stage.output, `stage ${stage.id} output`, ['run'], false);
  }
  if (stage.contract !== undefined &&
      !['output', 'local-commits'].includes(stage.contract)) {
    fail(`stage ${stage.id} has unknown contract '${stage.contract}'`);
  }
  if (stage.contract === 'output' && !stage.output) {
    fail(`stage ${stage.id} contract output requires an output path`);
  }
  if (stage.gate === 'auto' &&
      stage.type !== 'command' && !stage.output && stage.contract !== 'local-commits') {
    fail(`stage ${stage.id} uses auto gate without a verifiable contract`);
  }

  if (stage.type === 'interactive') {
    if (stage.gate !== 'approve') {
      fail(`interactive stage ${stage.id} must use gate approve`);
    }
    for (const field of ['executor', 'fresh_session', 'contract', 'run']) {
      if (stage[field] !== undefined) {
        fail(`interactive stage ${stage.id} cannot set ${field}`);
      }
    }
    if (stage.brief_from.length) {
      fail(`interactive stage ${stage.id} cannot set brief_from`);
    }
  }
  if (stage.type === 'agent') {
    if (!['broker', 'actor'].includes(stage.executor)) {
      fail(`agent stage ${stage.id} requires executor broker or actor`);
    }
    if (stage.run !== undefined) fail(`agent stage ${stage.id} cannot set run`);
    if (stage.fresh_session !== undefined && typeof stage.fresh_session !== 'boolean') {
      fail(`stage ${stage.id} fresh_session must be true or false`);
    }
    if (stage.executor === 'broker' && stage.fresh_session !== undefined) {
      fail(`broker stage ${stage.id} cannot set fresh_session`);
    }
    if (stage.executor === 'broker' && stage.contract === 'local-commits') {
      fail(`broker stage ${stage.id} cannot use local-commits`);
    }
    stage.fresh_session = stage.executor === 'actor'
      ? (stage.fresh_session ?? false)
      : null;
  }
  if (stage.type === 'command') {
    oneLine(stage.run, `command stage ${stage.id} run`);
    for (const field of ['executor', 'fresh_session', 'brief_from']) {
      if (field === 'brief_from' && stage.brief_from.length === 0) continue;
      if (stage[field] !== undefined && stage[field] !== null) {
        fail(`command stage ${stage.id} cannot set ${field}`);
      }
    }
    substitutions(stage.run, `stage ${stage.id} run`, ['run', 'output'], Boolean(stage.output));
    if (stage.contract === 'local-commits' &&
        !stages.slice(0, index).some(candidate =>
          candidate.type === 'agent' && candidate.executor === 'actor')) {
      fail(`command stage ${stage.id} local-commits requires a prior actor stage`);
    }
  }
  substitutions(
    stage.body,
    `stage ${stage.id} body`,
    ['run', 'input', 'output'],
    Boolean(stage.output)
  );
  prior.add(stage.id);
}
for (const id of bodies.keys()) {
  if (!ids.has(id)) fail(`markdown body '${id}' has no matching stage`);
}

process.stdout.write(JSON.stringify({
  workflow_version: 1,
  name: top.name,
  description: top.description,
  stages
}, null, 2) + '\n');
WF_NODE
}

wf_substitute() { # <text> <run> <input> <output>
  local value=$1 run=$2 input=$3 output=${4:-}
  value=${value//\{run\}/$run}
  value=${value//\{input\}/$input}
  value=${value//\{output\}/$output}
  printf '%s' "$value"
}

wf_shell_quote() {
  printf "'"
  printf '%s' "$1" | sed "s/'/'\\\\''/g"
  printf "'"
}

wf_output_path() { # <home> <declared-output> <run>
  local home=$1 declared=$2 run=$3 resolved
  [ -n "$declared" ] || return 1
  declared=$(wf_substitute "$declared" "$run" "" "")
  resolved=$(node - "$home" "$declared" <<'WF_PATH'
const fs = require('fs');
const path = require('path');
const home = path.resolve(process.argv[2]);
const resolved = path.resolve(home, process.argv[3]);
if (resolved === home || !resolved.startsWith(home + path.sep)) process.exit(1);
let current = home;
for (const part of path.relative(home, resolved).split(path.sep)) {
  current = path.join(current, part);
  try {
    if (fs.lstatSync(current).isSymbolicLink()) process.exit(1);
  } catch (error) {
    if (error.code === 'ENOENT') break;
    process.exit(1);
  }
}
process.stdout.write(resolved);
WF_PATH
  ) || {
    wf_error "output escapes the Multplx home: $declared"
    return 1
  }
  printf '%s\n' "$resolved"
}

wf_stage_json() { # <definition-json> <stage-id>
  jq -c --arg id "$2" '.stages[] | select(.id == $id)' "$1"
}

wf_stage_record_path() { # <run-dir> <stage-id>
  printf '%s/stages/%s.json\n' "$1" "$2"
}

wf_stage_record_write() { # <run-dir> <stage-id>
  local run_dir=$1 stage_id=$2
  wf_atomic_write "$(wf_stage_record_path "$run_dir" "$stage_id")" 600
}

wf_verify_snapshot() { # <run-dir>
  local run_dir=$1 snapshot normalized expected actual temporary
  snapshot="$run_dir/definition.workflow.md"
  normalized="$run_dir/definition.json"
  [ -f "$snapshot" ] && [ ! -L "$snapshot" ] \
    && [ -f "$normalized" ] && [ ! -L "$normalized" ] || {
    wf_error "definition snapshot is missing or unsafe"
    return 1
  }
  expected=$(jq -r '.definition_sha256 // empty' "$run_dir/run.json" 2>/dev/null)
  actual=$(wf_sha256_file "$snapshot") || return 1
  [ -n "$expected" ] && [ "$actual" = "$expected" ] || {
    wf_error "definition snapshot digest changed after launch"
    return 1
  }
  temporary=$(mktemp "$run_dir/.definition-check.XXXXXX") || return 1
  if ! wf_definition_json "$snapshot" >"$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  if ! cmp -s "$temporary" "$normalized"; then
    rm -f "$temporary"
    wf_error "normalized definition changed after launch"
    return 1
  fi
  rm -f "$temporary"
}

wf_stage_record_status() { # <run-dir> <stage-id>
  local record
  record=$(wf_stage_record_path "$1" "$2")
  if [ -f "$record" ]; then
    jq -r '.status // "pending"' "$record" 2>/dev/null || printf 'invalid\n'
  else
    printf 'pending\n'
  fi
}

wf_run_update() { # <run-dir> <jq args/filter>
  local run_dir=$1 run_file temporary
  shift
  run_file="$run_dir/run.json"
  temporary=$(mktemp "$run_dir/.run.tmp.XXXXXX") || return 1
  if ! jq "$@" "$run_file" >"$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  chmod 600 "$temporary" || {
    rm -f "$temporary"
    return 1
  }
  mv -f "$temporary" "$run_file"
}

wf_run_set_state() { # <run-dir> <status> <stage-id> <message>
  local run_dir=$1 status=$2 stage=$3 message=$4 now
  now=$(wf_now)
  wf_run_update "$run_dir" \
    --arg status "$status" --arg stage "$stage" --arg message "$message" --arg now "$now" \
    '.status=$status | .current_stage=$stage | .message=$message | .updated_at=$now'
}

wf_backlog_show_field() { # <show-output> <field>
  printf '%s\n' "$1" | sed -n "s/^  $2: //p" | head -1
}

wf_register_run_backlog() { # <home> <run> <workflow> <description>
  local home=$1 run=$2 workflow=$3 description=$4 backlog="$1/data/backlog.md" show
  [ -f "$backlog" ] || return 0
  # shellcheck source=bin/mx-backlog-lib.sh
  if ! command -v mx_backlog_show >/dev/null 2>&1; then
    return 1
  fi
  if show=$(mx_backlog_show "$backlog" "$run" 2>/dev/null); then
    wf_error "backlog identity already exists: $run"
    return 1
  fi
  mx_backlog_add "$backlog" "$run" "Workflow $workflow: $description" \
    --kind delivery --repo broker --start >/dev/null
}

wf_complete_run_backlog() { # <home> <run>
  local home=$1 run=$2 backlog="$1/data/backlog.md" show state
  [ -f "$backlog" ] || return 0
  show=$(mx_backlog_show "$backlog" "$run" 2>/dev/null) || return 1
  state=$(wf_backlog_show_field "$show" state)
  [ "$state" != done ] || return 0
  mx_backlog_done "$backlog" "$run" --note "workflow completed" >/dev/null
}

wf_hold_state() { # <home> <run> <key>
  local home=$1 run=$2 key=$3 backlog="$1/data/backlog.md" hold_id show state
  [ -f "$backlog" ] || {
    printf 'absent\n'
    return 0
  }
  hold_id="$run-decision-$key"
  if ! show=$(mx_backlog_show "$backlog" "$hold_id" 2>/dev/null); then
    printf 'absent\n'
    return 0
  fi
  state=$(wf_backlog_show_field "$show" state)
  case "$state" in
    done) printf 'resolved\n' ;;
    queued) printf 'open\n' ;;
    *) printf 'invalid\n' ;;
  esac
}

wf_create_hold() { # <home> <run> <key> <title> <reason>
  local home=$1 run=$2 key=$3 title=$4 reason=$5 backlog="$1/data/backlog.md" hold_id
  [ -f "$backlog" ] || {
    wf_error "approval requires an initialized backlog at $backlog"
    return 1
  }
  MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" MX_DATA_OVERRIDE="$home/data" \
    "$WF_SCRIPT_DIR/mx-decision-hold.sh" hold "$run" "$key" \
      --title "$title" --reason "$reason" --repo broker >/dev/null || return 1
  hold_id="$run-decision-$key"
  if mx_backlog_show "$backlog" "$run" | grep -F "blocked_by: " | grep -F "$hold_id" >/dev/null 2>&1; then
    return 0
  fi
  mx_backlog_block "$backlog" "$run" --by "$hold_id" >/dev/null
}

wf_attach_command_failure() { # <home> <run> <stage-id> <stage-record>
  local home=$1 run=$2 stage_id=$3 record=$4 backlog hold_id stdout stderr exit_code
  local stdout_tail stderr_tail body
  backlog="$home/data/backlog.md"
  hold_id="$run-decision-$stage_id-failure"
  [ -f "$backlog" ] && [ -f "$record" ] || return 1
  stdout=$(jq -r '.stdout // empty' "$record")
  stderr=$(jq -r '.stderr // empty' "$record")
  exit_code=$(jq -r '.exit_code // empty' "$record")
  stdout_tail=$(tail -50 "$stdout" 2>/dev/null | tail -c 4096 || true)
  stderr_tail=$(tail -50 "$stderr" 2>/dev/null | tail -c 4096 || true)
  body=$(printf 'Origin: %s\nDecision key: %s-failure\nState: awaiting maintainer decision.\nCommand exit: %s\nCaptured stdout: %s\nCaptured stderr: %s\n\nStdout tail:\n%s\n\nStderr tail:\n%s' \
    "$run" "$stage_id" "$exit_code" "$stdout" "$stderr" \
    "${stdout_tail:-[empty]}" "${stderr_tail:-[empty]}")
  mx_backlog_update "$backlog" "$hold_id" --body "$body" >/dev/null
}

wf_contract_check() { # <run-dir> <stage-json> <stage-record>
  local run_dir=$1 stage_json=$2 record=$3 home run output contract type stage_id
  local task_id worktree fork_sha head branch stdout stderr
  home=$(jq -r '.home' "$run_dir/run.json")
  run=$(jq -r '.run' "$run_dir/run.json")
  output=$(printf '%s\n' "$stage_json" | jq -r '.output // empty')
  contract=$(printf '%s\n' "$stage_json" | jq -r '.contract // empty')
  type=$(printf '%s\n' "$stage_json" | jq -r '.type')
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  if [ "$type" = command ]; then
    stdout="$run_dir/commands/$stage_id.stdout"
    stderr="$run_dir/commands/$stage_id.stderr"
    [ -f "$record" ] && [ ! -L "$record" ] \
      && [ -f "$stdout" ] && [ ! -L "$stdout" ] \
      && [ -f "$stderr" ] && [ ! -L "$stderr" ] \
      && jq -e --arg stdout "$stdout" --arg stderr "$stderr" \
        '.exit_code == 0 and .stdout == $stdout and .stderr == $stderr' \
        "$record" >/dev/null 2>&1 || return 1
  fi
  if [ -n "$output" ]; then
    output=$(wf_output_path "$home" "$output" "$run") || return 1
    [ -s "$output" ] || return 1
  fi
  if [ "$contract" = local-commits ]; then
    [ -f "$record" ] || return 1
    task_id=$(jq -r '.task_id // empty' "$record")
    worktree=$(jq -r '.worktree // empty' "$record")
    fork_sha=$(jq -r '.fork_sha // empty' "$record")
    [ -n "$task_id" ] && [ -d "$worktree" ] && [ -n "$fork_sha" ] || return 1
    head=$(git -C "$worktree" rev-parse --verify HEAD 2>/dev/null) || return 1
    branch=$(git -C "$worktree" symbolic-ref --quiet --short HEAD 2>/dev/null) || return 1
    [ "$branch" = "mx/$task_id" ] \
      && [ "$head" != "$fork_sha" ] \
      && git -C "$worktree" merge-base --is-ancestor "$fork_sha" "$head" 2>/dev/null
  fi
}

wf_prompt_file() { # <run-dir> <stage-json>
  local run_dir=$1 stage_json=$2 run input home stage_id output body prompt source source_output source_path
  run=$(jq -r '.run' "$run_dir/run.json")
  input=$(cat "$run_dir/input.txt")
  home=$(jq -r '.home' "$run_dir/run.json")
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  output=$(printf '%s\n' "$stage_json" | jq -r '.output // empty')
  if [ -n "$output" ]; then
    output=$(wf_output_path "$home" "$output" "$run") || return 1
  fi
  body=$(printf '%s\n' "$stage_json" | jq -r '.body')
  body=$(wf_substitute "$body" "$run" "$input" "$output")
  prompt="$run_dir/prompts/$stage_id.md"
  {
    printf '# Workflow stage: %s\n\n' "$(printf '%s\n' "$stage_json" | jq -r '.title')"
    printf 'Run: `%s`\n\n' "$run"
    printf '%s\n' "$body"
    while IFS= read -r source; do
      [ -n "$source" ] || continue
      source_output=$(jq -r --arg id "$source" '.stages[] | select(.id == $id) | .output' \
        "$run_dir/definition.json")
      source_path=$(wf_output_path "$home" "$source_output" "$run") || return 1
      printf '\n## Inherited artifact: %s\n\n' "$source"
      printf 'Path: `%s`\n\n' "$source_path"
      if [ -f "$source_path" ]; then
        sed -n '1,4000p' "$source_path"
      else
        printf '[artifact missing]\n'
      fi
    done <<EOF
$(printf '%s\n' "$stage_json" | jq -r '.brief_from[]?')
EOF
  } | wf_atomic_write "$prompt" 600 || return 1
  printf '%s\n' "$prompt"
}

wf_agent_schema() {
  cat <<'EOF'
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "required": ["status", "message"],
  "properties": {
    "status": {"type": "string", "enum": ["done", "failed"]},
    "message": {"type": "string", "minLength": 1, "maxLength": 12000}
  }
}
EOF
}

wf_agent_result_valid() { # <file>
  jq -e '
    type == "object" and
    ((keys | sort) == ["message","status"]) and
    (.status == "done" or .status == "failed") and
    (.message | type == "string" and length > 0 and length <= 12000)
  ' "$1" >/dev/null 2>&1
}

wf_execute_broker_agent() { # <run-dir> <stage-json>
  local run_dir=$1 stage_json=$2 stage_id record prompt schema output session_out session_id status now
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  record=$(wf_stage_record_path "$run_dir" "$stage_id")
  prompt=$(wf_prompt_file "$run_dir" "$stage_json") || return 1
  schema="$run_dir/schemas/agent-result.json"
  output="$run_dir/agents/$stage_id.json"
  session_out="$run_dir/agents/$stage_id.session"
  wf_agent_schema | wf_atomic_write "$schema" 600 || return 1
  now=$(wf_now)
  jq -n --arg id "$stage_id" --arg now "$now" --arg prompt "$prompt" \
    '{id:$id,status:"running",started_at:$now,prompt:$prompt}' \
    | wf_stage_record_write "$run_dir" "$stage_id" || return 1

  if [ -n "${MX_WORKFLOW_AGENT_COMMAND:-}" ]; then
    "$MX_WORKFLOW_AGENT_COMMAND" --session new --schema "$schema" --prompt "$prompt" \
      --output "$output" --session-out "$session_out"
  else
    # shellcheck source=bin/mx-deep-review-lib.sh
    . "$WF_SCRIPT_DIR/mx-deep-review-lib.sh"
    DR_REPO_ROOT=$(jq -r '.repo' "$run_dir/run.json")
    DR_MX_ROOT=$WF_MX_ROOT
    # The stage body and inherited artifacts are the complete charter.
    # A target project's branch-local instructions must not replace the broker
    # identity or expand this headless workflow stage's authority.
    DR_CONFIG_DISABLE_PROJECT_SETTINGS=true
    dr_agent_oneshot --session new --schema "$schema" --prompt "$prompt" \
      --output "$output" --session-out "$session_out"
  fi || {
    now=$(wf_now)
    jq -n --arg id "$stage_id" --arg now "$now" --arg prompt "$prompt" \
      '{id:$id,status:"failed",finished_at:$now,prompt:$prompt,message:"headless agent failed"}' \
      | wf_stage_record_write "$run_dir" "$stage_id"
    return 1
  }
  wf_agent_result_valid "$output" || {
    wf_error "stage $stage_id returned invalid structured completion"
    return 1
  }
  status=$(jq -r '.status' "$output")
  session_id=$(cat "$session_out")
  now=$(wf_now)
  jq -n --arg id "$stage_id" --arg now "$now" --arg prompt "$prompt" \
    --arg result "$output" --arg session "$session_id" --arg status "$status" \
    '{id:$id,status:$status,finished_at:$now,prompt:$prompt,result:$result,session_id:$session}' \
    | wf_stage_record_write "$run_dir" "$stage_id" || return 1
  [ "$status" = done ]
}

wf_actor_task_id() { # <run-dir> <stage-id>
  local run_dir=$1 stage_id=$2 run used
  run=$(jq -r '.run' "$run_dir/run.json")
  used=$(find "$run_dir/stages" -type f -name '*.json' -maxdepth 1 2>/dev/null \
    -exec jq -r '.task_id // empty' {} \; | grep -Fx "$run" || true)
  if [ -z "$used" ]; then
    printf '%s\n' "$run"
  else
    printf '%s-%s\n' "$run" "$stage_id"
  fi
}

wf_write_actor_brief() { # <run-dir> <stage-json> <task-id>
  local run_dir=$1 stage_json=$2 task_id=$3 home prompt brief
  home=$(jq -r '.home' "$run_dir/run.json")
  prompt=$(wf_prompt_file "$run_dir" "$stage_json") || return 1
  brief="$home/data/$task_id/brief.md"
  mkdir -p "$(dirname "$brief")"
  {
    printf '%s\n\n' 'You are an actor coordinated through a Multplx workflow. Work independently.'
    printf '# Stage charter\n\n'
    cat "$prompt"
    cat <<EOF

# Workflow execution rules

Verify that \`pwd -P\` and \`git rev-parse --show-toplevel\` identify the isolated worktree supplied by Multplx rather than its primary checkout.
Stop and report \`blocked\` if isolation is not genuine.
Create the local task branch with \`git checkout -b mx/$task_id\` before editing.
Work only in that isolated worktree, except for the stage's explicitly declared output path and the validated status command below.
Never push, open a pull request, merge, or invoke credentialed delivery.
Commit every intended project change locally.
Report completion through the validated status path:

\`$WF_MX_ROOT/bin/mx-report --id $task_id --state done --message "workflow stage complete at {full commit SHA}"\`

Report a real failure with state \`failed\`, a maintainer choice with \`needs-decision\`, and a broker-actionable blocker with \`blocked\`.
Do not write the status file directly.
The workflow engine, not this session, decides whether the declared contract is met and whether the next stage may start.
EOF
  } | wf_atomic_write "$brief" 600 || return 1
  printf '%s\n' "$brief"
}

wf_execute_actor_agent() { # <run-dir> <stage-json>
  local run_dir=$1 stage_json=$2 stage_id record status task_id repo brief harness meta worktree fork_sha now
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  record=$(wf_stage_record_path "$run_dir" "$stage_id")
  status=$(wf_stage_record_status "$run_dir" "$stage_id")
  case "$status" in
    waiting-agent|done|failed) return 0 ;;
    running)
      wf_error "stage $stage_id has an incomplete actor launch record"
      return 1
      ;;
  esac
  task_id=$(wf_actor_task_id "$run_dir" "$stage_id")
  repo=$(jq -r '.repo' "$run_dir/run.json")
  brief=$(wf_write_actor_brief "$run_dir" "$stage_json" "$task_id") || return 1
  now=$(wf_now)
  jq -n --arg id "$stage_id" --arg now "$now" --arg task "$task_id" --arg brief "$brief" \
    '{id:$id,status:"running",started_at:$now,task_id:$task,brief:$brief}' \
    | wf_stage_record_write "$run_dir" "$stage_id" || return 1
  if [ -n "${MX_WORKFLOW_SPAWN_COMMAND:-}" ]; then
    "$MX_WORKFLOW_SPAWN_COMMAND" "$task_id" "$repo"
  else
    if [ -n "${MX_WORKFLOW_ACTOR_HARNESS:-}" ]; then
      "$WF_SCRIPT_DIR/mx-spawn.sh" "$task_id" "$repo" --harness "$MX_WORKFLOW_ACTOR_HARNESS"
    else
      "$WF_SCRIPT_DIR/mx-spawn.sh" "$task_id" "$repo"
    fi
  fi || return 1
  meta="$(jq -r '.home' "$run_dir/run.json")/state/$task_id.meta"
  [ -f "$meta" ] || {
    wf_error "actor stage $stage_id spawned without metadata"
    return 1
  }
  worktree=$(sed -n 's/^worktree=//p' "$meta" | tail -1)
  [ -d "$worktree" ] || {
    wf_error "actor stage $stage_id has no worktree"
    return 1
  }
  fork_sha=$(git -C "$worktree" rev-parse --verify HEAD 2>/dev/null) || return 1
  now=$(wf_now)
  jq -n --arg id "$stage_id" --arg now "$now" --arg task "$task_id" \
    --arg brief "$brief" --arg worktree "$worktree" --arg fork "$fork_sha" \
    '{id:$id,status:"waiting-agent",started_at:$now,task_id:$task,brief:$brief,
      worktree:$worktree,fork_sha:$fork}' \
    | wf_stage_record_write "$run_dir" "$stage_id"
}

wf_reconcile_actor() { # <run-dir> <stage-json>
  local run_dir=$1 stage_json=$2 stage_id record task_id home state_line state now
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  record=$(wf_stage_record_path "$run_dir" "$stage_id")
  [ -f "$record" ] || return 1
  task_id=$(jq -r '.task_id // empty' "$record")
  home=$(jq -r '.home' "$run_dir/run.json")
  if [ -n "${MX_WORKFLOW_ACTOR_STATE_COMMAND:-}" ]; then
    state_line=$("$MX_WORKFLOW_ACTOR_STATE_COMMAND" "$task_id" 2>/dev/null || true)
  else
    state_line=$(MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" \
      "$WF_SCRIPT_DIR/mx-actor-state.sh" "$task_id" 2>/dev/null || true)
  fi
  state=$(printf '%s\n' "$state_line" | sed -n 's/^state: \([^ ·]*\).*/\1/p')
  case "$state" in
    done)
      if wf_contract_check "$run_dir" "$stage_json" "$record"; then
        now=$(wf_now)
        jq --arg now "$now" '.status="done" | .finished_at=$now' "$record" \
          | wf_stage_record_write "$run_dir" "$stage_id"
        return 0
      fi
      wf_error "actor stage $stage_id reported done before its contract was met"
      return 2
      ;;
    failed)
      now=$(wf_now)
      jq --arg now "$now" '.status="failed" | .finished_at=$now' "$record" \
        | wf_stage_record_write "$run_dir" "$stage_id"
      return 3
      ;;
    parked|blocked|paused|working|unknown|'') return 1 ;;
    *) return 1 ;;
  esac
}

wf_last_actor_worktree() { # <run-dir>
  local run_dir=$1 definition="$1/definition.json" stage_id record worktree
  while IFS= read -r stage_id; do
    record=$(wf_stage_record_path "$run_dir" "$stage_id")
    [ -f "$record" ] || continue
    worktree=$(jq -r '.worktree // empty' "$record")
    [ -d "$worktree" ] || continue
    printf '%s\n' "$worktree"
    return 0
  done <<EOF
$(jq -r '[.stages[] | select(.type == "agent" and .executor == "actor") | .id] | reverse[]' "$definition")
EOF
  jq -r '.repo' "$run_dir/run.json"
}

wf_last_actor_task_id() { # <run-dir>
  local run_dir=$1 definition="$1/definition.json" stage_id record task_id
  while IFS= read -r stage_id; do
    record=$(wf_stage_record_path "$run_dir" "$stage_id")
    [ -f "$record" ] || continue
    task_id=$(jq -r '.task_id // empty' "$record")
    [ -n "$task_id" ] || continue
    printf '%s\n' "$task_id"
    return 0
  done <<EOF
$(jq -r '[.stages[] | select(.type == "agent" and .executor == "actor") | .id] | reverse[]' "$definition")
EOF
  return 1
}

wf_execute_command() { # <run-dir> <stage-json>
  local run_dir=$1 stage_json=$2 stage_id record run home output command workdir stdout stderr now rc task_id actor_state
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  record=$(wf_stage_record_path "$run_dir" "$stage_id")
  run=$(jq -r '.run' "$run_dir/run.json")
  home=$(jq -r '.home' "$run_dir/run.json")
  output=$(printf '%s\n' "$stage_json" | jq -r '.output // empty')
  if [ -n "$output" ]; then
    output=$(wf_output_path "$home" "$output" "$run") || return 1
    output=$(wf_shell_quote "$output")
  fi
  command=$(printf '%s\n' "$stage_json" | jq -r '.run')
  command=$(wf_substitute "$command" "$run" "" "$output")
  workdir=$(wf_last_actor_worktree "$run_dir") || return 1
  stdout="$run_dir/commands/$stage_id.stdout"
  stderr="$run_dir/commands/$stage_id.stderr"
  now=$(wf_now)
  jq -n --arg id "$stage_id" --arg now "$now" --arg command "$command" \
    --arg cwd "$workdir" --arg stdout "$stdout" --arg stderr "$stderr" \
    '{id:$id,status:"running",started_at:$now,command:$command,cwd:$cwd,
      stdout:$stdout,stderr:$stderr}' \
    | wf_stage_record_write "$run_dir" "$stage_id" || return 1
  task_id=$(wf_last_actor_task_id "$run_dir" 2>/dev/null || true)
  (
    cd "$workdir" || exit 1
    MX_WORKFLOW_HOME="$home" MX_WORKFLOW_RUN="$run" MX_WORKFLOW_WORKTREE="$workdir" \
      MX_TASK_ID="${task_id:-$run}" bash -lc "$command"
  ) >"$stdout" 2>"$stderr"
  rc=$?
  now=$(wf_now)
  jq --arg now "$now" --argjson rc "$rc" \
    '.finished_at=$now | .exit_code=$rc' "$record" \
    | wf_stage_record_write "$run_dir" "$stage_id" || return 1
  if [ "$rc" -eq 0 ] && wf_contract_check "$run_dir" "$stage_json" "$record"; then
    jq --arg now "$now" \
      '.status="done" | .finished_at=$now' "$record" \
      | wf_stage_record_write "$run_dir" "$stage_id"
    return 0
  fi
  if [ -n "$task_id" ]; then
    if [ -n "${MX_WORKFLOW_ACTOR_STATE_COMMAND:-}" ]; then
      actor_state=$("$MX_WORKFLOW_ACTOR_STATE_COMMAND" "$task_id" 2>/dev/null || true)
    else
      actor_state=$(MX_HOME="$home" MX_STATE_OVERRIDE="$home/state" \
        "$WF_SCRIPT_DIR/mx-actor-state.sh" "$task_id" 2>/dev/null || true)
    fi
    case "$actor_state" in
      state:\ parked*|state:\ blocked*|state:\ paused*)
        jq --arg now "$now" --argjson rc "$rc" \
          '.status="waiting-external" | .finished_at=$now | .exit_code=$rc' "$record" \
          | wf_stage_record_write "$run_dir" "$stage_id"
        return 4
        ;;
    esac
  fi
  jq --arg now "$now" --argjson rc "$rc" \
    '.status="waiting-failure" | .finished_at=$now | .exit_code=$rc' "$record" \
    | wf_stage_record_write "$run_dir" "$stage_id"
  return 3
}

wf_gate_stage() { # <run-dir> <stage-json>
  local run_dir=$1 stage_json=$2 stage_id gate home run hold_state record status now
  stage_id=$(printf '%s\n' "$stage_json" | jq -r '.id')
  gate=$(printf '%s\n' "$stage_json" | jq -r '.gate')
  [ "$gate" = approve ] || return 0
  home=$(jq -r '.home' "$run_dir/run.json")
  run=$(jq -r '.run' "$run_dir/run.json")
  record=$(wf_stage_record_path "$run_dir" "$stage_id")
  hold_state=$(wf_hold_state "$home" "$run" "$stage_id")
  case "$hold_state" in
    resolved)
      wf_contract_check "$run_dir" "$stage_json" "$record" || return 2
      now=$(wf_now)
      if [ -f "$record" ]; then
        jq --arg now "$now" '.status="passed" | .approved_at=$now' "$record" \
          | wf_stage_record_write "$run_dir" "$stage_id"
      else
        jq -n --arg id "$stage_id" --arg now "$now" \
          '{id:$id,status:"passed",approved_at:$now}' \
          | wf_stage_record_write "$run_dir" "$stage_id"
      fi
      return 0
      ;;
    open) return 1 ;;
    absent)
      wf_create_hold "$home" "$run" "$stage_id" \
        "Approve workflow stage $stage_id" \
        "workflow stage $stage_id awaits approval" || return 2
      now=$(wf_now)
      if [ -f "$record" ]; then
        jq --arg now "$now" '.status="waiting-approval" | .gate_opened_at=$now' "$record" \
          | wf_stage_record_write "$run_dir" "$stage_id"
      else
        jq -n --arg id "$stage_id" --arg now "$now" \
          '{id:$id,status:"waiting-approval",gate_opened_at:$now}' \
          | wf_stage_record_write "$run_dir" "$stage_id"
      fi
      return 1
      ;;
    *) return 2 ;;
  esac
}

wf_mark_stage_passed() { # <run-dir> <stage-id>
  local run_dir=$1 stage_id=$2 record now
  record=$(wf_stage_record_path "$run_dir" "$stage_id")
  now=$(wf_now)
  if [ -f "$record" ]; then
    jq --arg now "$now" '.status="passed" | .passed_at=$now' "$record" \
      | wf_stage_record_write "$run_dir" "$stage_id"
  else
    jq -n --arg id "$stage_id" --arg now "$now" \
      '{id:$id,status:"passed",passed_at:$now}' \
      | wf_stage_record_write "$run_dir" "$stage_id"
  fi
}

wf_stage_order() { # <run-dir>
  local run_dir=$1 order="$1/stage-order.json"
  if [ -f "$order" ] && [ ! -L "$order" ]; then
    jq -er --slurpfile definition "$run_dir/definition.json" '
      select(type == "array" and all(.[]; type == "string") and length == ($definition[0].stages | length)) |
      select((unique | length) == length) |
      select((sort) == ([$definition[0].stages[].id] | sort)) | .[]
    ' "$order" 2>/dev/null
  else
    jq -r '.stages[].id' "$run_dir/definition.json"
  fi
}

wf_mark_stage_skipped() { # <run-dir> <stage-id> <override-request>
  local run_dir=$1 stage_id=$2 request=$3 now
  now=$(wf_now)
  jq -n --arg id "$stage_id" --arg request "$request" --arg now "$now" \
    '{id:$id,status:"skipped",exception:"maintainer-directed",override_request:$request,skipped_at:$now}' \
    | wf_stage_record_write "$run_dir" "$stage_id"
}

wf_assert_stage_order() { # <run-dir>
  local run_dir=$1 seen_unmet=0 stage_id status
  while IFS= read -r stage_id; do
    status=$(wf_stage_record_status "$run_dir" "$stage_id")
    if [ "$status" = skipped ]; then
      continue
    fi
    if [ "$status" = passed ]; then
      [ "$seen_unmet" -eq 0 ] || {
        wf_error "out-of-order passed record for stage $stage_id"
        return 1
      }
    else
      seen_unmet=1
    fi
  done <<EOF
$(wf_stage_order "$run_dir")
EOF
}

wf_reconcile_run() { # <run-dir>
  local run_dir=$1 stage_id stage_json type executor gate record status gate_rc exec_rc prompt
  [ -f "$run_dir/run.json" ] && [ -f "$run_dir/definition.json" ] || {
    wf_error "invalid workflow run directory: $run_dir"
    return 1
  }
  status=$(jq -r '.status' "$run_dir/run.json")
  case "$status" in
    aborted) wf_error "run $(jq -r '.run' "$run_dir/run.json") is permanently aborted"; return 1 ;;
    completed) return 0 ;;
  esac
  wf_verify_snapshot "$run_dir" || return 1
  wf_assert_stage_order "$run_dir" || return 1

  while IFS= read -r stage_id; do
    wf_verify_snapshot "$run_dir" || return 1
    stage_json=$(wf_stage_json "$run_dir/definition.json" "$stage_id") || return 1
    record=$(wf_stage_record_path "$run_dir" "$stage_id")
    status=$(wf_stage_record_status "$run_dir" "$stage_id")
    if [ "$status" = skipped ]; then
      continue
    fi
    if [ "$status" = passed ]; then
      wf_contract_check "$run_dir" "$stage_json" "$record" || {
        wf_run_set_state "$run_dir" failed "$stage_id" "passed stage contract no longer holds"
        return 1
      }
      continue
    fi
    wf_run_set_state "$run_dir" running "$stage_id" "reconciling stage $stage_id" || return 1
    if [ "$status" = pending ] && command -v wf_journal_stage_entered >/dev/null 2>&1; then
      wf_journal_stage_entered "$run_dir" "$stage_id" || true
    fi
    type=$(printf '%s\n' "$stage_json" | jq -r '.type')
    gate=$(printf '%s\n' "$stage_json" | jq -r '.gate')

    case "$type" in
      interactive)
        if [ "$status" = pending ]; then
          prompt=$(wf_prompt_file "$run_dir" "$stage_json") || return 1
          jq -n --arg id "$stage_id" --arg prompt "$prompt" --arg now "$(wf_now)" \
            '{id:$id,status:"ready",prompt:$prompt,started_at:$now}' \
            | wf_stage_record_write "$run_dir" "$stage_id" || return 1
        fi
        wf_gate_stage "$run_dir" "$stage_json"
        gate_rc=$?
        case "$gate_rc" in
          0)
            wf_mark_stage_passed "$run_dir" "$stage_id" || return 1
            command -v wf_journal_stage_gated >/dev/null 2>&1 \
              && wf_journal_stage_gated "$run_dir" "$stage_id" approve passed || true
            ;;
          1)
            wf_run_set_state "$run_dir" waiting "$stage_id" \
              "maintainer approval required; resolve $stage_id through mx-decision-hold"
            command -v wf_journal_stage_gated >/dev/null 2>&1 \
              && wf_journal_stage_gated "$run_dir" "$stage_id" approve waiting || true
            return 0
            ;;
          *)
            wf_run_set_state "$run_dir" failed "$stage_id" "interactive gate failed"
            command -v wf_journal_stage_gated >/dev/null 2>&1 \
              && wf_journal_stage_gated "$run_dir" "$stage_id" approve failed || true
            return 1
            ;;
        esac
        ;;
      agent)
        executor=$(printf '%s\n' "$stage_json" | jq -r '.executor')
        if [ "$executor" = broker ]; then
          case "$status" in
            done|waiting-approval) ;;
            failed)
              wf_run_set_state "$run_dir" failed "$stage_id" "broker agent stage failed"
              return 1
              ;;
            *) wf_execute_broker_agent "$run_dir" "$stage_json" || {
              wf_run_set_state "$run_dir" failed "$stage_id" "broker agent stage failed"
              return 1
            } ;;
          esac
        else
          case "$status" in
            pending) wf_execute_actor_agent "$run_dir" "$stage_json" || {
              wf_run_set_state "$run_dir" failed "$stage_id" "actor launch failed"
              return 1
            }
              wf_run_set_state "$run_dir" waiting "$stage_id" "actor stage is still running"
              return 0
              ;;
            waiting-approval) ;;
            waiting-agent|done)
              wf_reconcile_actor "$run_dir" "$stage_json"
              exec_rc=$?
              case "$exec_rc" in
                0) ;;
                1)
                  wf_run_set_state "$run_dir" waiting "$stage_id" "actor stage is still running"
                  return 0
                  ;;
                2)
                  wf_run_set_state "$run_dir" failed "$stage_id" "actor contract is unmet"
                  return 1
                  ;;
                *)
                  wf_run_set_state "$run_dir" failed "$stage_id" "actor reported failure"
                  return 1
                  ;;
              esac
              ;;
            failed)
              wf_run_set_state "$run_dir" failed "$stage_id" "actor reported failure"
              return 1
              ;;
          esac
        fi
        if ! wf_contract_check "$run_dir" "$stage_json" "$record"; then
          wf_run_set_state "$run_dir" failed "$stage_id" "stage contract is unmet"
          return 1
        fi
        if [ "$gate" = approve ]; then
          wf_gate_stage "$run_dir" "$stage_json"
          gate_rc=$?
          case "$gate_rc" in
            0) ;;
            1)
              wf_run_set_state "$run_dir" waiting "$stage_id" "maintainer approval required"
              command -v wf_journal_stage_gated >/dev/null 2>&1 \
                && wf_journal_stage_gated "$run_dir" "$stage_id" approve waiting || true
              return 0
              ;;
            *)
              wf_run_set_state "$run_dir" failed "$stage_id" "approval gate failed"
              command -v wf_journal_stage_gated >/dev/null 2>&1 \
                && wf_journal_stage_gated "$run_dir" "$stage_id" approve failed || true
              return 1
              ;;
          esac
        fi
        wf_mark_stage_passed "$run_dir" "$stage_id" || return 1
        if command -v wf_journal_stage_gated >/dev/null 2>&1; then
          wf_journal_stage_gated "$run_dir" "$stage_id" "$gate" passed || true
        fi
        ;;
      command)
        case "$status" in
          done|waiting-approval) ;;
          waiting-failure)
            case "$(wf_hold_state "$(jq -r '.home' "$run_dir/run.json")" \
              "$(jq -r '.run' "$run_dir/run.json")" "$stage_id-failure")" in
              resolved)
                wf_execute_command "$run_dir" "$stage_json"
                exec_rc=$?
                ;;
              open)
                wf_run_set_state "$run_dir" waiting "$stage_id" \
                  "command failure awaits maintainer decision"
                return 0
                ;;
              *)
                wf_run_set_state "$run_dir" failed "$stage_id" \
                  "command failure hold is missing or invalid"
                return 1
                ;;
            esac
            case "$exec_rc" in
              0) ;;
              4)
                wf_run_set_state "$run_dir" waiting "$stage_id" \
                  "command is waiting on its composed lifecycle"
                return 0
                ;;
              *)
                wf_run_set_state "$run_dir" failed "$stage_id" \
                  "command failed again after the accepted retry"
                return 1
                ;;
            esac
            ;;
          waiting-external)
            wf_execute_command "$run_dir" "$stage_json"
            exec_rc=$?
            [ "$exec_rc" -eq 0 ] || {
              wf_run_set_state "$run_dir" waiting "$stage_id" \
                "command is waiting on its composed lifecycle"
              return 0
            }
            ;;
          failed)
            wf_run_set_state "$run_dir" failed "$stage_id" "command stage failed"
            return 1
            ;;
          *)
            wf_execute_command "$run_dir" "$stage_json"
            exec_rc=$?
            case "$exec_rc" in
              0) ;;
              4)
                wf_run_set_state "$run_dir" waiting "$stage_id" \
                  "command is waiting on its composed lifecycle"
                return 0
                ;;
              *)
                wf_create_hold "$(jq -r '.home' "$run_dir/run.json")" \
                  "$(jq -r '.run' "$run_dir/run.json")" "$stage_id-failure" \
                  "Workflow command $stage_id failed" \
                  "workflow command $stage_id failed; inspect captured output" || true
                wf_attach_command_failure "$(jq -r '.home' "$run_dir/run.json")" \
                  "$(jq -r '.run' "$run_dir/run.json")" "$stage_id" \
                  "$(wf_stage_record_path "$run_dir" "$stage_id")" || true
                wf_run_set_state "$run_dir" waiting "$stage_id" \
                  "command failed; inspect commands/$stage_id.stderr"
                return 0
                ;;
            esac
            ;;
        esac
        if [ "$gate" = approve ]; then
          wf_gate_stage "$run_dir" "$stage_json"
          gate_rc=$?
          case "$gate_rc" in
            0) ;;
            1)
              wf_run_set_state "$run_dir" waiting "$stage_id" "maintainer approval required"
              command -v wf_journal_stage_gated >/dev/null 2>&1 \
                && wf_journal_stage_gated "$run_dir" "$stage_id" approve waiting || true
              return 0
              ;;
            *)
              wf_run_set_state "$run_dir" failed "$stage_id" "approval gate failed"
              command -v wf_journal_stage_gated >/dev/null 2>&1 \
                && wf_journal_stage_gated "$run_dir" "$stage_id" approve failed || true
              return 1
              ;;
          esac
        fi
        wf_mark_stage_passed "$run_dir" "$stage_id" || return 1
        if command -v wf_journal_stage_gated >/dev/null 2>&1; then
          wf_journal_stage_gated "$run_dir" "$stage_id" "$gate" passed || true
        fi
        ;;
    esac
  done <<EOF
$(wf_stage_order "$run_dir")
EOF

  wf_complete_run_backlog "$(jq -r '.home' "$run_dir/run.json")" \
    "$(jq -r '.run' "$run_dir/run.json")" || return 1
  wf_run_set_state "$run_dir" completed "" "workflow completed"
}

wf_definition_tracked() { # <root> <definition>
  local root=$1 definition=$2 relative
  root=$(cd "$root" && pwd -P) || return 1
  definition=$(cd "$(dirname "$definition")" && pwd -P)/$(basename "$definition")
  case "$definition" in
    "$root"/workflows/*.workflow.md) ;;
    *)
      wf_error "runnable definitions must live under $root/workflows"
      return 1
      ;;
  esac
  relative=${definition#"$root"/}
  git -C "$root" ls-files --error-unmatch "$relative" >/dev/null 2>&1 || {
    wf_error "runnable definition is not repo-tracked: $relative"
    return 1
  }
}

wf_create_run() { # <definition> <run-id> <input> <repo> <home> <state>
  local definition=$1 run=$2 input=$3 repo=$4 home=$5 state=$6 run_dir now digest definition_path
  wf_slug_valid "$run" || {
    wf_error "run id must be a privacy-safe slug"
    return 1
  }
  run_dir="$state/$run.workflow"
  [ ! -e "$run_dir" ] && [ ! -L "$run_dir" ] || {
    wf_error "run id already exists: $run"
    return 1
  }
  mkdir -p "$run_dir/stages" "$run_dir/prompts" "$run_dir/schemas" \
    "$run_dir/agents" "$run_dir/commands" || return 1
  chmod 700 "$run_dir" "$run_dir/stages" "$run_dir/prompts" "$run_dir/schemas" \
    "$run_dir/agents" "$run_dir/commands"
  wf_atomic_write "$run_dir/definition.workflow.md" 600 <"$definition" || return 1
  wf_definition_json "$run_dir/definition.workflow.md" \
    | wf_atomic_write "$run_dir/definition.json" 600 || return 1
  jq '[.stages[].id]' "$run_dir/definition.json" \
    | wf_atomic_write "$run_dir/stage-order.json" 600 || return 1
  printf '%s' "$input" | wf_atomic_write "$run_dir/input.txt" 600 || return 1
  digest=$(wf_sha256_file "$run_dir/definition.workflow.md") || return 1
  definition_path=$(cd "$(dirname "$definition")" && pwd -P)/$(basename "$definition")
  repo=$(cd "$repo" && pwd -P) || return 1
  home=$(cd "$home" && pwd -P) || return 1
  now=$(wf_now)
  jq -n --arg run "$run" \
    --arg workflow "$(jq -r '.name' "$run_dir/definition.json")" \
    --arg definition "$definition_path" --arg digest "$digest" \
    --arg repo "$repo" --arg home "$home" --arg now "$now" \
    '{
      version:1,run:$run,workflow:$workflow,definition_path:$definition,
      definition_sha256:$digest,repo:$repo,home:$home,status:"running",
      current_stage:null,message:"workflow launched",created_at:$now,updated_at:$now
    }' | wf_atomic_write "$run_dir/run.json" 600 || return 1
  wf_register_run_backlog "$home" "$run" "$(jq -r '.name' "$run_dir/definition.json")" \
    "$(jq -r '.description' "$run_dir/definition.json")" || return 1
  printf '%s\n' "$run_dir"
}

wf_status_render() { # <run-dir>
  local run_dir=$1 stage_id record stage_json output home run
  jq -r '
    "run: \(.run)",
    "workflow: \(.workflow)",
    "status: \(.status)",
    "current_stage: \(.current_stage // "-")",
    "message: \(.message)",
    "definition_sha256: \(.definition_sha256)"
  ' "$run_dir/run.json"
  home=$(jq -r '.home' "$run_dir/run.json")
  run=$(jq -r '.run' "$run_dir/run.json")
  printf 'stages:\n'
  while IFS= read -r stage_id; do
    record=$(wf_stage_record_path "$run_dir" "$stage_id")
    stage_json=$(wf_stage_json "$run_dir/definition.json" "$stage_id")
    printf '  %s: %s\n' "$stage_id" "$(wf_stage_record_status "$run_dir" "$stage_id")"
    output=$(printf '%s\n' "$stage_json" | jq -r '.output // empty')
    if [ -n "$output" ]; then
      printf '    output: %s\n' "$(wf_output_path "$home" "$output" "$run")"
    fi
    if [ -f "$record" ]; then
      jq -r 'select(.task_id != null) | "    task_id: \(.task_id)"' "$record"
      jq -r 'select(.prompt != null) | "    prompt: \(.prompt)"' "$record"
      jq -r 'select(.stdout != null) | "    stdout: \(.stdout)\n    stderr: \(.stderr)"' "$record"
    fi
  done <<EOF
$(wf_stage_order "$run_dir")
EOF
}
