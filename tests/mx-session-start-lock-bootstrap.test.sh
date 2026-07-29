#!/usr/bin/env bash
# Session-start lock and bootstrap composition contracts.
set -u
MX_TEST_CASE_GROUP=lock-bootstrap
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/session-start-helpers.sh"
