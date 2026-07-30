#!/usr/bin/env bash
# Run or resume the local deep-review validation gate.
#
# Usage:
#   mx-deep-review.sh <task-id> (--intent <text> | --intent-file <path>)
#                     [--base <branch>] [--title <pull-request-title>]
#   mx-deep-review.sh respond <task-id> --decision <key> --answer <text>
#
# The initiating actor owns every run and respond invocation.
# The broker and the credentialed delivery service never invoke this script.
# A successful run ends at a clean, validated local mx/<task-id> branch and
# writes state/<task-id>.ready-to-push with approval=pending.
# It never pushes, opens a pull request, merges, or watches remote CI.
#
# State under state/<task-id>.gate is restart-reconstructable.
# Every record write is an atomic whole-file replacement.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"

# shellcheck source=bin/mx-pr-lib.sh
. "$SCRIPT_DIR/mx-pr-lib.sh"
# shellcheck source=bin/mx-deliver-lib.sh
. "$SCRIPT_DIR/mx-deliver-lib.sh"
# shellcheck source=bin/mx-deep-review-lib.sh
. "$SCRIPT_DIR/mx-deep-review-lib.sh"

DR_MX_ROOT=$MX_ROOT
DR_MAX_ROUNDS=${MX_DEEP_REVIEW_MAX_ROUNDS:-5}
case "$DR_MAX_ROUNDS" in ''|*[!0-9]*) DR_MAX_ROUNDS=5 ;; esac
[ "$DR_MAX_ROUNDS" -ge 1 ] || DR_MAX_ROUNDS=1

usage() {
  sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//' >&2
}

fail() {
  printf 'deep-review: %s\n' "$*" >&2
  exit 1
}

json_write() { # <file> <jq args/filter...>
  local file=$1
  shift
  jq "$@" "$RUN_FILE" | dr_atomic_write "$file" 600
}

default_branch() {
  local ref
  ref=$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)
  if [ -n "$ref" ]; then
    ref=${ref#origin/}
    if git show-ref --verify --quiet "refs/heads/$ref"; then
      printf '%s\n' "$ref"
    else
      printf 'origin/%s\n' "$ref"
    fi
  elif git show-ref --verify --quiet refs/heads/main; then
    printf 'main\n'
  elif git show-ref --verify --quiet refs/heads/master; then
    printf 'master\n'
  else
    return 1
  fi
}

task_ownership_valid() { # <id>
  local id=$1 meta worktree cwd
  [ "${MX_TASK_ID:-}" = "$id" ] || return 1
  meta="$STATE/$id.meta"
  [ -f "$meta" ] || return 1
  worktree=$(sed -n 's/^worktree=//p' "$meta" | head -1)
  [ -n "$worktree" ] && [ -d "$worktree" ] || return 1
  cwd=$(pwd -P)
  [ "$cwd" = "$(cd "$worktree" && pwd -P)" ]
}

record_update() { # <jq filter> [jq args before filter are unsupported]
  local filter=$1 tmp
  tmp=$(mktemp "$GATE_DIR/.run.tmp.XXXXXX") || return 1
  if ! jq "$filter" "$RUN_FILE" > "$tmp"; then
    rm -f "$tmp"
    return 1
  fi
  chmod 600 "$tmp" || {
    rm -f "$tmp"
    return 1
  }
  mv -f "$tmp" "$RUN_FILE"
}

record_update_args() { # <jq args...> -- <filter>
  local filter tmp arg
  local -a args
  tmp=$(mktemp "$GATE_DIR/.run.tmp.XXXXXX") || return 1
  args=()
  while [ "$#" -gt 0 ]; do
    arg=$1
    shift
    if [ "$arg" = -- ]; then
      break
    fi
    args+=("$arg")
  done
  filter=${1:?missing jq filter}
  if ! jq "${args[@]}" "$filter" "$RUN_FILE" > "$tmp"; then
    rm -f "$tmp"
    return 1
  fi
  chmod 600 "$tmp" || {
    rm -f "$tmp"
    return 1
  }
  mv -f "$tmp" "$RUN_FILE"
}

