#!/usr/bin/env bash
# Rust-owned unit contracts for deep-review sanitization, schemas, findings, and prompts.
set -eu
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cargo test -q -p multplx-cli --lib 'deep_review::tests' \
  || fail "Rust deep-review contract tests failed"
pass "Rust owns deep-review sanitization, schemas, findings, and prompt boundaries"
