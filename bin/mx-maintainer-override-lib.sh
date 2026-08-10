#!/usr/bin/env bash
# Shared owner of exact, single-use maintainer exceptions.
#
# Records live under state/maintainer-overrides/{pending,granted,denied,
# consumed,stale}.  They are private JSON data, never shell source and never an
# audit-log substitute for control flow.  A grant binds one registered policy
# boundary, task, project, operation, target, expected-state digest, and expiry.
# The exceptional caller must consume it atomically before mutation.
#
# This file owns the schema, registry, validation, transitions, and locking.
# Subsystems own the exceptional action itself and call mx_override_consume with
# freshly observed binding values immediately before that action.

MX_OVERRIDE_SCHEMA_VERSION=1
MX_OVERRIDE_DEFAULT_TTL=${MX_OVERRIDE_DEFAULT_TTL:-3600}

mx_override_error() {
  printf 'mx-maintainer-override: %s\n' "$*" >&2
}

mx_override_sha256_text() {
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$1" | shasum -a 256 | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$1" | sha256sum | awk '{print $1}'
  else
    mx_override_error "shasum or sha256sum is required"
    return 1
  fi
}

mx_override_now() {
  date +%s
}

mx_override_slug_valid() {
  case "${1:-}" in
    ''|*[!A-Za-z0-9._-]*) return 1 ;;
  esac
}

mx_override_digest_valid() {
  case "${1:-}" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]* )
      [ "${#1}" -eq 64 ]
      ;;
    *) return 1 ;;
  esac
}

mx_override_one_line_valid() {
  [ -n "${1:-}" ] || return 1
  case "$1" in
    *$'\n'*|*$'\r'*) return 1 ;;
  esac
}

# Stable policy-boundary registry.
# Format: boundary<TAB>class<TAB>registered alternate owner.
# Capability and integrity entries intentionally have no consumable alternate;
# they are inventoried so callers can distinguish a factual blocker from a
# Multplx policy decision without pretending the fact changed.
mx_override_registry() {
  cat <<'EOF'
workflow.skip-stage	policy	bin/mx-workflow.sh
workflow.reorder-stage	policy	bin/mx-workflow.sh
validation.waive-gate	policy	bin/mx-deep-review.sh
delivery.merge-red	policy	bin/mx-pr-merge.sh
cleanup.discard-unlanded	policy	bin/mx-teardown.sh
project.direct-write	policy	bin/mx-override-run.sh
isolation.single-checkout	policy	bin/mx-spawn.sh
session.terminate-owner	policy	bin/mx-lock.sh
security.one-action-elevation	policy	bin/mx-override-run.sh
delivery.credentialed-action	policy	bin/mx-maintainer-override.sh handoff
dependency.install	policy	bin/mx-override-run.sh
authentication.login	policy	bin/mx-maintainer-override.sh handoff
integrity.validation-state	integrity	coded alternate required; facts remain unchanged
integrity.object-identity	integrity	coded alternate required; facts remain unchanged
integrity.session-lock	integrity	coded alternate required; facts remain unchanged
integrity.worktree-isolation	integrity	isolation.single-checkout
capability.tool-unavailable	capability	dependency.install or operator handoff
capability.authentication-required	capability	authentication.login or operator handoff
capability.credential-unavailable	capability	delivery.credentialed-action or operator handoff
capability.host-restriction	capability	operator handoff only
EOF
}

mx_override_boundary_lookup() {
  local wanted=$1 line boundary
  while IFS= read -r line; do
    boundary=${line%%$'\t'*}
    if [ "$boundary" = "$wanted" ]; then
      printf '%s\n' "$line"
      return 0
    fi
  done <<EOF
$(mx_override_registry)
EOF
  return 1
}

