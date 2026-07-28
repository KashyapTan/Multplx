# shellcheck shell=bash
# Shared "supervision missing" predicate.
# Usage: . bin/mx-supervision-lib.sh
#
# Reports whether a Multplx home needs supervision because it has in-flight
# work (a state/<id>.meta exists), and whether its watcher has a fresh liveness
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
#   MX_SUP_IN_FLIGHT      count of state/*.meta (in-flight tasks)
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
