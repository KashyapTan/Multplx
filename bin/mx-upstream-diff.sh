#!/usr/bin/env bash
# Fetch, classify, and report upstream changes without modifying the Multplx tree.
#
# Usage:
#   mx-upstream-diff.sh --out <dir> [--since <sha>]
#   mx-upstream-diff.sh --record-reviewed <sha-or-head-sha-file>
#   mx-upstream-diff.sh --status
#
# A review run writes only below --out, including its private .upstream clone.
# --record-reviewed is the sole supported writer of docs/upstream.md's
# last_reviewed field and validates forward ancestry against fetched upstream.
# MX_UPSTREAM_RECORD_FILE overrides the record path for fixture tests.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RECORD_FILE="${MX_UPSTREAM_RECORD_FILE:-$ROOT_DIR/docs/upstream.md}"
RETIRED_EXIT=3
export GIT_NO_REPLACE_OBJECTS=1
export GIT_TERMINAL_PROMPT=0

usage() {
  sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
}

die() {
  printf 'mx-upstream-diff: %s\n' "$*" >&2
  exit 1
}

record_value() { # <key>
  local key=$1
  awk -v key="$key" '
    NR == 1 && $0 == "---" { in_header=1; next }
    in_header && $0 == "---" { exit }
    in_header && index($0, key ":") == 1 {
      value=substr($0, length(key) + 2)
      sub(/^[[:space:]]+/, "", value)
      sub(/[[:space:]]+$/, "", value)
      print value
      found=1
      exit
    }
    END { if (!found) exit 1 }
  ' "$RECORD_FILE"
}

validate_record() {
  [ -f "$RECORD_FILE" ] && [ ! -L "$RECORD_FILE" ] \
    || die "record is missing or unsafe: $RECORD_FILE"

  UPSTREAM_REPO=$(record_value upstream_repo) \
    || die "record is missing upstream_repo"
  FORK_POINT=$(record_value fork_point) \
    || die "record is missing fork_point"
  LAST_REVIEWED=$(record_value last_reviewed) \
    || die "record is missing last_reviewed"
  SYNC_STATUS=$(record_value status) \
    || die "record is missing status"
  RETIRED_REASON=$(record_value retired_reason) \
    || die "record is missing retired_reason"

  case "$SYNC_STATUS" in
    active) ;;
    retired)
      [ -n "$RETIRED_REASON" ] \
        || die "retired record requires retired_reason"
      ;;
    *) die "record status must be active or retired" ;;
  esac
  printf '%s\n' "$FORK_POINT" | grep -Eq '^[0-9a-f]{40}$' \
    || die "record contains an invalid fork_point"
  printf '%s\n' "$LAST_REVIEWED" | grep -Eq '^[0-9a-f]{40}$' \
    || die "record contains an invalid last_reviewed"
}

print_status() {
  printf 'upstream_repo=%s\n' "$UPSTREAM_REPO"
  printf 'fork_point=%s\n' "$FORK_POINT"
  printf 'last_reviewed=%s\n' "$LAST_REVIEWED"
  printf 'status=%s\n' "$SYNC_STATUS"
  printf 'retired_reason=%s\n' "$RETIRED_REASON"
}

refuse_if_retired() {
  if [ "$SYNC_STATUS" = retired ]; then
    printf 'upstream sync retired: %s\n' "$RETIRED_REASON" >&2
    exit "$RETIRED_EXIT"
  fi
}

canonical_parent_and_name() { # <path>
  local requested=$1 parent name
  case "$requested" in
    */*) parent=${requested%/*}; name=${requested##*/} ;;
    *) parent=.; name=$requested ;;
  esac
  [ -n "$name" ] && [ "$name" != . ] && [ "$name" != .. ] \
    || die "unsafe output path: $requested"
  mkdir -p "$parent" || die "cannot create output parent: $parent"
  parent=$(cd "$parent" && pwd -P) \
    || die "cannot resolve output parent: $parent"
  printf '%s/%s\n' "$parent" "$name"
}

prepare_output() { # <path>
  local resolved
  resolved=$(canonical_parent_and_name "$1")
  if [ -e "$resolved" ] && [ -L "$resolved" ]; then
    die "output directory must not be a symlink: $resolved"
  fi
  mkdir -p "$resolved" || die "cannot create output directory: $resolved"
  [ -d "$resolved" ] || die "output is not a directory: $resolved"
  OUTPUT_DIR=$(cd "$resolved" && pwd -P)
  CLONE_DIR="$OUTPUT_DIR/.upstream"
  REPORT_FILE="$OUTPUT_DIR/report-input.md"
  HEAD_FILE="$OUTPUT_DIR/head-sha"
}

