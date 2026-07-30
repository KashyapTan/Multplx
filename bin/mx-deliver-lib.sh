#!/usr/bin/env bash
# Shared owner of the least-privilege delivery record and verification contract.
#
# A delivery record is an ordinary mode-0600, single-link file:
#
#   version=1
#   task=<task id>
#   worktree=<absolute task worktree>
#   branch=mx/<task id>
#   approved_sha=<40- or 64-character lowercase object id>
#   base=<safe base branch>
#   gate_run=<absolute state/<task id>.gate directory>
#   approval=pending|approved
#   title=<single-line PR title>
#
# The record is data, never shell source. Every key occurs exactly once and
# unknown keys are rejected. The referenced gate run must contain a private
# run.json with status=passed, approved_head equal to approved_sha, summary,
# risk_level, and risk_rationale. bin/mx-deliver.sh deterministically renders
# the PR body from those gate-owned fields.
#
# This library also owns the non-agent launch check used by delivery and remote
# merge commands. There is deliberately no environment escape hatch: a process
# carrying any known agent or gate marker cannot cross this boundary.

MX_DELIVERY_RECORD_VERSION=1

mx_delivery_reset_record() {
  MX_DELIVERY_VERSION=
  MX_DELIVERY_TASK=
  MX_DELIVERY_WORKTREE=
  MX_DELIVERY_BRANCH=
  MX_DELIVERY_APPROVED_SHA=
  MX_DELIVERY_BASE=
  MX_DELIVERY_GATE_RUN=
  MX_DELIVERY_APPROVAL=
  MX_DELIVERY_TITLE=
  MX_DELIVERY_RECORD_IDENTITY=
  MX_DELIVERY_RECORD_HASH=
  MX_DELIVERY_SUMMARY=
  MX_DELIVERY_RISK_LEVEL=
  MX_DELIVERY_RISK_RATIONALE=
  MX_DELIVERY_BODY=
  MX_DELIVERY_STALE_REASON=
}

mx_delivery_agent_ambience() {
  [ "${CLAUDECODE+x}" != x ] || return 0
  [ "${CODEX_THREAD_ID+x}" != x ] || return 0
  [ "${PI_CODING_AGENT+x}" != x ] || return 0
  [ "${NO_MISTAKES_GATE+x}" != x ] || return 0
  [ "${DEEP_REVIEW_GATE+x}" != x ] || return 0
  return 1
}

mx_delivery_refuse_agent_ambience() {
  mx_delivery_agent_ambience || return 0
  echo "error: credentialed delivery must run outside every broker, actor, daemon, and gate session" >&2
  return 3
}

mx_delivery_ref_valid() {
  local ref=${1-}
  [ -n "$ref" ] || return 1
  [ "${#ref}" -le 200 ] || return 1
  case "$ref" in
    -*|*'..'*|*'@{'*|*' '*|*'~'*|*'^'*|*':'*|*'?'*|*'['*|*'\'*|*'//'|*/|/*|*.)
      return 1
      ;;
    *[!A-Za-z0-9._/-]*) return 1 ;;
  esac
  return 0
}

mx_delivery_title_valid() {
  local title=${1-}
  [ -n "$title" ] || return 1
  [ "${#title}" -le 200 ] || return 1
  case "$title" in
    *$'\r'*|*$'\n'*|*$'\t'*) return 1 ;;
  esac
  return 0
}

