#!/usr/bin/env bash
# Daemon config-reread generation, retry, and cleanup contracts.
set -u
MX_TEST_CASE_GROUP=config-reread-retry
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/daemon-harness-helpers.sh"
