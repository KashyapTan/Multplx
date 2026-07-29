#!/usr/bin/env bash
# Daemon spawn and inherited-config propagation contracts.
set -u
MX_TEST_CASE_GROUP=spawn-config-inheritance
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/daemon-harness-helpers.sh"
