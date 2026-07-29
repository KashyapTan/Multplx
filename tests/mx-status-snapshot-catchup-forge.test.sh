#!/usr/bin/env bash
# Snapshot catchup rendering and opt-in forge-enrichment contracts.
set -u
MX_TEST_CASE_GROUP=catchup-forge
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/status-snapshot-helpers.sh"