current_head() {
  git rev-parse --verify HEAD 2>/dev/null
}

record_head() {
  local head
  head=$(current_head) || return 1
  record_update_args --arg head "$head" -- '.approved_head=$head'
}

step_start() { # <step>
  local step=$1 head
  head=$(current_head) || return 1
  record_update_args --arg step "$step" --arg head "$head" -- \
    '.step=$step | .status="running" | .approved_head=$head |
     .steps[$step]="running" |
     if (.history[-1] // "") == $step then . else .history += [$step] end'
}

step_complete() { # <step> <next-or-empty>
  local step=$1 next=${2:-} head
  head=$(current_head) || return 1
  if [ -n "$next" ]; then
    record_update_args --arg step "$step" --arg next "$next" --arg head "$head" -- \
      '.steps[$step]="passed" | .step=$next | .round=1 | .approved_head=$head'
  else
    record_update_args --arg step "$step" --arg head "$head" -- \
      '.steps[$step]="passed" | .approved_head=$head'
  fi
}

write_schema() { # <review|test|summary>
  local name=$1 path
  path="$GATE_DIR/schemas/$name.json"
  case "$name" in
    review) dr_review_schema ;;
    test) dr_test_schema ;;
    summary) dr_summary_schema ;;
    *) return 1 ;;
  esac | dr_atomic_write "$path" 600 || return 1
  printf '%s\n' "$path"
}

sessions_set() { # <role> <session-id>
  local role=$1 session_id=$2 tmp
  tmp=$(mktemp "$GATE_DIR/.sessions.tmp.XXXXXX") || return 1
  jq --arg role "$role" --arg id "$session_id" '.[$role]=$id' \
    "$GATE_DIR/sessions.json" > "$tmp" || {
    rm -f "$tmp"
    return 1
  }
  chmod 600 "$tmp" || {
    rm -f "$tmp"
    return 1
  }
  mv -f "$tmp" "$GATE_DIR/sessions.json"
}

call_agent() { # <step> <assess|fix> <review|test|summary> <role>
  local step=$1 mode=$2 schema_name=$3 role=$4
  local schema prompt output session_out attempt session_id reviewer_id
  schema=$(write_schema "$schema_name") || return 1
  prompt="$GATE_DIR/prompts/${step}-round-$(printf '%02d' "$ROUND")-${mode}.txt"
  output="$GATE_DIR/findings/round-$(printf '%02d' "$ROUND")-${step}-${mode}-raw.json"
  session_out="$GATE_DIR/.session-current"
  DR_BRANCH=$BRANCH
  DR_BASE_SHA=$(git rev-parse "$DEFAULT_BRANCH" 2>/dev/null || true)
  DR_HEAD_SHA=$(current_head)
  DR_DEFAULT_BRANCH=$DEFAULT_BRANCH
  DR_GATE_DIR=$GATE_DIR
  DR_INTENT_FILE=$GATE_DIR/intent.txt
  DR_REPO_ROOT=$REPO_ROOT
  export DR_BRANCH DR_BASE_SHA DR_HEAD_SHA DR_DEFAULT_BRANCH DR_GATE_DIR
  export DR_INTENT_FILE DR_REPO_ROOT DR_MX_ROOT
  dr_prompt "$step" "$mode" | dr_atomic_write "$prompt" 600 || return 1

  attempt=1
  while [ "$attempt" -le "$DR_MAX_AGENT_ATTEMPTS" ]; do
    rm -f "$output" "$session_out"
    if dr_agent_oneshot --session new --schema "$schema" --prompt "$prompt" \
      --output "$output" --session-out "$session_out" \
      && dr_validate_json "$schema_name" "$output"; then
      session_id=$(cat "$session_out")
      [ -n "$session_id" ] || return 1
      if [ "$step" = review ] && [ "$mode" = fix ]; then
        reviewer_id=$(jq -r --arg role "review-assess-r$ROUND" '.[$role] // empty' \
          "$GATE_DIR/sessions.json")
        if [ -n "$reviewer_id" ] && [ "$reviewer_id" = "$session_id" ]; then
          printf 'deep-review: refusing reviewer/fixer session reuse (%s)\n' "$session_id" >&2
          return 1
        fi
      fi
      sessions_set "$role" "$session_id" || return 1
      AGENT_OUTPUT=$output
      return 0
    fi
    printf 'deep-review: %s %s returned invalid structured output (attempt %s/%s)\n' \
      "$step" "$mode" "$attempt" "$DR_MAX_AGENT_ATTEMPTS" >&2
    attempt=$((attempt + 1))
  done
  return 1
}

