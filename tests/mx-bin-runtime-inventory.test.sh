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

git -C "$ROOT" ls-files --cached --others --exclude-standard -z bin \
  | while IFS= read -r -d '' path; do
      [ -f "$ROOT/$path" ] && [ -x "$ROOT/$path" ] && printf '%s\n' "$path"
    done \
  | LC_ALL=C sort -u > "$ACTUAL"
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
      grep -Eq '^((export )?MX_MULTICALL_EXPLICIT=1 )?exec .*\$?(BINARY|MX_BINARY|rust_bin|mx_[a-z_]+_bin)' "$ROOT/$path" \
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

ABI_FIXTURE="$TMP/abi-fixture"
mkdir -p "$ABI_FIXTURE/bin"
for file in mx-rust-runtime.sh mx-session-start.sh mx-spawn.sh mx-update.sh mx-bootstrap.sh mx-teardown.sh; do
  cp "$ROOT/bin/$file" "$ABI_FIXTURE/bin/$file"
done
cat >"$ABI_FIXTURE/stale-mx" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${MX_ABI_CALL_LOG:?}"
exit 2
SH
chmod +x "$ABI_FIXTURE/stale-mx"
: >"$ABI_FIXTURE/calls"
for adapter in mx-session-start.sh mx-spawn.sh mx-update.sh mx-bootstrap.sh mx-teardown.sh; do
  output=$(MX_RUST_BIN="$ABI_FIXTURE/stale-mx" MX_ABI_CALL_LOG="$ABI_FIXTURE/calls" \
    "$ABI_FIXTURE/bin/$adapter" 2>&1) && inventory_fail "$adapter accepted an incompatible runtime"
  assert_contains "$output" "incompatible with the current wrappers" \
    "$adapter did not diagnose its stale runtime"
done
[ "$(wc -l <"$ABI_FIXTURE/calls" | tr -d ' ')" -eq 5 ] \
  || inventory_fail "ABI refusal recursively or repeatedly invoked the stale runtime"
[ "$(sort -u "$ABI_FIXTURE/calls")" = runtime-abi ] \
  || inventory_fail "ABI refusal dispatched a wrapper command before compatibility was proven"
[ "$FAILED" -eq 0 ] || exit 1
pass "cutover wrappers reject stale runtimes once without recursive dispatch"
