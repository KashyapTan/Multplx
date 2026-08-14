#!/usr/bin/env bash
# Exhaustive executable-bin cutover gate.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

MANIFEST="$ROOT/tests/fixtures/bin-runtime-inventory.tsv"
TMP=$(mx_test_tmproot mx-bin-runtime-inventory)
ACTUAL="$TMP/actual"
DECLARED="$TMP/declared"
FAILED=0
inventory_fail() {
  printf 'not ok - %s\n' "$1" >&2
  FAILED=1
}

git -C "$ROOT" ls-files -s bin \
  | awk '$1 == "100755" { print $4 }' \
  | LC_ALL=C sort > "$ACTUAL"
awk -F '\t' '!/^#/ && NF { print $1 }' "$MANIFEST" | LC_ALL=C sort > "$DECLARED"

missing=$(comm -23 "$ACTUAL" "$DECLARED" || true)
extra=$(comm -13 "$ACTUAL" "$DECLARED" || true)
[ -z "$missing" ] || inventory_fail "bin runtime inventory is missing executable paths: $missing"
[ -z "$extra" ] || inventory_fail "bin runtime inventory names non-executable paths: $extra"
[ "$(wc -l < "$DECLARED" | tr -d ' ')" = "$(sort -u "$DECLARED" | wc -l | tr -d ' ')" ] \
  || inventory_fail "bin runtime inventory contains duplicate paths"

while IFS="$(printf '\t')" read -r path class reason extra_field; do
  case "$path" in ''|'#'*) continue ;; esac
  [ -z "${extra_field:-}" ] || inventory_fail "$path inventory row has extra fields"
  [ -n "${reason:-}" ] || inventory_fail "$path inventory row has no written reason"
  case "$class" in
    minimal-adapter)
      lines=$(wc -l < "$ROOT/$path" | tr -d ' ')
      [ "$lines" -le 12 ] || inventory_fail "$path minimal adapter grew to $lines lines"
      grep -Eq '^exec .*\$?(BINARY|MX_BINARY|rust_bin|mx_[a-z_]+_bin)' "$ROOT/$path" \
        || inventory_fail "$path minimal adapter does not end in an exec-only Rust handoff"
      if rg -n '^[[:space:]]*(mktemp|mkdir|rm|mv|cp|chmod|chown|jq|git|gh|tmux|herdr)([[:space:]]|$)' "$ROOT/$path" >/dev/null; then
        inventory_fail "$path minimal adapter contains policy, mutation, or orchestration"
      fi
      ;;
    sourced-shell-abi)
      lines=$(wc -l < "$ROOT/$path" | tr -d ' ')
      [ "$lines" -gt 12 ] || inventory_fail "$path sourced shell ABI is small enough to be a minimal adapter"
      ;;
    pending-native-cutover)
      inventory_fail "$path is still pending native cutover: $reason"
      ;;
    *) inventory_fail "$path has unknown runtime class '$class'" ;;
  esac
done < "$MANIFEST"

[ "$FAILED" -eq 0 ] || exit 1
pass "every tracked executable bin path is a minimal adapter or documented shell ABI"