commit_if_dirty() { # <subject-fragment>
  local subject=$1
  if [ -n "$(git status --porcelain)" ]; then
    git add -A || return 1
    if ! git diff --cached --quiet; then
      git commit -m "fix: deep-review $subject" >/dev/null || return 1
    fi
  fi
  record_head
}

report_status() { # <state> <message> [key]
  local state=$1 message=$2 key=${3:-}
  if [ -n "$key" ]; then
    MX_HOME="$MX_HOME" MX_STATE_OVERRIDE="$STATE" \
      "$SCRIPT_DIR/mx-report" --id "$ID" --state "$state" \
      --message "$message" --key "$key"
  else
    MX_HOME="$MX_HOME" MX_STATE_OVERRIDE="$STATE" \
      "$SCRIPT_DIR/mx-report" --id "$ID" --state "$state" \
      --message "$message"
  fi
}

park_for_findings() { # <step> <processed-findings-file>
  local step=$1 file=$2 ids key message
  ids=$(jq -r '[.findings[]? | select(.action == "ask-user" or .severity == "error") | .id] | join("-")' "$file")
  [ -n "$ids" ] || ids=decision
  ids=$(printf '%s' "$ids" | tr -cd 'A-Za-z0-9._-' | cut -c1-80)
  key="deep-review-${step}-r${ROUND}-${ids}"
  message="deep-review $step round $ROUND finding $ids"
  record_update_args --arg key "$key" --arg step "$step" -- \
    '.status="parked" | .step=$step | .pending_decision_key=$key |
     .decision_ready=false | .steps[$step]="parked"'
  report_status needs-decision "$message" "$key" || {
    printf 'deep-review: run parked but validated decision report failed\n' >&2
    return 1
  }
  printf 'deep-review: parked for decision %s\n' "$key"
  return 10
}

process_agent_findings() { # <step> <raw-file>
  local step=$1 raw=$2 processed
  processed="$GATE_DIR/findings/round-$(printf '%02d' "$ROUND")-${step}.json"
  dr_strip_deferred_delivery_findings < "$raw" | dr_atomic_write "$processed" 600 \
    || return 1
  PROCESSED_FINDINGS=$processed
  if jq -e 'any(.findings[]?; .action == "ask-user")' "$processed" >/dev/null; then
    park_for_findings "$step" "$processed"
    return $?
  fi
  if dr_has_blocking_findings < "$processed"; then
    return 20
  fi
  return 0
}

consume_decision_if_ready() { # <step>
  local step=$1 ready
  ready=$(jq -r '.decision_ready // false' "$RUN_FILE")
  [ "$ready" = true ] || return 1
  record_update_args --arg step "$step" -- \
    '.decision_ready=false | .pending_decision_key=null |
     .status="running" | .steps[$step]="running"'
  return 0
}

run_intent_step() {
  step_start intent || return 1
  [ -s "$GATE_DIR/intent.txt" ] || return 1
  step_complete intent rebase
}

