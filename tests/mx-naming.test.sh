#!/usr/bin/env bash
# Permanent tripwire for pre-Multplx vocabulary and identifiers.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mx-naming.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT

is_allowlisted_path() {
  case "$1" in
    firstmate|firstmate/*|plans/*|UPDATE_PLAN.md|firstmate_dependencies.md|\
    firstmate-architecture.html|docs/upstream.md|tests/mx-naming.test.sh)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

legacy_path_pattern='(^|/)(fm[-_]|[^/]*(captain|crewmate|secondmate|fleet|ahoy|bearings))'
legacy_content_pattern='captain|crewmate|second[[:space:]_-]*mate|first[[:space:]_-]*mate|fleet|(^|[^[:alnum:]_])crew([^[:alnum:]_]|$)|(^|[^[:alnum:]_])ship([^[:alnum:]_]|$)|fm-|fm_|FM_|ahoy|bearings|2ndmate'
failures="$TMP_ROOT/failures"
: > "$failures"

while IFS= read -r file; do
  is_allowlisted_path "$file" && continue

  if printf '%s\n' "$file" | grep -Eiq "$legacy_path_pattern"; then
    printf '%s\n' "legacy path: $file" >> "$failures"
  fi

  [ -f "$ROOT/$file" ] || continue
  if LC_ALL=C grep -Iq . "$ROOT/$file"; then
    case "$file" in
      bin/mx-doc-audience-check.sh|docs/documentation-audiences.json)
        sed -e 's#firstmate/##g' "$ROOT/$file"
        ;;
      *)
        cat "$ROOT/$file"
        ;;
    esac \
      | sed \
      -e 's#`firstmate/`##g' \
      -e 's#firstmate_dependencies\.md##g' \
      -e 's#firstmate-architecture\.html##g' \
      -e 's#TASKS_AXI_MAINTAINER_KIND=captain#TASKS_AXI_MAINTAINER_KIND=external#g' \
      -e 's#if \. == "captain" then "maintainer"#if . == "external" then "maintainer"#g' \
      | grep -Ein "$legacy_content_pattern" \
      | sed "s#^#$file:#" >> "$failures" || true
  fi
done < <(git -C "$ROOT" ls-files)

if [ -s "$failures" ]; then
  cat "$failures" >&2
  fail "legacy naming remains outside the historical allowlist"
fi

assert_grep 'TASKS_AXI_MAINTAINER_KIND=captain' "$ROOT/bin/mx-decision-hold.sh" \
  "tasks-axi compatibility must preserve its legacy upstream role token at the CLI boundary"
assert_grep 'if . == "captain" then "maintainer" else . end;' "$ROOT/bin/mx-system-snapshot.sh" \
  "tasks-axi backlog reads must normalize the legacy upstream role token"

pass "maintained paths and content use Multplx vocabulary"
