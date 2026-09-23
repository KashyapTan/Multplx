# shellcheck shell=bash
# Shared "supervision missing" predicate.
# Usage: . bin/mx-supervision-lib.sh
#
# Reports whether a Multplx home needs supervision because it has in-flight
# work (a state/<id>.meta exists, except a valid completed ordinary task), and
# whether its watcher has a fresh liveness
# beacon (state/.last-watcher-beat, touched every poll cycle, within the grace
# window).
# bin/mx-guard.sh keeps its task-specific grace-based warning predicate;
# bin/mx-turnend-guard.sh uses the status fields here for its banner but performs
# its end-of-turn block decision with the live watcher lock check in
# bin/mx-wake-lib.sh.

# Portable mtime; Linux stat lacks -f, macOS stat lacks -c.
mx_sup_stat_mtime() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %m "$1" 2>/dev/null
  else
    stat -c %Y "$1" 2>/dev/null
  fi
}

# mx_supervision_status <state-dir> [grace-seconds]
# Populates, for the state dir at $1:
#   MX_SUP_IN_FLIGHT      count of active, legacy, persistent, or unknown task records
#   MX_SUP_NEEDED         true/false - in-flight work
#   MX_SUP_WATCHER_FRESH  true/false - a watcher beacon within the grace window
#   MX_SUP_BEACON_DESC    human-readable beacon age, for banners ("never" if absent)
#   MX_SUP_QUEUE_PENDING  true/false - state/.wake-queue has unread records
# grace-seconds defaults to $MX_GUARD_GRACE, then 300, matching mx-guard.sh.
# Always returns 0; callers read the vars, or use mx_supervision_unhealthy below.
mx_supervision_status() {
  local state=$1 grace=${2:-${MX_GUARD_GRACE:-300}} meta beat m age
  MX_SUP_IN_FLIGHT=0
  MX_SUP_NEEDED=false
  MX_SUP_WATCHER_FRESH=false
  MX_SUP_BEACON_DESC=never
  MX_SUP_QUEUE_PENDING=false

  for meta in "$state"/*.meta; do
    [ -e "$meta" ] || continue
    mx_sup_completed_ordinary "$meta" && continue
    MX_SUP_IN_FLIGHT=$((MX_SUP_IN_FLIGHT + 1))
  done
  if [ "$MX_SUP_IN_FLIGHT" -gt 0 ]; then
    MX_SUP_NEEDED=true
  fi

  beat="$state/.last-watcher-beat"
  if [ -e "$beat" ]; then
    m=$(mx_sup_stat_mtime "$beat")
    if [ -n "$m" ]; then
      age=$(( $(date +%s) - m ))
      MX_SUP_BEACON_DESC="${age}s ago"
      [ "$age" -lt "$grace" ] && MX_SUP_WATCHER_FRESH=true
    else
      # shellcheck disable=SC2034 # Read by callers (mx-guard.sh) after sourcing.
      MX_SUP_BEACON_DESC=unknown
    fi
  fi

  # shellcheck disable=SC2034 # Read by callers (mx-guard.sh) after sourcing.
  [ -s "$state/.wake-queue" ] && MX_SUP_QUEUE_PENDING=true
  return 0
}

# Exclude only a bounded, non-symlink canonical ordinary assignment whose
# completion state is explicit. Without jq or complete identity, keep the
# metadata in-flight conservatively.
mx_sup_completed_ordinary() {
  local meta=$1 id size schema_count model_count kind_count kind canonical
  command -v jq >/dev/null 2>&1 || return 1
  [ -f "$meta" ] && [ ! -L "$meta" ] || return 1
  size=$(wc -c < "$meta" 2>/dev/null) || return 1
  [ "$size" -le 4194304 ] 2>/dev/null || return 1
  awk -F= 'NF < 2 || $1 !~ /^[A-Za-z_][A-Za-z0-9_]*$/ || seen[$1]++ { exit 1 }' "$meta" \
    || return 1
  schema_count=$(grep -c '^schema_version=2$' "$meta" 2>/dev/null || true)
  model_count=$(grep -c '^canonical_model=' "$meta" 2>/dev/null || true)
  kind_count=$(grep -c '^kind=' "$meta" 2>/dev/null || true)
  [ "$schema_count" -eq 1 ] && [ "$model_count" -eq 1 ] && [ "$kind_count" -eq 1 ] || return 1
  kind=$(sed -n 's/^kind=//p' "$meta")
  canonical=$(sed -n 's/^canonical_model=//p' "$meta")
  [ -n "$canonical" ] || return 1
  id=${meta##*/}
  id=${id%.meta}
  printf '%s\n' "$canonical" | jq -e --arg id "$id" --arg kind "$kind" '
    type == "object" and
    .schema_version == 2 and .task_id == $id and
    (.role == "researcher" or .role == "implementer" or .role == "reviewer") and
    (.artifact == "report" or .artifact == "implementation") and
    (((.persistent == true or .private_home == true) and $kind == "daemon") or
     ((.persistent == false and .private_home == false) and
      ((.artifact == "report" and $kind == "scout") or (.artifact == "implementation" and $kind == "delivery")))) and
    .persistent == false and .private_home == false and .legacy_unknown == false and
    (.attempt.id | type == "string" and length > 0) and
    (.attempt.generation | type == "number" and . > 0) and
    (.attempt.brief_revision | type == "number" and . > 0) and
    .accepted_brief_revision == .attempt.brief_revision and
    .schedule.state == "completed"
  ' >/dev/null 2>&1
}

# mx_supervision_needed <state-dir> [grace-seconds]
# Exit 0 (true) exactly when in-flight work needs a watcher. Exit 1 (false)
# for an idle home.
mx_supervision_needed() {
  mx_supervision_status "$@"
  [ "$MX_SUP_NEEDED" = true ]
}

# mx_supervision_unhealthy <state-dir> [grace-seconds]
# Exit 0 (true) exactly in the dangerous state: in-flight work exists and no
# watcher has a fresh beacon. Exit 1 (false) otherwise, including zero in-flight.
mx_supervision_unhealthy() {
  mx_supervision_status "$@"
  [ "$MX_SUP_IN_FLIGHT" -gt 0 ] && [ "$MX_SUP_WATCHER_FRESH" = false ]
}
