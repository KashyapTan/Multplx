#!/usr/bin/env bash
# tests/mx-removed-deps.test.sh - regression tripwire for the plan-01 deletions.
#
# Plan 01 of the Multplx port removed five subsystems outright: the
# mybroker.io social relay (X mode), the standalone shellcheck gate and
# installer, glab/GitLab forge support, the osascript wedge-alarm channel, and
# the pruned backends/harnesses (zellij, orca / grok, opencode). This test
# asserts they stay dead: no deleted file reappears and no kept file grows a
# reference back to a removed subsystem.
#
# Scope: the Multplx tree only (bin/ tests/ docs/ skills/ .agents/ .github/
# AGENTS.md README.md CONTRIBUTING.md .gitignore). The read-only upstream
# reference folder and historical planning material legitimately describe the
# removed subsystems and are excluded. Inert `# shellcheck` lint directives
# inside scripts are allowed.

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$ROOT"

SCOPE=(bin tests docs skills .agents .github AGENTS.md README.md CONTRIBUTING.md .gitignore)
SELF=tests/mx-removed-deps.test.sh

# --- deleted files must stay deleted ----------------------------------------

test_deleted_files_absent() {
  local path
  for path in \
    bin/mx-x-lib.sh bin/mx-x-poll.sh bin/mx-x-reply.sh bin/mx-x-dismiss.sh \
    bin/mx-x-followup.sh bin/mx-x-link.sh \
    .agents/skills/fmx-respond \
    bin/mx-lint.sh bin/mx-install-shellcheck.sh \
    docs/gitlab-merge-watch.md docs/wedge-alarm.md \
    bin/backends/zellij.sh bin/backends/orca.sh \
    docs/zellij-backend.md docs/orca-backend.md \
    .agents/skills/broker-orca \
    bin/mx-turnend-guard-grok.sh .grok .opencode \
    docs/supervision-protocols/grok.md docs/supervision-protocols/opencode.md \
    tests/mx-x-mode.test.sh tests/mx-lint.test.sh \
    tests/mx-backend-zellij.test.sh tests/mx-backend-zellij-smoke.test.sh \
    tests/zellij-test-safety.sh tests/mx-backend-orca.test.sh \
    tests/mx-grok-continuity-live-e2e.test.sh tests/mx-grok-harness.test.sh \
    tests/mx-opencode-primary-live-e2e.test.sh \
    bin/mx-launcher-lib.sh bin/mx-arm-command-policy.mjs \
    bin/mx-cd-command-policy.mjs bin/mx-report-mcp.mjs \
    bin/mx-viz-server.mjs bin/mx-vplan-server.mjs \
    bin/backends/herdr-eventwait.py bin/backends/herdr-workspace-move.py \
    tests/mx-backend-herdr-eventwait.test.py AGENTS-PORTING.md; do
    [ ! -e "$path" ] || fail "removed path has reappeared: $path"
  done
  pass "every plan-01-deleted path stays deleted"
}

# --- reference sweeps --------------------------------------------------------

# grep_hits <pattern> [extra-filter...]: prints matches of the extended-regex
# pattern across SCOPE, excluding this test itself.
grep_hits() {
  local pattern=$1
  grep -rniE "$pattern" "${SCOPE[@]}" 2>/dev/null \
    | grep -Fv "$SELF:" \
    | grep -Fv 'docs/mx-test-isolation-proof.json:' \
    | grep -Fv 'docs/upstream.md:' \
    | grep -Fv 'tests/mx-upstream-diff.test.sh:' \
    | grep -Fv 'tests/fixtures/upstream-sync/' || true
}

assert_no_hits() {
  local label=$1 hits=$2
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" >&2
    fail "$label references survive (see stderr)"
  fi
  pass "$label: zero references"
}

test_no_x_mode_references() {
  assert_no_hits "X-mode relay (FMX_/mx-x-/mybroker)" \
    "$(grep_hits 'FMX_|mx-x-|mybroker')"
  # x-mode cadence plumbing; the leading boundary spares TMUX_MODE/codex-mode.
  assert_no_hits "x-mode supervision plumbing" \
    "$(grep_hits '(^|[^a-z_-])x[-_ ]mode' | grep -viE 'mx-x-|FMX_')"
}

test_no_lint_gate_references() {
  assert_no_hits "standalone lint gate (mx-lint/mx-install-shellcheck)" \
    "$(grep_hits 'mx-lint\.sh|mx-install-shellcheck')"
  # shellcheck as a dependency; inert lint directives inside scripts stay.
  assert_no_hits "shellcheck dependency" \
    "$(grep_hits 'shellcheck' | grep -v '# shellcheck')"
}

test_no_gitlab_references() {
  # \b spares hiddenThinkingLabel (pi UI API) which contains "nglab".
  assert_no_hits "GitLab/glab forge support" \
    "$(grep_hits '\bglab\b|gitlab')"
}

test_no_osascript_references() {
  assert_no_hits "osascript wedge-alarm channel" "$(grep_hits 'osascript')"
}

test_no_pruned_backend_references() {
  assert_no_hits "pruned backends (zellij/orca)" \
    "$(grep_hits '\bzellij\b|\borca\b')"
}

test_no_pruned_harness_references() {
  assert_no_hits "pruned harnesses (grok/opencode)" \
    "$(grep_hits '\bgrok\b|opencode')"
}

# --- structural pins ---------------------------------------------------------

test_backend_registry_is_pruned() {
  grep -q 'MX_BACKEND_KNOWN="tmux herdr cmux"' bin/mx-backend.sh \
    || fail "MX_BACKEND_KNOWN must be exactly \"tmux herdr cmux\""
  pass "backend registry is exactly tmux herdr cmux"
}

test_ci_has_no_lint_step() {
  ! grep -qE 'mx-lint|shellcheck' .github/workflows/ci.yml \
    || fail "ci.yml has grown a lint/shellcheck step back"
  pass "ci.yml carries no standalone lint step"
}

test_no_retired_plan13_runtime_paths() {
  local hits
  hits=$(grep -rniE \
    'mx-launcher-lib\.sh|mx-report-mcp\.mjs|mx-viz-server\.mjs|mx-vplan-server\.mjs|herdr-eventwait\.py|herdr-workspace-move\.py' \
    bin share/shell .github .agents README.md CONTRIBUTING.md CLAUDE.md AGENTS.md 2>/dev/null || true)
  assert_no_hits "retired Plan 13 runtime paths" "$hits"
}

test_deleted_files_absent
test_no_x_mode_references
test_no_lint_gate_references
test_no_gitlab_references
test_no_osascript_references
test_no_pruned_backend_references
test_no_pruned_harness_references
test_backend_registry_is_pruned
test_ci_has_no_lint_step
test_no_retired_plan13_runtime_paths
