#!/usr/bin/env bash
# Session-start real child-process and endpoint-liveness contracts.
set -u
MX_TEST_CASE_GROUP=process-liveness
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/session-start-helpers.sh"