configure_fetch_only_remote() { # <clone>
  local clone=$1
  git -C "$clone" remote set-url --push upstream /dev/null \
    || die "cannot disable the upstream push URL"
}

prepare_clone() { # <clone>
  local clone=$1 current_url
  if [ -e "$clone" ] && [ -L "$clone" ]; then
    die "scratch clone must not be a symlink: $clone"
  fi
  if [ -d "$clone/.git" ]; then
    current_url=$(git -C "$clone" remote get-url upstream 2>/dev/null || true)
    [ "$current_url" = "$UPSTREAM_REPO" ] \
      || die "scratch clone remote does not match upstream_repo"
  elif [ -e "$clone" ]; then
    die "scratch clone path exists but is not a git clone: $clone"
  else
    git clone --quiet --no-checkout --origin upstream \
      "$UPSTREAM_REPO" "$clone" \
      || die "cannot clone upstream repository"
  fi
  configure_fetch_only_remote "$clone"
  git -C "$clone" fetch --quiet --no-tags upstream \
    '+refs/heads/*:refs/remotes/upstream/*' \
    || die "cannot fetch upstream repository"
}

upstream_head() { # <clone>
  local clone=$1 symbolic candidate
  symbolic=$(git -C "$clone" symbolic-ref -q refs/remotes/upstream/HEAD 2>/dev/null || true)
  if [ -n "$symbolic" ]; then
    git -C "$clone" rev-parse "${symbolic}^{commit}"
    return
  fi
  for candidate in refs/remotes/upstream/main refs/remotes/upstream/master; do
    if git -C "$clone" show-ref --verify --quiet "$candidate"; then
      git -C "$clone" rev-parse "${candidate}^{commit}"
      return
    fi
  done
  die "cannot resolve upstream default branch"
}

assert_commit() { # <clone> <sha> <label>
  git -C "$1" cat-file -e "$2^{commit}" 2>/dev/null \
    || die "$3 is not a fetched upstream commit: $2"
}

assert_ancestor() { # <clone> <older> <newer> <message>
  git -C "$1" merge-base --is-ancestor "$2" "$3" 2>/dev/null \
    || die "$4"
}

load_relevance_map() { # <destination>
  local destination=$1
  awk '
    $0 == "<!-- mx-upstream-map:start -->" { in_map=1; next }
    $0 == "<!-- mx-upstream-map:end -->" { in_map=0; done=1; next }
    in_map && $0 ~ /^\|/ {
      count=split($0, cells, "|")
      if (count < 4) next
      pattern=cells[2]
      class=cells[3]
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", pattern)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", class)
      gsub(/^`|`$/, "", pattern)
      if (pattern == "Upstream path glob" || pattern ~ /^-+$/) next
      if (class !~ /^(relevant|irrelevant|deleted|flag)$/) {
        printf "invalid relevance class for %s: %s\n", pattern, class >"/dev/stderr"
        exit 2
      }
      if (pattern == "" || pattern ~ /[[:space:]]/) {
        printf "invalid relevance path glob: %s\n", pattern >"/dev/stderr"
        exit 2
      }
      printf "%s\t%s\n", pattern, class
      rows++
    }
    END {
      if (!done || !rows) {
        print "relevance map markers or rows are missing" >"/dev/stderr"
        exit 2
      }
    }
  ' "$RECORD_FILE" >"$destination" \
    || die "cannot parse relevance map"
}

classify_path() { # <path> <map>
  local path=$1 map=$2 pattern class
  while IFS=$'\t' read -r pattern class; do
    [ -n "$pattern" ] || continue
    if [[ "$path" == $pattern ]]; then
      printf '%s\n' "$class"
      return
    fi
  done <"$map"
  printf 'flag\n'
}

append_commit_report() { # <clone> <commit> <destination>
  local clone=$1 commit=$2 destination=$3
  {
    printf '#### Change metadata and diff\n\n'
    printf '```diff\n'
    git -C "$clone" --no-pager show --no-ext-diff --no-textconv --no-renames \
      --format=fuller --stat --patch "$commit"
    printf '```\n\n'
  } >>"$destination"
}

