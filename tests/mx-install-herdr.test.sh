#!/usr/bin/env bash
# Contract tests for the pinned Herdr / Treehouse CI installers and the
# bounded Herdr lab cleanup helper. These tests do not download release assets
# and never start or stop the maintainer's default Herdr session.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

HERDR_INSTALL="$ROOT/bin/mx-install-herdr.sh"
TREEHOUSE_INSTALL="$ROOT/bin/mx-install-treehouse.sh"
HERDR_RUST="$ROOT/crates/multplx-backend/src/herdr_tools.rs"
TREEHOUSE_RUST="$ROOT/crates/multplx-backend/src/treehouse_tools.rs"
CLEANUP="$ROOT/bin/mx-herdr-ci-cleanup.sh"
CI="$ROOT/.github/workflows/ci.yml"

assert_present "$HERDR_INSTALL" "bin/mx-install-herdr.sh is missing"
[ ! -e "$TREEHOUSE_INSTALL" ] || fail "retired Treehouse installer adapter exists"
assert_present "$HERDR_RUST" "Rust Herdr installer is missing"
[ ! -e "$TREEHOUSE_RUST" ] || fail "retired Treehouse installer exists"
assert_present "$CLEANUP" "bin/mx-herdr-ci-cleanup.sh is missing"
[ -x "$HERDR_INSTALL" ] || fail "mx-install-herdr.sh must be executable"
[ -x "$CLEANUP" ] || fail "mx-herdr-ci-cleanup.sh must be executable"

test_herdr_installer_pins_exact_version_and_checksums() {
  assert_grep 'CI_VERSION: &str = "0.7.4"' "$HERDR_RUST" \
    "Herdr installer must pin suite-verified 0.7.4"
  assert_grep 'CI_MIN_PROTOCOL: u64 = 16' "$HERDR_RUST" \
    "Herdr installer must require protocol floor 16"
  assert_grep 'ogulcancelik/herdr' "$HERDR_RUST" \
    "Herdr installer must use the official GitHub release source"
  assert_grep 'herdr-linux-x86_64' "$HERDR_RUST" \
    "Herdr installer must name the Linux x86_64 release asset"
  assert_grep 'bc0fc02d4ba500f9cac2353a43e67fe036785ecca6eb55378e050fac3c103059' "$HERDR_RUST" \
    "Herdr installer must pin the Linux x86_64 SHA-256"
  assert_grep 'Sha256' "$HERDR_RUST" \
    "Rust Herdr installer must verify a SHA-256 checksum"
  assert_grep 'CI_MAX_BYTES: u64 = 25_000_000' "$HERDR_RUST" \
    "Herdr installer must bound the download size"
  assert_no_grep 'brew install' "$HERDR_RUST" \
    "Herdr installer must not use a floating package-manager install"
  assert_no_grep 'apt-get install' "$HERDR_RUST" \
    "Herdr installer must not use a floating package-manager install"
  pass "Herdr installer pins exact version, asset, checksum, and protocol floor"
}

test_retired_worktree_installer_cannot_download() {
  local dir fakebin rc
  mx_test_tmproot_into dir mx-retired-installer
  fakebin=$(mx_fakebin "$dir")
  cat > "$fakebin/curl" <<SH
#!/bin/sh
touch '$dir/download-invoked'
exit 99
SH
  chmod +x "$fakebin/curl"
  PATH="$fakebin:$PATH" "$MX_RUST_BIN" install-treehouse "$dir/install" > "$dir/out" 2>&1 && fail "retired installer command succeeded"
  [ ! -e "$dir/download-invoked" ] || fail "retired command downloaded an asset"
  assert_grep 'unrecognized subcommand' "$dir/out" "retired command still dispatched"
  pass "Treehouse installer is retired and cannot download assets"
}

test_cleanup_only_targets_job_owned_lab_sessions() {
  assert_grep 'mx-lab-' "$HERDR_RUST" \
    "cleanup must only consider mx-lab-* session names"
  assert_grep 'entry.get("default")' "$HERDR_RUST" \
    "cleanup must refuse default sessions"
  assert_grep 'snapshot' "$HERDR_RUST" \
    "cleanup must support a pre-suite snapshot"
  assert_grep 'teardown' "$HERDR_RUST" \
    "cleanup must support post-suite teardown of the delta"
  # Must not call ambient server stop.
  assert_no_grep 'server stop' "$HERDR_RUST" \
    "cleanup must never call ambient herdr server stop"
  pass "cleanup is bounded to job-owned mx-lab-* sessions"
}

test_host_adapters_are_exec_only() {
  local adapter lines
  for adapter in "$HERDR_INSTALL" "$CLEANUP"; do
    lines=$(wc -l < "$adapter" | tr -d ' ')
    [ "$lines" -le 10 ] || fail "$(basename "$adapter") is not a minimal host adapter"
    assert_grep 'exec "$BINARY"' "$adapter" \
      "$(basename "$adapter") must replace itself with the Rust binary"
    assert_no_grep 'curl|sha256|jq|mktemp|mkdir|rm |mv |cp |chmod|case |while |for ' "$adapter" \
      "$(basename "$adapter") contains installer or cleanup policy"
  done
  pass "Herdr host adapters contain transport only"
}

test_ci_wires_installers_and_required_lane() {
  assert_grep 'tests-herdr:' "$CI" "CI must define the required Herdr Behavior job"
  assert_grep 'target/release/mx install-herdr' "$CI" "CI must call the Rust Herdr installer"
  assert_no_grep 'install-treehouse' "$CI" "CI must not install the retired provider"
  assert_grep 'target/release/mx herdr-ci-cleanup snapshot' "$CI" "CI must snapshot sessions through Rust before the suite"
  assert_grep 'target/release/mx herdr-ci-cleanup teardown' "$CI" "CI must teardown job-owned sessions through Rust after"
  assert_grep "fail-on-gate-skip 'herdr not found'" "$CI" \
    "CI Herdr lane must fail on herdr-not-found"
  assert_grep 'family real-herdr-gated' "$CI" \
    "CI Herdr lane must run only the real-herdr-gated family"
  assert_grep 'lane portable-parallel-1' "$CI" \
    "portable CI must run parallel shard 1"
  assert_grep 'lane portable-parallel-2' "$CI" \
    "portable CI must run parallel shard 2"
  assert_grep 'lane portable-serial' "$CI" \
    "portable CI must run the serial remainder"
  assert_grep 'mx test-run --check-coverage' "$CI" \
    "CI must prove portable lanes and Herdr partition the complete inventory"
  # Live harness credential tests must stay out of the default Herdr lane.
  assert_no_grep 'live-harness-optin' "$CI" \
    "CI must not run live-harness-optin in the required Herdr lane"
  assert_no_grep 'MX_AFK_PI_HERDR_E2E' "$CI" \
    "CI must not enable live Pi/Herdr credential tests"
  assert_no_grep 'MX_SEND_MARKER_HERDR_E2E' "$CI" \
    "CI must not enable live marker Herdr credential tests"
  pass "CI wires pinned installers into a required serial Herdr lane"
}

test_herdr_installer_pins_exact_version_and_checksums
test_retired_worktree_installer_cannot_download
test_cleanup_only_targets_job_owned_lab_sessions
test_host_adapters_are_exec_only
test_ci_wires_installers_and_required_lane
