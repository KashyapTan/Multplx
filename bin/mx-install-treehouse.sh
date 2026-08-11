#!/usr/bin/env bash
# mx-install-treehouse.sh - install CI's pinned, verified Treehouse build.
#
# Used only by the required real-Herdr CI lane for E2E scripts that genuinely
# need treehouse (spawn worktree acquisition). Same pin/checksum discipline as
# mx-install-herdr.sh: official release URL, exact asset, SHA-256, bounded
# download, post-install version check. Never a floating package-manager latest.
#
# Usage:
#   mx-install-treehouse.sh <destination-directory>
#
# Pins Treehouse v2.0.1, the version exercised by the local real-Herdr suite.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  _mx_treehouse_tool_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  # shellcheck source=bin/mx-rust-runtime.sh
  . "$_mx_treehouse_tool_dir/mx-rust-runtime.sh"
  _mx_treehouse_tools_implementation=$(mx_treehouse_tools_implementation) || exit $?
  if [ "$_mx_treehouse_tools_implementation" = rust ]; then
    _mx_treehouse_rust_bin=$(mx_rust_runtime_bin) || exit $?
    exec "$_mx_treehouse_rust_bin" install-treehouse "$@"
  fi
fi
set -eu

MX_TREEHOUSE_CI_VERSION=2.0.1
MX_TREEHOUSE_CI_TAG="v${MX_TREEHOUSE_CI_VERSION}"
# Bounded download ceiling (bytes). Official 2.0.1 archives are under 8 MiB.
MX_TREEHOUSE_CI_MAX_BYTES=15000000
MX_TREEHOUSE_CI_REPO=kunchenguid/treehouse

die() {
  printf 'mx-install-treehouse.sh: %s\n' "$*" >&2
  exit 1
}

DESTINATION=${1:?usage: mx-install-treehouse.sh <destination-directory>}

os=$(uname -s)
arch=$(uname -m)
case "${os}-${arch}" in
  Linux-x86_64)
    ARCHIVE=treehouse-v${MX_TREEHOUSE_CI_VERSION}-linux-amd64.tar.gz
    SHA256=1d5a32751ab921670103fd201ddb2b91b47338cb13976f45642b827cf8976af2
    ;;
  Linux-aarch64|Linux-arm64)
    ARCHIVE=treehouse-v${MX_TREEHOUSE_CI_VERSION}-linux-arm64.tar.gz
    SHA256=eaccc9c5b98125df8bd77425598eeecee66cb0371db4eb1cf75f0d813c18fab9
    ;;
  Darwin-arm64)
    ARCHIVE=treehouse-v${MX_TREEHOUSE_CI_VERSION}-darwin-arm64.tar.gz
    SHA256=7ee5078f3d1f33c01196548797fce65408e459d53530b77d4ba56e074fa1c1a2
    ;;
  Darwin-x86_64)
    ARCHIVE=treehouse-v${MX_TREEHOUSE_CI_VERSION}-darwin-amd64.tar.gz
    SHA256=1cf44580a5837f995e1d3bb74f4fbd3112b642acd20406087d9735a8106112fd
    ;;
  *)
    die "unsupported platform ${os}-${arch}; official Treehouse assets are linux/darwin amd64 and arm64"
    ;;
esac

URL="https://github.com/${MX_TREEHOUSE_CI_REPO}/releases/download/${MX_TREEHOUSE_CI_TAG}/${ARCHIVE}"
TMP=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mx-treehouse.XXXXXX")
trap 'rm -rf "$TMP"' EXIT

printf 'mx-install-treehouse.sh: downloading %s from %s\n' "$ARCHIVE" "$URL" >&2
curl -fsSL --max-filesize "$MX_TREEHOUSE_CI_MAX_BYTES" "$URL" -o "$TMP/$ARCHIVE" \
  || die "download failed for $URL (bounded at $MX_TREEHOUSE_CI_MAX_BYTES bytes)"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256=$(sha256sum "$TMP/$ARCHIVE" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA256=$(shasum -a 256 "$TMP/$ARCHIVE" | awk '{print $1}')
else
  die "need sha256sum or shasum to verify the Treehouse archive"
fi

[ "$ACTUAL_SHA256" = "$SHA256" ] || die "checksum mismatch for $ARCHIVE (expected $SHA256, got $ACTUAL_SHA256)"

tar -xzf "$TMP/$ARCHIVE" -C "$TMP"
# Archive layout: a single `treehouse` binary at the archive root (verified for v2.0.1).
if [ -f "$TMP/treehouse" ]; then
  BIN="$TMP/treehouse"
elif [ -f "$TMP/treehouse-v${MX_TREEHOUSE_CI_VERSION}/treehouse" ]; then
  BIN="$TMP/treehouse-v${MX_TREEHOUSE_CI_VERSION}/treehouse"
else
  BIN=$(find "$TMP" -type f -name treehouse | head -n 1)
  [ -n "$BIN" ] || die "archive $ARCHIVE did not contain a treehouse binary"
fi

mkdir -p "$DESTINATION"
install -m 0755 "$BIN" "$DESTINATION/treehouse"

installed_version=$("$DESTINATION/treehouse" --version 2>/dev/null | tr -d '[:space:]')
# treehouse prints "v2.0.1" (leading v) on --version.
case "$installed_version" in
  "v${MX_TREEHOUSE_CI_VERSION}"|"${MX_TREEHOUSE_CI_VERSION}") ;;
  *)
    die "installed treehouse version is '${installed_version:-<empty>}', expected exact pin v${MX_TREEHOUSE_CI_VERSION}"
    ;;
esac

lease_help=$("$DESTINATION/treehouse" get --help 2>&1) \
  || die "could not inspect installed treehouse lease support"
case "$lease_help" in
  *--lease*) ;;
  *) die "installed treehouse does not support the required 'get --lease' capability" ;;
esac

printf 'mx-install-treehouse.sh: installed treehouse %s with get --lease to %s\n' \
  "$installed_version" "$DESTINATION/treehouse" >&2
"$DESTINATION/treehouse" --version
