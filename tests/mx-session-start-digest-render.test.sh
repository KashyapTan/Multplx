#!/usr/bin/env bash
# Session-start digest, rendering, backlog, and supervision contracts.
set -u
MX_TEST_CASE_GROUP=digest-render
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/session-start-helpers.sh"