report_run() { # <out> <since-override>
  local requested_out=$1 since_override=$2 map commits_file relevant_file
  local flagged_file skipped_file needs_file commit path class category
  local relevant_count=0 flagged_count=0 skipped_count=0 commit_count=0
  local needs_count head from short subject paths

  prepare_output "$requested_out"
  prepare_clone "$CLONE_DIR"
  head=$(upstream_head "$CLONE_DIR")
  from=${since_override:-$LAST_REVIEWED}
  [ -n "$from" ] || from=$FORK_POINT
  assert_commit "$CLONE_DIR" "$FORK_POINT" fork_point
  assert_commit "$CLONE_DIR" "$from" since
  assert_ancestor "$CLONE_DIR" "$FORK_POINT" "$from" \
    "since commit is older than or unrelated to the fork point"
  assert_ancestor "$CLONE_DIR" "$from" "$head" \
    "since commit is not an ancestor of upstream HEAD"

  map="$OUTPUT_DIR/.relevance-map"
  commits_file="$OUTPUT_DIR/.commits"
  relevant_file="$OUTPUT_DIR/.relevant"
  flagged_file="$OUTPUT_DIR/.flagged"
  skipped_file="$OUTPUT_DIR/.skipped"
  needs_file="$OUTPUT_DIR/.needs-mapping"
  load_relevance_map "$map"
  git -C "$CLONE_DIR" rev-list --reverse "$from..$head" >"$commits_file"
  : >"$relevant_file"
  : >"$flagged_file"
  : >"$skipped_file"
  : >"$needs_file"

  while IFS= read -r commit; do
    [ -n "$commit" ] || continue
    commit_count=$((commit_count + 1))
    category=skipped
    paths=
    while IFS= read -r path; do
      [ -n "$path" ] || continue
      class=$(classify_path "$path" "$map")
      paths="${paths}${paths:+, }$path ($class)"
      case "$class" in
        relevant) category=relevant ;;
        flag)
          printf '%s\n' "$path" >>"$needs_file"
          [ "$category" = relevant ] || category=flagged
          ;;
      esac
    done < <(git -C "$CLONE_DIR" diff-tree --root --no-commit-id \
      --name-only -r "$commit")
    short=$(git -C "$CLONE_DIR" rev-parse --short=12 "$commit")
    subject=$(git -C "$CLONE_DIR" log -1 --format=%s "$commit")
    case "$category" in
      relevant)
        relevant_count=$((relevant_count + 1))
        {
          printf '### `%s` %s\n\n' "$short" "$subject"
          printf -- '- Paths: %s\n\n' "$paths"
        } >>"$relevant_file"
        append_commit_report "$CLONE_DIR" "$commit" "$relevant_file"
        ;;
      flagged)
        flagged_count=$((flagged_count + 1))
        {
          printf '### `%s` %s\n\n' "$short" "$subject"
          printf -- '- Paths: %s\n\n' "$paths"
        } >>"$flagged_file"
        append_commit_report "$CLONE_DIR" "$commit" "$flagged_file"
        ;;
      skipped)
        skipped_count=$((skipped_count + 1))
        printf -- '- `%s` %s - %s\n' "$short" "$subject" "$paths" \
          >>"$skipped_file"
        ;;
    esac
  done <"$commits_file"

  if [ -s "$needs_file" ]; then
    LC_ALL=C sort -u "$needs_file" >"$needs_file.sorted"
    mv "$needs_file.sorted" "$needs_file"
    needs_count=$(wc -l <"$needs_file" | tr -d ' ')
  else
    needs_count=0
  fi

  {
    printf '# Upstream review input\n\n'
    printf -- '- Upstream repository: %s\n' "$UPSTREAM_REPO"
    printf -- '- Diff range: `%s..%s`\n' "$from" "$head"
    printf -- '- Upstream HEAD: `%s`\n' "$head"
    printf -- '- Commits: %s\n' "$commit_count"
    printf -- '- Relevant commits: %s\n' "$relevant_count"
    printf -- '- Flagged commits: %s\n' "$flagged_count"
    printf -- '- Mechanically skipped commits: %s\n' "$skipped_count"
    printf -- '- Paths needing mapping: %s\n\n' "$needs_count"
    printf '## Relevant changes\n\n'
    if [ -s "$relevant_file" ]; then
      cat "$relevant_file"
    else
      printf 'None.\n\n'
    fi
    printf '## Flagged changes\n\n'
    if [ -s "$flagged_file" ]; then
      cat "$flagged_file"
    else
      printf 'None.\n\n'
    fi
    printf '## Paths needing mapping\n\n'
    if [ -s "$needs_file" ]; then
      while IFS= read -r path; do
        printf -- '- `%s`\n' "$path"
      done <"$needs_file"
      printf '\n'
    else
      printf 'None.\n\n'
    fi
    printf '## Mechanically skipped\n\n'
    if [ -s "$skipped_file" ]; then
      cat "$skipped_file"
    else
      printf 'None.\n'
    fi
  } >"$REPORT_FILE"
  printf '%s\n' "$head" >"$HEAD_FILE"
  rm -f "$map" "$commits_file" "$relevant_file" "$flagged_file" \
    "$skipped_file" "$needs_file"
  printf 'report=%s\nhead=%s\nrange=%s..%s\n' \
    "$REPORT_FILE" "$head" "$from" "$head"
}

