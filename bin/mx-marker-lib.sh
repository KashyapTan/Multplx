#!/usr/bin/env bash
# mx-marker-lib.sh - compatibility entry point for from-broker routing.
#
# bin/mx-operational-input.sh owns current operational-input construction,
# parsing, marker bytes, and the established from-broker compatibility
# carrier. Existing callers source this path so they do not need a flag-day
# migration. No side effects on source. set -u / set -e safe.

_MX_MARKER_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=bin/mx-operational-input.sh
. "$_MX_MARKER_LIB_DIR/mx-operational-input.sh"
unset _MX_MARKER_LIB_DIR
