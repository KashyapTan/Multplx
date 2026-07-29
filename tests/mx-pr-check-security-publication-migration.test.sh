#!/usr/bin/env bash
# PR security publication and non-executing migration contracts.
set -u
MX_TEST_CASE_GROUP=publication-migration
export MX_TEST_CASE_GROUP
. "$(dirname "${BASH_SOURCE[0]}")/pr-check-security-helpers.sh"