rebase_conflict_finding() {
  cat <<EOF | dr_atomic_write "$GATE_DIR/findings/round-$(printf '%02d' "$ROUND")-rebase.json" 600
{"findings":[{"id":"rebase-conflict","file":".git","line":1,"severity":"error","action":"ask-user","review_scope":"source","message":"Rebase onto $DEFAULT_BRANCH conflicted and requires an authority-guided resolution."}],"risk_level":"high","risk_rationale":"The branch cannot be validated against the current base until the conflict is resolved.","risk_scope":"rebase"}
EOF
}

run_rebase_step() {
  local finding
  step_start rebase || return 1
  consume_decision_if_ready rebase || true
  if ! git rebase "$DEFAULT_BRANCH"; then
    git rebase --abort >/dev/null 2>&1 || true
    rebase_conflict_finding || return 1
    finding="$GATE_DIR/findings/round-$(printf '%02d' "$ROUND")-rebase.json"
    park_for_findings rebase "$finding"
    return $?
  fi
  record_head || return 1
  step_complete rebase review
}

run_review_step() {
  local before before_head rc risk summary rationale
  step_start review || return 1
  ROUND=$(jq -r '.round' "$RUN_FILE")

  if consume_decision_if_ready review; then
    call_agent review fix summary "review-fix-r$ROUND" || return 1
    commit_if_dirty "review round $ROUND" || return 1
    ROUND=$((ROUND + 1))
    record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
  fi

  while [ "$ROUND" -le "$DR_MAX_ROUNDS" ]; do
    before=$(git status --porcelain)
    before_head=$(current_head)
    call_agent review assess review "review-assess-r$ROUND" || return 1
    [ "$(git status --porcelain)" = "$before" ] \
      && [ "$(current_head)" = "$before_head" ] || {
      printf 'deep-review: reviewer modified the worktree; refusing self-review\n' >&2
      return 1
    }
    if process_agent_findings review "$AGENT_OUTPUT"; then
      rc=0
    else
      rc=$?
    fi
    risk=$(jq -r '.risk_level' "$PROCESSED_FINDINGS")
    rationale=$(jq -r '.risk_rationale' "$PROCESSED_FINDINGS")
    summary=$(jq -r '[.findings[]?.message] | if length == 0 then "Deep review found no blocking source findings." else join(" ") end' "$PROCESSED_FINDINGS")
    record_update_args --arg risk "$risk" --arg rationale "$rationale" \
      --arg summary "$summary" -- \
      '.risk_level=$risk | .risk_rationale=$rationale | .summary=$summary' || return 1
    case "$rc" in
      0)
        step_complete review test
        return
        ;;
      10) return 10 ;;
      20)
        if ! jq -e 'any(.findings[]?; .action == "auto-fix")' "$PROCESSED_FINDINGS" >/dev/null; then
          park_for_findings review "$PROCESSED_FINDINGS"
          return $?
        fi
        call_agent review fix summary "review-fix-r$ROUND" || return 1
        commit_if_dirty "review round $ROUND" || return 1
        ROUND=$((ROUND + 1))
        record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
        ;;
      *) return "$rc" ;;
    esac
  done
  printf 'deep-review: review exceeded %s fix rounds\n' "$DR_MAX_ROUNDS" >&2
  return 1
}

write_command_record() { # <name> <command> <exit> <output-file>
  local name=$1 command=$2 exit_code=$3 output_file=$4 record
  record="$GATE_DIR/cmd-output/$name.json"
  jq -n --arg command "$command" --arg source "$DR_CONFIG_COMMAND_SOURCE" \
    --arg output "$output_file" --argjson exit_code "$exit_code" \
    '{command:$command,command_source:$source,exit_code:$exit_code,output:$output}' \
    | dr_atomic_write "$record" 600
}

run_configured_command() { # <test|format|lint> <command>
  local name=$1 command=$2 output exit_code
  output="$GATE_DIR/cmd-output/${name}-round-$(printf '%02d' "$ROUND").log"
  set +e
  bash -lc "$command" > "$output" 2>&1
  exit_code=$?
  set -e
  chmod 600 "$output"
  write_command_record "$name" "$command" "$exit_code" "$output" || return 1
  COMMAND_EXIT=$exit_code
  COMMAND_OUTPUT=$output
}