resolve_record_target() { # <sha-or-file>
  local requested=$1 line_count
  if [ -f "$requested" ] && [ ! -L "$requested" ]; then
    line_count=$(wc -l <"$requested" | tr -d ' ')
    [ "$line_count" -eq 1 ] \
      || die "reviewed SHA file must contain exactly one line"
    RECORD_TARGET=$(sed -n '1p' "$requested")
    RECORD_EVIDENCE_DIR=$(cd "$(dirname "$requested")" && pwd -P)
  else
    RECORD_TARGET=$requested
    RECORD_EVIDENCE_DIR=
  fi
  printf '%s\n' "$RECORD_TARGET" | grep -Eq '^[0-9a-f]{40}$' \
    || die "invalid reviewed commit id: $RECORD_TARGET"
}

record_clone() {
  local candidate temporary
  candidate=
  if [ -n "$RECORD_EVIDENCE_DIR" ] &&
      [ -d "$RECORD_EVIDENCE_DIR/.upstream/.git" ] &&
      [ ! -L "$RECORD_EVIDENCE_DIR/.upstream" ]; then
    candidate="$RECORD_EVIDENCE_DIR/.upstream"
  fi
  if [ -n "$candidate" ]; then
    RECORD_CLONE=$candidate
    RECORD_TEMP=
    prepare_clone "$RECORD_CLONE"
    return
  fi
  temporary=$(mktemp -d "${TMPDIR:-/tmp}/mx-upstream-record.XXXXXX") \
    || die "cannot create record-validation scratch directory"
  RECORD_TEMP=$temporary
  RECORD_CLONE="$temporary/.upstream"
  prepare_clone "$RECORD_CLONE"
}

write_last_reviewed() { # <sha> <review-date>
  local target=$1 review_date=$2 record_dir temporary
  record_dir=$(cd "$(dirname "$RECORD_FILE")" && pwd -P)
  temporary=$(mktemp "$record_dir/.upstream-record.XXXXXX") \
    || die "cannot create record update"
  if ! awk -v target="$target" -v review_date="$review_date" '
    BEGIN { changed=0; log_start=0; log_end=0; in_log=0 }
    /^last_reviewed:[[:space:]]*/ {
      print "last_reviewed: " target
      changed++
      next
    }
    $0 == "<!-- mx-upstream-log:start -->" {
      log_start++
      in_log=1
      print
      next
    }
    $0 == "<!-- mx-upstream-log:end -->" {
      log_end++
      print "- " review_date ": reviewed through `" target "` via the upstream-sync workflow."
      print
      in_log=0
      next
    }
    in_log && $0 == "_No completed upstream review has been recorded._" { next }
    { print }
    END {
      if (changed != 1 || log_start != 1 || log_end != 1 || in_log) exit 2
    }
  ' "$RECORD_FILE" >"$temporary"; then
    rm -f "$temporary"
    die "record must contain one last_reviewed field and one completed-review log"
  fi
  chmod "$(stat -f '%Lp' "$RECORD_FILE" 2>/dev/null ||
    stat -c '%a' "$RECORD_FILE" 2>/dev/null || printf '644')" "$temporary" \
    2>/dev/null || true
  mv "$temporary" "$RECORD_FILE" \
    || { rm -f "$temporary"; die "cannot update record"; }
}

record_lock_release() {
  local pid
  [ -n "${RECORD_LOCK:-}" ] || return 0
  [ -d "$RECORD_LOCK" ] && [ ! -L "$RECORD_LOCK" ] || return 0
  pid=$(cat "$RECORD_LOCK/pid" 2>/dev/null || true)
  [ "$pid" = "${BASHPID:-$$}" ] || return 0
  rm -f "$RECORD_LOCK/pid" 2>/dev/null || true
  rmdir "$RECORD_LOCK" 2>/dev/null || true
}

path_age_seconds() { # <path>
  local modified
  if [ "$(uname)" = Darwin ]; then
    modified=$(stat -f %m "$1" 2>/dev/null) || { printf '999999\n'; return; }
  else
    modified=$(stat -c %Y "$1" 2>/dev/null) || { printf '999999\n'; return; }
  fi
  printf '%s\n' "$(( $(date +%s) - modified ))"
}