mx_delivery_record_parse() {
  local record=$1 expected_task=$2 state=$3
  local state_device line key value seen
  mx_delivery_reset_record
  state_device=$(mx_pr_file_device "$state") || return 1
  mx_pr_private_file_valid "$record" 600 "$state_device" || return 1
  MX_DELIVERY_RECORD_IDENTITY=$(mx_pr_file_identity "$record") || return 1
  MX_DELIVERY_RECORD_HASH=$(mx_pr_sha256 "$record") || return 1
  seen='|'

  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      *=*) ;;
      *) return 1 ;;
    esac
    key=${line%%=*}
    value=${line#*=}
    case "$seen" in
      *"|$key|"*) return 1 ;;
    esac
    seen="${seen}${key}|"
    case "$key" in
      version) MX_DELIVERY_VERSION=$value ;;
      task) MX_DELIVERY_TASK=$value ;;
      worktree) MX_DELIVERY_WORKTREE=$value ;;
      branch) MX_DELIVERY_BRANCH=$value ;;
      approved_sha) MX_DELIVERY_APPROVED_SHA=$value ;;
      base) MX_DELIVERY_BASE=$value ;;
      gate_run) MX_DELIVERY_GATE_RUN=$value ;;
      approval) MX_DELIVERY_APPROVAL=$value ;;
      title) MX_DELIVERY_TITLE=$value ;;
      *) return 1 ;;
    esac
  done < "$record"

  [ "$MX_DELIVERY_VERSION" = "$MX_DELIVERY_RECORD_VERSION" ] || return 1
  [ "$MX_DELIVERY_TASK" = "$expected_task" ] || return 1
  mx_pr_task_id_valid "$MX_DELIVERY_TASK" || return 1
  case "$MX_DELIVERY_WORKTREE" in /*) ;; *) return 1 ;; esac
  [ "$MX_DELIVERY_BRANCH" = "mx/$MX_DELIVERY_TASK" ] || return 1
  mx_pr_head_valid "$MX_DELIVERY_APPROVED_SHA" || return 1
  mx_delivery_ref_valid "$MX_DELIVERY_BASE" || return 1
  [ "$MX_DELIVERY_GATE_RUN" = "$state/$MX_DELIVERY_TASK.gate" ] || return 1
  case "$MX_DELIVERY_APPROVAL" in pending|approved) ;; *) return 1 ;; esac
  mx_delivery_title_valid "$MX_DELIVERY_TITLE" || return 1
}

mx_delivery_record_unchanged() {
  local record=$1
  [ "$(mx_pr_file_identity "$record" 2>/dev/null || true)" = "$MX_DELIVERY_RECORD_IDENTITY" ] \
    && [ "$(mx_pr_sha256 "$record" 2>/dev/null || true)" = "$MX_DELIVERY_RECORD_HASH" ]
}

mx_delivery_meta_matches() {
  local state=$1 meta state_device recorded_worktree
  meta="$state/$MX_DELIVERY_TASK.meta"
  state_device=$(mx_pr_file_device "$state") || return 1
  mx_pr_private_file_valid "$meta" 600 "$state_device" || return 1
  recorded_worktree=$(sed -n 's/^worktree=//p' "$meta")
  [ "$recorded_worktree" = "$MX_DELIVERY_WORKTREE" ]
}

mx_delivery_gate_load() {
  local state=$1 run state_device json
  [ -d "$MX_DELIVERY_GATE_RUN" ] && [ ! -L "$MX_DELIVERY_GATE_RUN" ] || return 1
  run="$MX_DELIVERY_GATE_RUN/run.json"
  state_device=$(mx_pr_file_device "$state") || return 1
  mx_pr_private_file_valid "$run" 600 "$state_device" || return 1
  command -v jq >/dev/null 2>&1 || return 1
  json=$(jq -ce --arg sha "$MX_DELIVERY_APPROVED_SHA" '
    select(
      .status == "passed" and
      .approved_head == $sha and
      (.summary | type == "string" and length > 0 and length <= 20000) and
      (.risk_level == "low" or .risk_level == "medium" or .risk_level == "high") and
      (.risk_rationale | type == "string" and length > 0 and length <= 4000)
    ) |
    [.summary, .risk_level, .risk_rationale]
  ' "$run" 2>/dev/null) || return 1
  MX_DELIVERY_SUMMARY=$(printf '%s' "$json" | jq -r '.[0]') || return 1
  MX_DELIVERY_RISK_LEVEL=$(printf '%s' "$json" | jq -r '.[1]') || return 1
  MX_DELIVERY_RISK_RATIONALE=$(printf '%s' "$json" | jq -r '.[2]') || return 1
  MX_DELIVERY_BODY=$(printf '## Summary\n\n%s\n\n## Risk\n\n%s - %s\n' \
    "$MX_DELIVERY_SUMMARY" "$MX_DELIVERY_RISK_LEVEL" "$MX_DELIVERY_RISK_RATIONALE")
}

# Return codes:
#   0 eligible
#   2 valid but pending approval
#   3 stale validation/worktree binding; caller archives it as stale
#   1 malformed or unsafe record; caller leaves it in place for inspection
mx_delivery_eligible() {
  local state=$1 top branch head dirty
  MX_DELIVERY_STALE_REASON=
  if [ "$MX_DELIVERY_APPROVAL" != approved ]; then
    return 2
  fi
  if ! mx_delivery_meta_matches "$state"; then
    MX_DELIVERY_STALE_REASON="task metadata no longer binds the recorded worktree"
    return 3
  fi
  if [ ! -d "$MX_DELIVERY_WORKTREE" ]; then
    MX_DELIVERY_STALE_REASON="recorded worktree is missing"
    return 3
  fi
  top=$(git -C "$MX_DELIVERY_WORKTREE" rev-parse --show-toplevel 2>/dev/null) || {
    MX_DELIVERY_STALE_REASON="recorded worktree is not an inspectable git worktree"
    return 3
  }
  [ "$top" = "$MX_DELIVERY_WORKTREE" ] || {
    MX_DELIVERY_STALE_REASON="recorded worktree is not its git top level"
    return 3
  }
  branch=$(git -C "$MX_DELIVERY_WORKTREE" symbolic-ref --quiet --short HEAD 2>/dev/null) || {
    MX_DELIVERY_STALE_REASON="recorded worktree is detached"
    return 3
  }
  [ "$branch" = "$MX_DELIVERY_BRANCH" ] || {
    MX_DELIVERY_STALE_REASON="worktree branch moved from the approved branch"
    return 3
  }
  head=$(git -C "$MX_DELIVERY_WORKTREE" rev-parse --verify HEAD 2>/dev/null) || {
    MX_DELIVERY_STALE_REASON="worktree HEAD is unavailable"
    return 3
  }
  [ "$head" = "$MX_DELIVERY_APPROVED_SHA" ] || {
    MX_DELIVERY_STALE_REASON="worktree HEAD moved past the approved SHA"
    return 3
  }
  dirty=$(git -C "$MX_DELIVERY_WORKTREE" status --porcelain 2>/dev/null) || {
    MX_DELIVERY_STALE_REASON="worktree cleanliness could not be verified"
    return 3
  }
  [ -z "$dirty" ] || {
    MX_DELIVERY_STALE_REASON="worktree changed after validation"
    return 3
  }
  git -C "$MX_DELIVERY_WORKTREE" remote get-url origin >/dev/null 2>&1 || {
    MX_DELIVERY_STALE_REASON="worktree has no origin remote"
    return 3
  }
  if ! mx_delivery_gate_load "$state"; then
    MX_DELIVERY_STALE_REASON="gate run no longer proves this approved SHA"
    return 3
  fi
  return 0
}

mx_delivery_mark_stale() {
  local record=$1 destination
  destination="$record.stale"
  mx_delivery_record_unchanged "$record" || return 1
  mx_pr_regular_destination_or_absent "$destination" || return 1
  [ ! -e "$destination" ] || return 1
  mv -- "$record" "$destination"
}