write_command_finding() { # <test|format|lint> <exit> <output>
  local name=$1 exit_code=$2 output=$3 file
  file="$GATE_DIR/findings/round-$(printf '%02d' "$ROUND")-${name}-command.json"
  jq -n --arg id "${name}-command-failed" --arg output "$output" \
    --arg name "$name" --argjson exit_code "$exit_code" \
    '{findings:[{id:$id,file:$output,line:1,severity:"error",action:"auto-fix",review_scope:"source",message:($name+" command exited "+($exit_code|tostring)+"; captured output: "+$output)}]}' \
    | dr_atomic_write "$file" 600
}

run_test_step() {
  local rc
  step_start test || return 1
  ROUND=$(jq -r '.round' "$RUN_FILE")
  if consume_decision_if_ready test; then
    call_agent test fix summary "test-fix-r$ROUND" || return 1
    commit_if_dirty "test round $ROUND" || return 1
    ROUND=$((ROUND + 1))
    record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
  fi

  while [ "$ROUND" -le "$DR_MAX_ROUNDS" ]; do
    if [ -n "$DR_CONFIG_TEST" ]; then
      run_configured_command test "$DR_CONFIG_TEST" || return 1
      if [ "$COMMAND_EXIT" -ne 0 ]; then
        write_command_finding test "$COMMAND_EXIT" "$COMMAND_OUTPUT" || return 1
        call_agent test fix summary "test-fix-r$ROUND" || return 1
        commit_if_dirty "test round $ROUND" || return 1
        ROUND=$((ROUND + 1))
        record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
        continue
      fi
    else
      printf 'no test command configured, asking agent to run tests…\n'
    fi

    call_agent test assess test "test-assess-r$ROUND" || return 1
    commit_if_dirty "test evidence round $ROUND" || return 1
    if process_agent_findings test "$AGENT_OUTPUT"; then
      rc=0
    else
      rc=$?
    fi
    case "$rc" in
      0)
        step_complete test document
        return
        ;;
      10) return 10 ;;
      20)
        call_agent test fix summary "test-fix-r$ROUND" || return 1
        commit_if_dirty "test round $ROUND" || return 1
        ROUND=$((ROUND + 1))
        record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
        ;;
      *) return "$rc" ;;
    esac
  done
  printf 'deep-review: test exceeded %s fix rounds\n' "$DR_MAX_ROUNDS" >&2
  return 1
}

run_document_step() {
  step_start document || return 1
  ROUND=$(jq -r '.round' "$RUN_FILE")
  call_agent document assess summary "document-r$ROUND" || return 1
  commit_if_dirty "documentation round $ROUND" || return 1
  step_complete document lint
}

run_lint_step() {
  step_start lint || return 1
  ROUND=$(jq -r '.round' "$RUN_FILE")
  if [ -z "$DR_CONFIG_FORMAT" ] && [ -z "$DR_CONFIG_LINT" ]; then
    step_complete lint
    return
  fi
  while [ "$ROUND" -le "$DR_MAX_ROUNDS" ]; do
    if [ -n "$DR_CONFIG_FORMAT" ]; then
      run_configured_command format "$DR_CONFIG_FORMAT" || return 1
      if [ "$COMMAND_EXIT" -ne 0 ]; then
        write_command_finding format "$COMMAND_EXIT" "$COMMAND_OUTPUT" || return 1
        call_agent lint fix summary "lint-fix-r$ROUND" || return 1
        commit_if_dirty "format round $ROUND" || return 1
        ROUND=$((ROUND + 1))
        record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
        continue
      fi
      commit_if_dirty "format round $ROUND" || return 1
    fi
    if [ -z "$DR_CONFIG_LINT" ]; then
      step_complete lint
      return
    fi
    run_configured_command lint "$DR_CONFIG_LINT" || return 1
    if [ "$COMMAND_EXIT" -eq 0 ]; then
      step_complete lint
      return
    fi
    write_command_finding lint "$COMMAND_EXIT" "$COMMAND_OUTPUT" || return 1
    call_agent lint fix summary "lint-fix-r$ROUND" || return 1
    commit_if_dirty "lint round $ROUND" || return 1
    ROUND=$((ROUND + 1))
    record_update_args --argjson round "$ROUND" -- '.round=$round' || return 1
  done
  printf 'deep-review: lint exceeded %s fix rounds\n' "$DR_MAX_ROUNDS" >&2
  return 1
}

