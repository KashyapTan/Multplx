#!/usr/bin/env bash
# Snapshot projection and reconciliation contracts.
set -u
MX_TEST_CASE_GROUP=projection-reconciliation
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/status-snapshot-helpers.sh"
