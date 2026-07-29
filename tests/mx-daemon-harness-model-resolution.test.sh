#!/usr/bin/env bash
# Daemon harness/model resolution and spawn override contracts.
set -u
MX_TEST_CASE_GROUP=harness-model-resolution
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/daemon-harness-helpers.sh"
