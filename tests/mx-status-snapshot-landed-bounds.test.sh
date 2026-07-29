#!/usr/bin/env bash
# Snapshot landed-work ordering and output-bound contracts.
set -u
MX_TEST_CASE_GROUP=landed-bounds
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/status-snapshot-helpers.sh"
