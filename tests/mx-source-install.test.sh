#!/usr/bin/env bash
# Source-install entrypoint help and missing-tool diagnostics.
set -euo pipefail

# shellcheck source=tests/lib.sh disable=SC1091
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

install_help=$("$ROOT/install.sh" --help)
assert_contains "$install_help" "Build and install Multplx from this checkout" \
  "source installer help should describe the local install path"
assert_contains "$install_help" "--upgrade" \
  "source installer help should describe explicit upgrades"
assert_contains "$install_help" "--data-dir PATH" \
  "source installer help should list custom installation paths"

bootstrap_help=$("$ROOT/install-from-github.sh" --help)
assert_contains "$bootstrap_help" "raw.githubusercontent.com/KashyapTan/Multplx/main/install-from-github.sh" \
  "bootstrap help should show the public source command"
assert_contains "$bootstrap_help" "--upgrade" \
  "bootstrap help should describe forwarded installer options"

tmp=$(mx_test_tmproot mx-source-install)
repository="$tmp/bootstrap-source"
mkdir -p "$repository"
cat >"$repository/install.sh" <<'INSTALL'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${MX_BOOTSTRAP_TEST_ARGS:?}"
INSTALL
chmod +x "$repository/install.sh"
git -C "$repository" init -q
git -C "$repository" add install.sh
git -C "$repository" -c user.name='Multplx Tests' \
  -c user.email='tests@example.invalid' commit -qm fixture
git -C "$repository" branch -M main
TMPDIR="$tmp" MULTPLX_REPOSITORY="$repository" \
  MX_BOOTSTRAP_TEST_ARGS="$tmp/forwarded.args" \
  "$ROOT/install-from-github.sh" --home "$tmp/home with spaces" --upgrade
printf '%s\n' --home "$tmp/home with spaces" --upgrade >"$tmp/expected.args"
cmp -s "$tmp/expected.args" "$tmp/forwarded.args" \
  || fail "GitHub bootstrap did not forward installer options unchanged"
for leftover in "$tmp"/multplx-bootstrap.*; do
  [ ! -e "$leftover" ] || fail "GitHub bootstrap left its cloned source directory behind"
done

no_cargo_path="$tmp/no-cargo-path"
mkdir -p "$no_cargo_path"
ln -s "$(command -v bash)" "$no_cargo_path/bash"
ln -s "$(command -v dirname)" "$no_cargo_path/dirname"
if PATH="$no_cargo_path" "$ROOT/install.sh" --bin-dir "$tmp/bin" \
    --config-dir "$tmp/config" --data-dir "$tmp/data" >"$tmp/no-cargo.out" 2>&1; then
  status=0
else
  status=$?
fi
expect_code 2 "$status" "source install without Cargo"
assert_contains "$(cat "$tmp/no-cargo.out")" "Cargo is required" \
  "missing Cargo should produce an actionable error instead of installing tools"
[ ! -e "$tmp/bin" ] && [ ! -e "$tmp/data" ] \
  || fail "missing-Cargo refusal created installation paths"

pass "source install help and missing-Cargo refusal"