mx_override_boundary_class() {
  local line rest
  line=$(mx_override_boundary_lookup "$1") || return 1
  rest=${line#*$'\t'}
  printf '%s\n' "${rest%%$'\t'*}"
}

mx_override_boundary_alternate() {
  local line
  line=$(mx_override_boundary_lookup "$1") || return 1
  printf '%s\n' "${line##*$'\t'}"
}

mx_override_file_mode() {
  if [ "$(uname)" = Darwin ]; then
    stat -f '%Lp' "$1" 2>/dev/null
  else
    stat -c '%a' "$1" 2>/dev/null
  fi
}

mx_override_link_count() {
  if [ "$(uname)" = Darwin ]; then
    stat -f '%l' "$1" 2>/dev/null
  else
    stat -c '%h' "$1" 2>/dev/null
  fi
}

mx_override_state_root() {
  local state=${MX_STATE_OVERRIDE:-${MX_HOME:-${MX_ROOT_OVERRIDE:-.}}/state}
  printf '%s/maintainer-overrides\n' "$state"
}

mx_override_prepare_root() {
  local root dir
  root=$(mx_override_state_root) || return 1
  if [ -e "$root" ] || [ -L "$root" ]; then
    [ -d "$root" ] && [ ! -L "$root" ] || {
      mx_override_error "override root is not a real directory: $root"
      return 1
    }
  else
    mkdir -p "$root" || return 1
  fi
  chmod 700 "$root" || return 1
  for dir in pending granted denied consumed stale; do
    if [ -e "$root/$dir" ] || [ -L "$root/$dir" ]; then
      [ -d "$root/$dir" ] && [ ! -L "$root/$dir" ] || {
        mx_override_error "override state path is not a real directory: $root/$dir"
        return 1
      }
    else
      mkdir "$root/$dir" || return 1
    fi
    chmod 700 "$root/$dir" || return 1
  done
  printf '%s\n' "$root"
}

mx_override_record_path() {
  local root=$1 state=$2 request=$3
  mx_override_slug_valid "$request" || return 1
  case "$state" in pending|granted|denied|consumed|stale) ;; *) return 1 ;; esac
  printf '%s/%s/%s.json\n' "$root" "$state" "$request"
}

mx_override_record_secure() {
  local file=$1
  [ -f "$file" ] && [ ! -L "$file" ] || return 1
  [ "$(mx_override_file_mode "$file")" = 600 ] || return 1
  [ "$(mx_override_link_count "$file")" = 1 ] || return 1
}

mx_override_record_unique() {
  local root=$1 request=$2 expected=$3 state file count=0
  for state in pending granted denied consumed stale; do
    file=$(mx_override_record_path "$root" "$state" "$request") || return 1
    if [ -e "$file" ] || [ -L "$file" ]; then
      count=$((count + 1))
      [ "$file" = "$expected" ] || return 1
    fi
  done
  [ "$count" -eq 1 ]
}

mx_override_record_validate() {
  local file=$1 expected_state=${2:-}
  command -v jq >/dev/null 2>&1 || return 1
  mx_override_record_secure "$file" || return 1
  jq -e --argjson version "$MX_OVERRIDE_SCHEMA_VERSION" --arg expected "$expected_state" '
    type == "object" and
    (keys | sort) == ([
      "action_argv_or_operation", "action_digest", "alternate", "boundary_class",
      "boundary_id", "consequence", "consumed_at", "decided_at", "decision",
      "expected_state_digest", "expires_at", "maintainer_words_digest", "outcome",
      "outcome_digest", "project", "request_id", "requested_at", "schema_version",
      "target_identity", "task_id"
    ] | sort) and
    .schema_version == $version and
    (.request_id | type == "string" and test("^[A-Za-z0-9._-]+$")) and
    (.boundary_id | type == "string" and test("^[A-Za-z0-9._-]+$")) and
    (.boundary_class == "policy") and
    (.task_id | type == "string" and test("^[A-Za-z0-9._-]+$")) and
    (.project | type == "string" and test("^[A-Za-z0-9._-]+$")) and
    (.action_argv_or_operation | type == "string" and length > 0) and
    (.action_digest | type == "string" and test("^[0-9a-f]{64}$")) and
    (.target_identity | type == "string" and length > 0) and
    (.expected_state_digest | type == "string" and test("^[0-9a-f]{64}$")) and
    (.consequence | type == "string" and length > 0) and
    (.alternate | type == "string" and length > 0) and
    (.requested_at as $requested |
      ($requested | type) == "number" and ($requested | floor) == $requested and $requested > 0 and
      (.expires_at as $expires |
        ($expires | type) == "number" and ($expires | floor) == $expires and $expires > $requested)) and
    (.decision == "pending" or .decision == "granted" or .decision == "denied" or .decision == "consumed" or .decision == "stale") and
    (.decided_at == null or
      (.decided_at as $decided | ($decided | type) == "number" and ($decided | floor) == $decided and $decided > 0)) and
    (.maintainer_words_digest == null or (.maintainer_words_digest | type == "string" and test("^[0-9a-f]{64}$"))) and
    (.consumed_at == null or
      (.consumed_at as $consumed | ($consumed | type) == "number" and ($consumed | floor) == $consumed and $consumed > 0)) and
    (.outcome == "pending" or .outcome == "not-run" or .outcome == "succeeded" or .outcome == "failed" or .outcome == "denied" or .outcome == "expired" or .outcome == "state-changed") and
    (.outcome_digest == null or (.outcome_digest | type == "string" and test("^[0-9a-f]{64}$"))) and
    (if .decision == "pending" then
       .decided_at == null and .maintainer_words_digest == null and .consumed_at == null and
       .outcome == "pending" and .outcome_digest == null
     elif .decision == "granted" then
       .decided_at != null and .maintainer_words_digest != null and .consumed_at == null and
       .outcome == "pending" and .outcome_digest == null
     elif .decision == "denied" then
       .decided_at != null and .maintainer_words_digest != null and .consumed_at == null and
       .outcome == "denied" and .outcome_digest != null
     elif .decision == "consumed" then
       .decided_at != null and .maintainer_words_digest != null and .consumed_at != null and
       (.outcome == "not-run" or .outcome == "succeeded" or .outcome == "failed") and
       (if .outcome == "not-run" then .outcome_digest == null else .outcome_digest != null end)
     else
       .decided_at != null and (.outcome == "expired" or .outcome == "state-changed") and .outcome_digest != null
     end) and
    ($expected == "" or .decision == $expected)
  ' "$file" >/dev/null 2>&1 || return 1
  local operation recorded_digest boundary recorded_alternate
  operation=$(jq -r '.action_argv_or_operation' "$file") || return 1
  recorded_digest=$(jq -r '.action_digest' "$file") || return 1
  [ "$(mx_override_sha256_text "$operation")" = "$recorded_digest" ] || return 1
  boundary=$(jq -r '.boundary_id' "$file") || return 1
  [ "$(mx_override_boundary_class "$boundary" 2>/dev/null || true)" = policy ] || return 1
  recorded_alternate=$(jq -r '.alternate' "$file") || return 1
  [ "$(mx_override_boundary_alternate "$boundary" 2>/dev/null || true)" = "$recorded_alternate" ]
}