write_delivery_record() {
  local approved_sha risk summary title tmp
  approved_sha=$(current_head) || return 1
  risk=$(jq -r '.risk_level' "$RUN_FILE")
  summary=$(jq -r '.summary' "$RUN_FILE")
  title=$TITLE
  [ -n "$title" ] \
    || title=$(git log --reverse --format=%s "$DEFAULT_BRANCH"..HEAD | head -1)
  [ -n "$title" ] || title=$(git log -1 --format=%s)
  mx_delivery_title_valid "$title" 2>/dev/null || {
    printf 'deep-review: generated delivery title is invalid\n' >&2
    return 1
  }
  tmp=$(mktemp "$STATE/.ready-to-push.tmp.XXXXXX") || return 1
  {
    printf 'version=1\n'
    printf 'task=%s\n' "$ID"
    printf 'worktree=%s\n' "$REPO_ROOT"
    printf 'branch=%s\n' "$BRANCH"
    printf 'approved_sha=%s\n' "$approved_sha"
    printf 'base=%s\n' "$DEFAULT_BRANCH"
    printf 'gate_run=%s\n' "$GATE_DIR"
    printf 'approval=pending\n'
    printf 'title=%s\n' "$title"
  } > "$tmp"
  chmod 600 "$tmp" || {
    rm -f "$tmp"
    return 1
  }
  mv -f "$tmp" "$STATE/$ID.ready-to-push" || return 1
  record_update_args --arg head "$approved_sha" --arg risk "$risk" \
    --arg summary "$summary" -- \
    '.status="passed" | .approved_head=$head | .pending_decision_key=null |
     .decision_ready=false | .risk_level=$risk | .summary=$summary' || return 1
}

respond_command() {
  local id=${1:-} key= answer=
  shift || true
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --decision) key=${2:-}; shift 2 ;;
      --answer) answer=${2:-}; shift 2 ;;
      -h|--help) usage; exit 0 ;;
      *) usage; exit 2 ;;
    esac
  done
  mx_pr_task_id_valid "$id" || fail "invalid task id"
  [ -n "$key" ] && [ -n "$answer" ] || fail "respond requires --decision and --answer"
  ID=$id
  GATE_DIR="$STATE/$ID.gate"
  RUN_FILE="$GATE_DIR/run.json"
  [ -f "$RUN_FILE" ] || fail "no deep-review run for $ID"
  task_ownership_valid "$ID" || fail "only the initiating actor may respond for $ID"
  [ "$(jq -r '.status' "$RUN_FILE")" = parked ] || fail "run is not parked"
  [ "$(jq -r '.pending_decision_key // empty' "$RUN_FILE")" = "$key" ] \
    || fail "decision key does not match the parked run"
  jq -n --arg key "$key" --arg answer "$answer" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{key:$key,answer:$answer,recorded_at:$at}' \
    | dr_atomic_write "$GATE_DIR/decisions/$key.json" 600 || fail "could not store decision"
  record_update_args --arg key "$key" -- \
    '.status="running" | .decision_ready=true |
     .last_decision_key=$key' || fail "could not resume run"
  report_status resolved "deep-review decision recorded" "$key" \
    || fail "decision stored but validated resolved report failed"
  printf 'deep-review: decision recorded; rerun the gate to continue\n'
}

if [ "${1:-}" = respond ]; then
  shift
  respond_command "$@"
  exit 0
fi

