#!/usr/bin/env bash
# Opt-in authenticated Cursor CLI smoke test; never mutates global Cursor settings.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

[ "${MX_CURSOR_LIVE_TESTS:-0}" = 1 ] || { echo "skip: set MX_CURSOR_LIVE_TESTS=1 for authenticated Cursor CLI verification"; exit 0; }
AGENT=$(command -v agent || command -v cursor-agent || true)
[ -n "$AGENT" ] || { echo "skip: Cursor agent CLI not found"; exit 0; }
"$AGENT" status >/dev/null || fail "Cursor CLI is not authenticated"
version=$("$AGENT" --version) || fail "Cursor CLI version is unavailable"
case "$version" in 2026.*) ;; *) fail "Cursor CLI version is outside the verified 2026 adapter line: $version" ;; esac
help=$("$AGENT" --help)
assert_contains "$help" '--sandbox <mode>' "Cursor CLI no longer exposes explicit sandbox control"
assert_contains "$help" '--plugin-dir <path>' "Cursor CLI no longer exposes per-run plugins"
assert_contains "$help" '--resume [chatId]' "Cursor CLI no longer exposes exact resume"
pass "authenticated Cursor CLI retains the verified sandbox, plugin, and resume surface ($version)"
