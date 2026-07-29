#!/usr/bin/env bash
# PR security fault, quarantine, and descendant-cleanup contracts.
set -u
MX_TEST_CASE_GROUP=fault-quarantine
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/pr-check-security-helpers.sh"