ID=${1:-}
[ -n "$ID" ] || {
  usage
  exit 2
}
shift
mx_pr_task_id_valid "$ID" || fail "invalid task id"

INTENT=
INTENT_FILE_ARG=
DEFAULT_BRANCH_ARG=
TITLE=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --intent) INTENT=${2:-}; shift 2 ;;
    --intent-file) INTENT_FILE_ARG=${2:-}; shift 2 ;;
    --base) DEFAULT_BRANCH_ARG=${2:-}; shift 2 ;;
    --title) TITLE=${2:-}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done
[ -z "$INTENT" ] || [ -z "$INTENT_FILE_ARG" ] \
  || fail "choose --intent or --intent-file, not both"

command -v jq >/dev/null 2>&1 || fail "jq is required"
REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || fail "run inside the task git worktree"
REPO_ROOT=$(cd "$REPO_ROOT" && pwd -P)
cd "$REPO_ROOT" || exit 1
BRANCH=$(git symbolic-ref --quiet --short HEAD 2>/dev/null) || fail "task branch is detached"
[ "$BRANCH" = "mx/$ID" ] || fail "expected task branch mx/$ID, found $BRANCH"
[ -d "$STATE" ] || fail "state directory is unavailable: $STATE"
task_ownership_valid "$ID" || fail "only the initiating actor may run deep-review for $ID"

GATE_DIR="$STATE/$ID.gate"
RUN_FILE="$GATE_DIR/run.json"
if [ ! -e "$RUN_FILE" ] && [ -z "$INTENT" ] && [ -z "$INTENT_FILE_ARG" ]; then
  fail "explicit intent required"
fi
if [ -n "$INTENT_FILE_ARG" ]; then
  [ -f "$INTENT_FILE_ARG" ] && [ ! -L "$INTENT_FILE_ARG" ] \
    || fail "intent file must be a regular non-symlink file"
  INTENT=$(cat "$INTENT_FILE_ARG")
fi

DEFAULT_BRANCH=${DEFAULT_BRANCH_ARG:-}
[ -n "$DEFAULT_BRANCH" ] || DEFAULT_BRANCH=$(default_branch) \
  || fail "cannot determine default branch"
mx_delivery_ref_valid "$DEFAULT_BRANCH" 2>/dev/null || fail "invalid default branch"

if [ ! -e "$RUN_FILE" ]; then
  [ -z "$(git status --porcelain)" ] || fail "worktree must be clean before validation"
  mkdir -p "$GATE_DIR"/findings "$GATE_DIR"/sessions "$GATE_DIR"/cmd-output \
    "$GATE_DIR"/prompts "$GATE_DIR"/schemas "$GATE_DIR"/decisions || exit 1
  chmod 700 "$GATE_DIR" "$GATE_DIR"/findings "$GATE_DIR"/sessions \
    "$GATE_DIR"/cmd-output "$GATE_DIR"/prompts "$GATE_DIR"/schemas \
    "$GATE_DIR"/decisions
  printf '%s' "$INTENT" | dr_sanitize_intent \
    | dr_atomic_write "$GATE_DIR/intent.txt" 600 || fail "could not store intent"
  printf '{}\n' | dr_atomic_write "$GATE_DIR/sessions.json" 600 \
    || fail "could not initialize sessions"
  HEAD_SHA=$(current_head) || fail "cannot read HEAD"
  BASE_SHA=$(git rev-parse "$DEFAULT_BRANCH" 2>/dev/null) || fail "cannot resolve base branch"
  jq -n --arg task "$ID" --arg worktree "$REPO_ROOT" --arg branch "$BRANCH" \
    --arg default_branch "$DEFAULT_BRANCH" --arg base_head "$BASE_SHA" \
    --arg approved_head "$HEAD_SHA" \
    '{version:1,task:$task,worktree:$worktree,branch:$branch,
      default_branch:$default_branch,base_head:$base_head,
      approved_head:$approved_head,status:"running",step:"intent",round:1,
      steps:{intent:"pending",rebase:"pending",review:"pending",test:"pending",document:"pending",lint:"pending"},
      history:[],pending_decision_key:null,decision_ready:false,
      summary:"Validation has not completed.",risk_level:"high",
      risk_rationale:"Validation has not completed."}' \
    | dr_atomic_write "$RUN_FILE" 600 || fail "could not initialize run record"