mx_override_lock_acquire() {
  local root=$1 lock="$1/.transition.lock" attempt=0 owner
  while ! mkdir "$lock" 2>/dev/null; do
    owner=$(cat "$lock/pid" 2>/dev/null || true)
    case "$owner" in
      ''|*[!0-9]*) ;;
      *)
        if ! kill -0 "$owner" 2>/dev/null; then
          rm -f "$lock/pid" 2>/dev/null || true
          rmdir "$lock" 2>/dev/null || true
          continue
        fi
        ;;
    esac
    attempt=$((attempt + 1))
    [ "$attempt" -lt 500 ] || {
      mx_override_error "could not acquire override transition lock"
      return 1
    }
    sleep 0.01
  done
  chmod 700 "$lock" 2>/dev/null || {
    rmdir "$lock" 2>/dev/null || true
    return 1
  }
  printf '%s\n' "$$" >"$lock/pid" || {
    rmdir "$lock" 2>/dev/null || true
    return 1
  }
  chmod 600 "$lock/pid" || {
    rm -f "$lock/pid"
    rmdir "$lock" 2>/dev/null || true
    return 1
  }
  MX_OVERRIDE_HELD_LOCK=$lock
}

mx_override_lock_release() {
  [ -n "${MX_OVERRIDE_HELD_LOCK:-}" ] || return 0
  rm -f "$MX_OVERRIDE_HELD_LOCK/pid" 2>/dev/null || true
  rmdir "$MX_OVERRIDE_HELD_LOCK" 2>/dev/null || true
  MX_OVERRIDE_HELD_LOCK=
}

