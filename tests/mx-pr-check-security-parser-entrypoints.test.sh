#!/usr/bin/env bash
# PR security parser and entrypoint contracts split from the Plan-06 suite.
set -u
MX_TEST_CASE_GROUP=parser-entrypoints
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/pr-check-security-helpers.sh"