else
  [ -f "$RUN_FILE" ] && [ ! -L "$RUN_FILE" ] || fail "unsafe run record"
  jq -e '
    .version == 1 and
    (.status == "running" or .status == "parked" or .status == "passed" or .status == "failed") and
    (.step == "intent" or .step == "rebase" or .step == "review" or .step == "test" or .step == "document" or .step == "lint")
  ' "$RUN_FILE" >/dev/null 2>&1 || fail "invalid or unknown step in run record"
  [ "$(jq -r '.task' "$RUN_FILE")" = "$ID" ] || fail "run task binding changed"
  [ "$(jq -r '.worktree' "$RUN_FILE")" = "$REPO_ROOT" ] || fail "run worktree binding changed"
  DEFAULT_BRANCH=$(jq -r '.default_branch' "$RUN_FILE")
  RECORDED_HEAD=$(jq -r '.approved_head' "$RUN_FILE")
  HEAD_SHA=$(current_head)
  if [ "$RECORDED_HEAD" != "$HEAD_SHA" ]; then
    rm -f "$STATE/$ID.ready-to-push"
    record_update_args --arg head "$HEAD_SHA" -- \
      '.approved_head=$head | .status="running" |
       .steps[.step]="pending" | .pending_decision_key=null |
       .decision_ready=false' || fail "could not invalidate stale step"
    printf 'deep-review: HEAD changed; restarting current step against %s\n' "$HEAD_SHA"
  fi
  if [ "$(jq -r '.status' "$RUN_FILE")" = passed ]; then
    printf 'deep-review: already passed at %s\n' "$HEAD_SHA"
    exit 0
  fi
  if [ "$(jq -r '.status' "$RUN_FILE")" = parked ]; then
    fail "run is parked; record a matching decision with the respond subcommand"
  fi
fi

DR_DEFAULT_BRANCH=$DEFAULT_BRANCH
DR_REPO_ROOT=$REPO_ROOT
DR_GATE_DIR=$GATE_DIR
DR_INTENT_FILE=$GATE_DIR/intent.txt
export DR_DEFAULT_BRANCH DR_REPO_ROOT DR_GATE_DIR DR_INTENT_FILE DR_MX_ROOT

gate_exit_trap() {
  local rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ] && [ "$rc" -ne 10 ] && [ "$rc" -lt 128 ] \
    && [ -f "$RUN_FILE" ] && [ ! -L "$RUN_FILE" ]; then
    record_update \
      '.status="failed" | .steps[.step]="failed"' >/dev/null 2>&1 || true
  fi
  exit "$rc"
}
trap gate_exit_trap EXIT

dr_load_config "$DEFAULT_BRANCH" "$DR_CONFIG_FILE" \
  || fail "could not load trusted deep-review config"

set -e
while :; do
  STEP=$(jq -r '.step' "$RUN_FILE")
  ROUND=$(jq -r '.round' "$RUN_FILE")
  case "$STEP" in
    intent) run_intent_step ;;
    rebase) run_rebase_step ;;
    review) run_review_step ;;
    test) run_test_step ;;
    document) run_document_step ;;
    lint)
      run_lint_step
      [ -z "$(git status --porcelain)" ] || fail "validation ended with a dirty worktree"
      write_delivery_record || fail "could not write delivery handoff"
      report_status done "validated local branch at $(current_head)" \
        || fail "validation passed but completion report failed"
      printf 'deep-review: passed at %s; delivery approval is pending\n' "$(current_head)"
      exit 0
      ;;
    *) fail "unknown step '$STEP'" ;;
  esac
done
