#!/usr/bin/env bash
# PR security terminal-retirement and teardown contracts.
set -u
MX_TEST_CASE_GROUP=retirement-teardown
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/pr-check-security-helpers.sh"