record_lock_acquire() {
  local record_dir attempt pid stale
  record_dir=$(cd "$(dirname "$RECORD_FILE")" && pwd -P)
  RECORD_LOCK="$record_dir/.upstream-record.lock"
  attempt=0
  while [ "$attempt" -lt 3 ]; do
    if mkdir "$RECORD_LOCK" 2>/dev/null; then
      printf '%s\n' "${BASHPID:-$$}" >"$RECORD_LOCK/pid" \
        || { rmdir "$RECORD_LOCK" 2>/dev/null || true; die "cannot claim record lock"; }
      trap record_lock_release EXIT
      return
    fi
    [ -d "$RECORD_LOCK" ] && [ ! -L "$RECORD_LOCK" ] \
      || die "record lock path is unsafe"
    pid=$(cat "$RECORD_LOCK/pid" 2>/dev/null || true)
    case "$pid" in
      ''|*[!0-9]*)
        [ "$(path_age_seconds "$RECORD_LOCK")" -ge 2 ] \
          || die "record update lock is still being claimed"
        ;;
      *)
        if kill -0 "$pid" 2>/dev/null; then
          die "record update is already running as pid $pid"
        fi
        ;;
    esac
    stale="$record_dir/.upstream-record.lock.stale.${BASHPID:-$$}"
    if mv "$RECORD_LOCK" "$stale" 2>/dev/null; then
      rm -f "$stale/pid" 2>/dev/null || true
      rmdir "$stale" 2>/dev/null \
        || die "stale record lock contains unexpected files"
    fi
    attempt=$((attempt + 1))
  done
  die "cannot acquire record update lock"
}

record_reviewed() { # <sha-or-file>
  local requested=$1 head review_date
  resolve_record_target "$requested"
  record_lock_acquire
  validate_record
  refuse_if_retired
  record_clone
  head=$(upstream_head "$RECORD_CLONE")
  assert_commit "$RECORD_CLONE" "$FORK_POINT" fork_point
  assert_commit "$RECORD_CLONE" "$LAST_REVIEWED" last_reviewed
  assert_commit "$RECORD_CLONE" "$RECORD_TARGET" reviewed
  assert_ancestor "$RECORD_CLONE" "$FORK_POINT" "$LAST_REVIEWED" \
    "last_reviewed is older than or unrelated to the fork point"
  assert_ancestor "$RECORD_CLONE" "$LAST_REVIEWED" "$RECORD_TARGET" \
    "refusing to move last_reviewed backwards or outside the reviewed range"
  assert_ancestor "$RECORD_CLONE" "$RECORD_TARGET" "$head" \
    "reviewed commit is not reachable from upstream HEAD"
  if [ "$RECORD_TARGET" != "$LAST_REVIEWED" ]; then
    review_date=${MX_UPSTREAM_REVIEW_DATE:-$(date -u +%Y-%m-%d)}
    printf '%s\n' "$review_date" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' \
      || die "invalid review date: $review_date"
    write_last_reviewed "$RECORD_TARGET" "$review_date"
  fi
  [ -z "$RECORD_TEMP" ] || rm -rf "$RECORD_TEMP"
  printf 'last_reviewed=%s\n' "$RECORD_TARGET"
  if [ "$RECORD_TARGET" = "$LAST_REVIEWED" ]; then
    printf 'unchanged=true\n'
  fi
  record_lock_release
  trap - EXIT
}

main() {
  local mode= out= since= record=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --out)
        [ "$#" -ge 2 ] || { usage >&2; exit 2; }
        [ -z "$mode" ] || { usage >&2; exit 2; }
        mode=report
        out=$2
        shift 2
        ;;
      --since)
        [ "$#" -ge 2 ] || { usage >&2; exit 2; }
        since=$2
        shift 2
        ;;
      --record-reviewed)
        [ "$#" -ge 2 ] || { usage >&2; exit 2; }
        [ -z "$mode" ] || { usage >&2; exit 2; }
        mode=record
        record=$2
        shift 2
        ;;
      --status)
        [ -z "$mode" ] || { usage >&2; exit 2; }
        mode=status
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        usage >&2
        exit 2
        ;;
    esac
  done
  [ -n "$mode" ] || { usage >&2; exit 2; }
  [ "$mode" = report ] || [ -z "$since" ] || { usage >&2; exit 2; }

  validate_record
  if [ "$mode" = status ]; then
    print_status
    [ "$SYNC_STATUS" = active ] || exit "$RETIRED_EXIT"
    exit 0
  fi
  refuse_if_retired
  case "$mode" in
    report) report_run "$out" "$since" ;;
    record) record_reviewed "$record" ;;
  esac
}

main "$@"