mx_override_publish_json() {
  local destination=$1 json=$2 parent temporary
  parent=${destination%/*}
  [ ! -e "$destination" ] && [ ! -L "$destination" ] || return 1
  temporary=$(mktemp "$parent/.override-write.XXXXXX") || return 1
  chmod 600 "$temporary" || { rm -f "$temporary"; return 1; }
  if ! printf '%s\n' "$json" >"$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  mv "$temporary" "$destination"
}

mx_override_replace_json() {
  local source=$1 destination=$2 json=$3 parent temporary
  parent=${destination%/*}
  [ ! -e "$destination" ] && [ ! -L "$destination" ] || return 1
  temporary=$(mktemp "$parent/.override-write.XXXXXX") || return 1
  chmod 600 "$temporary" || { rm -f "$temporary"; return 1; }
  if ! printf '%s\n' "$json" >"$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  mv "$temporary" "$destination" || { rm -f "$temporary"; return 1; }
  rm -f "$source"
}

mx_override_generate_id() {
  local seed digest
  seed="$(date -u +%Y%m%dT%H%M%SZ):$$:${RANDOM:-0}:${1:-override}"
  digest=$(mx_override_sha256_text "$seed") || return 1
  printf 'mo-%s-%s\n' "$(date -u +%Y%m%d%H%M%S)" "${digest%"${digest#????????????}"}"
}

mx_override_request() {
  local boundary=$1 task=$2 project=$3 operation=$4 target=$5 state_digest=$6 consequence=$7 ttl=${8:-$MX_OVERRIDE_DEFAULT_TTL}
  local root request now expires action_digest alternate json destination
  [ "$(mx_override_boundary_class "$boundary" 2>/dev/null || true)" = policy ] || {
    mx_override_error "boundary is not a registered policy exception: $boundary"
    return 1
  }
  mx_override_slug_valid "$task" || { mx_override_error "invalid task id"; return 1; }
  mx_override_slug_valid "$project" || { mx_override_error "invalid project id"; return 1; }
  [ -n "$operation" ] || { mx_override_error "operation must not be empty"; return 1; }
  mx_override_one_line_valid "$target" || { mx_override_error "target identity must be one non-empty line"; return 1; }
  mx_override_digest_valid "$state_digest" || { mx_override_error "expected-state digest must be lowercase SHA-256"; return 1; }
  mx_override_one_line_valid "$consequence" || { mx_override_error "consequence must be one non-empty line"; return 1; }
  case "$ttl" in ''|*[!0-9]*|0) mx_override_error "ttl must be positive seconds"; return 1 ;; esac
  command -v jq >/dev/null 2>&1 || { mx_override_error "jq is required"; return 1; }
  root=$(mx_override_prepare_root) || return 1
  mx_override_lock_acquire "$root" || return 1
  request=$(mx_override_generate_id "$boundary:$task:$target") || { mx_override_lock_release; return 1; }
  now=$(mx_override_now)
  expires=$((now + ttl))
  action_digest=$(mx_override_sha256_text "$operation") || { mx_override_lock_release; return 1; }
  alternate=$(mx_override_boundary_alternate "$boundary") || { mx_override_lock_release; return 1; }
  json=$(jq -cn \
    --argjson schema_version "$MX_OVERRIDE_SCHEMA_VERSION" \
    --arg request_id "$request" --arg boundary_id "$boundary" --arg boundary_class policy \
    --arg task_id "$task" --arg project "$project" \
    --arg action_argv_or_operation "$operation" --arg action_digest "$action_digest" \
    --arg target_identity "$target" --arg expected_state_digest "$state_digest" \
    --arg consequence "$consequence" --arg alternate "$alternate" \
    --argjson requested_at "$now" --argjson expires_at "$expires" \
    '{schema_version:$schema_version,request_id:$request_id,boundary_id:$boundary_id,boundary_class:$boundary_class,task_id:$task_id,project:$project,action_argv_or_operation:$action_argv_or_operation,action_digest:$action_digest,target_identity:$target_identity,expected_state_digest:$expected_state_digest,consequence:$consequence,requested_at:$requested_at,expires_at:$expires_at,decision:"pending",decided_at:null,maintainer_words_digest:null,consumed_at:null,outcome:"pending",outcome_digest:null,alternate:$alternate}') \
    || { mx_override_lock_release; return 1; }
  destination=$(mx_override_record_path "$root" pending "$request") || { mx_override_lock_release; return 1; }
  mx_override_publish_json "$destination" "$json" || { mx_override_lock_release; return 1; }
  mx_override_record_validate "$destination" pending || {
    rm -f "$destination"
    mx_override_lock_release
    return 1
  }
  mx_override_lock_release
  printf '%s\n' "$request"
}

mx_override_require_primary_lock() {
  local state=${MX_STATE_OVERRIDE:-${MX_HOME:-${MX_ROOT_OVERRIDE:-.}}/state}
  local script_dir
  script_dir=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P) || return 1
  # shellcheck source=bin/mx-session-lock-lib.sh
  . "$script_dir/mx-session-lock-lib.sh"
  mx_session_lock_owned_by_self "$state" || {
    mx_override_error "grant and denial require the lock-owning primary session"
    return 1
  }
}

mx_override_decide() {
  local request=$1 decision=$2 words=$3 root source destination now words_digest json boundary target operation
  case "$decision" in granted|denied) ;; *) return 1 ;; esac
  [ -n "$words" ] || { mx_override_error "maintainer words must not be empty"; return 1; }
  mx_override_require_primary_lock || return 1
  root=$(mx_override_prepare_root) || return 1
  mx_override_lock_acquire "$root" || return 1
  source=$(mx_override_record_path "$root" pending "$request") || { mx_override_lock_release; return 1; }
  mx_override_record_unique "$root" "$request" "$source" || {
    mx_override_error "request identity is duplicated or misplaced: $request"
    mx_override_lock_release
    return 1
  }
  mx_override_record_validate "$source" pending || {
    mx_override_error "pending request is missing, unsafe, or invalid: $request"
    mx_override_lock_release
    return 1
  }
  now=$(mx_override_now)
  words_digest=$(mx_override_sha256_text "$words") || { mx_override_lock_release; return 1; }
  if [ "$(jq -r '.expires_at' "$source")" -le "$now" ]; then
    json=$(jq -c --argjson now "$now" --arg digest "$(mx_override_sha256_text expired)" \
      '.decision="stale" | .decided_at=$now | .outcome="expired" | .outcome_digest=$digest' "$source") \
      || { mx_override_lock_release; return 1; }
    destination=$(mx_override_record_path "$root" stale "$request") || { mx_override_lock_release; return 1; }
    mx_override_replace_json "$source" "$destination" "$json" || { mx_override_lock_release; return 1; }
    mx_override_lock_release
    mx_override_error "request expired before decision: $request"
    return 1
  fi
  if [ "$decision" = granted ]; then
    boundary=$(jq -r '.boundary_id' "$source") || { mx_override_lock_release; return 1; }
    target=$(jq -r '.target_identity' "$source") || { mx_override_lock_release; return 1; }
    operation=$(jq -r '.action_argv_or_operation' "$source") || { mx_override_lock_release; return 1; }
    case "$words" in *"$boundary"*) ;; *) mx_override_error "grant words must name the exact boundary $boundary"; mx_override_lock_release; return 1 ;; esac
    case "$words" in *"$target"*) ;; *) mx_override_error "grant words must name the exact target"; mx_override_lock_release; return 1 ;; esac
    case "$words" in *"$operation"*) ;; *) mx_override_error "grant words must name the exact operation"; mx_override_lock_release; return 1 ;; esac
    json=$(jq -c --argjson now "$now" --arg words "$words_digest" \
      '.decision="granted" | .decided_at=$now | .maintainer_words_digest=$words' "$source") \
      || { mx_override_lock_release; return 1; }
    destination=$(mx_override_record_path "$root" granted "$request") || { mx_override_lock_release; return 1; }
  else
    json=$(jq -c --argjson now "$now" --arg words "$words_digest" --arg outcome "$(mx_override_sha256_text denied)" \
      '.decision="denied" | .decided_at=$now | .maintainer_words_digest=$words | .outcome="denied" | .outcome_digest=$outcome' "$source") \
      || { mx_override_lock_release; return 1; }
    destination=$(mx_override_record_path "$root" denied "$request") || { mx_override_lock_release; return 1; }
  fi
  mx_override_replace_json "$source" "$destination" "$json" || { mx_override_lock_release; return 1; }
  mx_override_record_validate "$destination" "$decision" || {
    mx_override_lock_release
    mx_override_error "decision publication failed validation"
    return 1
  }
  mx_override_lock_release
}

mx_override_grant() {
  mx_override_decide "$1" granted "$2"
}

mx_override_deny() {
  mx_override_decide "$1" denied "$2"
}

mx_override_consume() {
  local request=$1 boundary=$2 task=$3 project=$4 operation=$5 target=$6 state_digest=$7
  local root source destination now action_digest mismatch='' json outcome_digest
  root=$(mx_override_prepare_root) || return 1
  mx_override_lock_acquire "$root" || return 1
  source=$(mx_override_record_path "$root" granted "$request") || { mx_override_lock_release; return 1; }
  mx_override_record_unique "$root" "$request" "$source" || {
    mx_override_error "grant identity is duplicated or misplaced: $request"
    mx_override_lock_release
    return 1
  }
  mx_override_record_validate "$source" granted || {
    mx_override_error "granted request is missing, unsafe, invalid, or already consumed: $request"
    mx_override_lock_release
    return 1
  }
  now=$(mx_override_now)
  action_digest=$(mx_override_sha256_text "$operation") || { mx_override_lock_release; return 1; }
  [ "$(jq -r '.expires_at' "$source")" -gt "$now" ] || mismatch=expired
  [ "$(jq -r '.boundary_id' "$source")" = "$boundary" ] || mismatch="state-changed"
  [ "$(jq -r '.task_id' "$source")" = "$task" ] || mismatch="state-changed"
  [ "$(jq -r '.project' "$source")" = "$project" ] || mismatch="state-changed"
  [ "$(jq -r '.action_digest' "$source")" = "$action_digest" ] || mismatch="state-changed"
  [ "$(jq -r '.target_identity' "$source")" = "$target" ] || mismatch="state-changed"
  [ "$(jq -r '.expected_state_digest' "$source")" = "$state_digest" ] || mismatch="state-changed"
  if [ -n "$mismatch" ]; then
    outcome_digest=$(mx_override_sha256_text "$mismatch") || { mx_override_lock_release; return 1; }
    json=$(jq -c --argjson now "$now" --arg outcome "$mismatch" --arg digest "$outcome_digest" \
      '.decision="stale" | .consumed_at=$now | .outcome=$outcome | .outcome_digest=$digest' "$source") \
      || { mx_override_lock_release; return 1; }
    destination=$(mx_override_record_path "$root" stale "$request") || { mx_override_lock_release; return 1; }
    mx_override_replace_json "$source" "$destination" "$json" || { mx_override_lock_release; return 1; }
    mx_override_lock_release
    mx_override_error "grant binding changed or expired; a new maintainer decision is required: $request"
    return 1
  fi
  json=$(jq -c --argjson now "$now" \
    '.decision="consumed" | .consumed_at=$now | .outcome="not-run"' "$source") \
    || { mx_override_lock_release; return 1; }
  destination=$(mx_override_record_path "$root" consumed "$request") || { mx_override_lock_release; return 1; }
  mx_override_replace_json "$source" "$destination" "$json" || { mx_override_lock_release; return 1; }
  mx_override_record_validate "$destination" consumed || {
    mx_override_lock_release
    mx_override_error "consumption publication failed validation"
    return 1
  }
  mx_override_lock_release
  printf '%s\n' "$destination"
}

mx_override_result() {
  local request=$1 outcome=$2 detail=$3 root file digest json temporary
  case "$outcome" in succeeded|failed) ;; *) mx_override_error "result must be succeeded or failed"; return 1 ;; esac
  [ -n "$detail" ] || { mx_override_error "result detail must not be empty"; return 1; }
  root=$(mx_override_prepare_root) || return 1
  mx_override_lock_acquire "$root" || return 1
  file=$(mx_override_record_path "$root" consumed "$request") || { mx_override_lock_release; return 1; }
  mx_override_record_unique "$root" "$request" "$file" || { mx_override_lock_release; return 1; }
  mx_override_record_validate "$file" consumed || { mx_override_lock_release; return 1; }
  [ "$(jq -r '.outcome' "$file")" = not-run ] || {
    mx_override_error "consumed request already records an outcome: $request"
    mx_override_lock_release
    return 1
  }
  digest=$(mx_override_sha256_text "$detail") || { mx_override_lock_release; return 1; }
  json=$(jq -c --arg outcome "$outcome" --arg digest "$digest" \
    '.outcome=$outcome | .outcome_digest=$digest' "$file") || { mx_override_lock_release; return 1; }
  temporary=$(mktemp "${file%/*}/.override-result.XXXXXX") || { mx_override_lock_release; return 1; }
  chmod 600 "$temporary" || { rm -f "$temporary"; mx_override_lock_release; return 1; }
  printf '%s\n' "$json" >"$temporary" || { rm -f "$temporary"; mx_override_lock_release; return 1; }
  mv "$temporary" "$file" || { rm -f "$temporary"; mx_override_lock_release; return 1; }
  mx_override_record_validate "$file" consumed || { mx_override_lock_release; return 1; }
  mx_override_lock_release
}

mx_override_find_record() {
  local request=$1 root state file
  root=$(mx_override_state_root)
  for state in pending granted denied consumed stale; do
    file=$(mx_override_record_path "$root" "$state" "$request") || return 1
    if [ -e "$file" ] || [ -L "$file" ]; then
      mx_override_record_unique "$root" "$request" "$file" || return 1
      mx_override_record_validate "$file" || return 1
      printf '%s\n' "$file"
      return 0
    fi
  done
  return 1
}
