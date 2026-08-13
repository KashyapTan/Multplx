#!/usr/bin/env bash
# Shared read-only environment probes for bootstrap and mx-doctor.
#
# This file is the single owner of:
# - required external-tool presence and Treehouse durable-lease compatibility;
# - supported install guidance for those tools;
# - primary-checkout worktree-tangle classification.
#
# mx_probe_tool_records <backend> prints tab-separated structured records:
#   MISSING<TAB><tool><TAB><install command>
#   MISSING_MANUAL<TAB><tool><TAB><instructions URL>
#   BACKEND_INVALID<TAB><backend><TAB><known backend list>
#
# mx_probe_tangle_record <root> prints:
#   <feature branch><TAB><expected default branch>
#
# The bootstrap renderers below preserve mx-bootstrap.sh's established output
# contract. Other callers should consume the structured record functions rather
# than parse those legacy lines.

MX_PROBE_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v mx_backend_required_tools >/dev/null 2>&1; then
  # shellcheck source=bin/mx-backend.sh disable=SC1091
  . "$MX_PROBE_LIB_DIR/mx-backend.sh"
fi
if ! command -v mx_primary_tangle_branch >/dev/null 2>&1; then
  # shellcheck source=bin/mx-tangle-lib.sh disable=SC1091
  . "$MX_PROBE_LIB_DIR/mx-tangle-lib.sh"
fi

MX_PROBE_COMMON_TOOLS="git gh jq treehouse"

mx_probe_install_cmd() {
  case "$1" in
    tmux|git|gh|curl|jq) echo "brew install $1  # or the platform's package manager" ;;
    cmux) echo "brew install --cask cmux  # or see https://cmux.com" ;;
    treehouse) echo "curl -fsSL https://kunchenguid.github.io/treehouse/install.sh | sh" ;;
    *) return 1 ;;
  esac
}

mx_probe_manual_install_url() {
  case "$1" in
    herdr) echo "https://herdr.dev" ;;
    *) return 1 ;;
  esac
}

mx_probe_missing_tool_record() {
  local tool=$1 instructions
  if instructions=$(mx_probe_manual_install_url "$tool"); then
    printf 'MISSING_MANUAL\t%s\t%s\n' "$tool" "$instructions"
    return 0
  fi
  printf 'MISSING\t%s\t%s\n' "$tool" "$(mx_probe_install_cmd "$tool")"
}

mx_probe_treehouse_supports_lease() {
  treehouse get --help 2>&1 | grep -Eq '(^|[^[:alnum:]_-])--lease([^[:alnum:]_-]|$)'
}

mx_probe_tool_records() {
  local backend=$1 backend_tools='' tool
  if ! backend_tools=$(mx_backend_required_tools "$backend"); then
    printf 'BACKEND_INVALID\t%s\t%s\n' "$backend" "$MX_BACKEND_KNOWN"
    backend_tools=''
  fi
  for tool in $backend_tools; do
    mx_backend_required_tool_available "$backend" "$tool" \
      || mx_probe_missing_tool_record "$tool"
  done
  for tool in $MX_PROBE_COMMON_TOOLS; do
    command -v "$tool" >/dev/null 2>&1 || mx_probe_missing_tool_record "$tool"
  done
  if command -v treehouse >/dev/null 2>&1 && ! mx_probe_treehouse_supports_lease; then
    mx_probe_missing_tool_record treehouse
  fi
}

mx_probe_render_bootstrap_tool_record() {
  local code=$1 subject=$2 detail=$3
  case "$code" in
    MISSING)
      printf 'MISSING: %s (install: %s)\n' "$subject" "$detail"
      ;;
    MISSING_MANUAL)
      printf 'MISSING_MANUAL: %s (instructions: %s)\n' "$subject" "$detail"
      ;;
    BACKEND_INVALID)
      printf 'BACKEND_INVALID: %s (known: %s)\n' "$subject" "$detail"
      ;;
  esac
}

mx_probe_bootstrap_tools() {
  local backend=$1 code subject detail
  while IFS=$'\t' read -r code subject detail; do
    [ -n "$code" ] || continue
    mx_probe_render_bootstrap_tool_record "$code" "$subject" "$detail"
  done < <(mx_probe_tool_records "$backend")
}

mx_probe_tangle_record() {
  local root=$1 branch default
  branch=$(mx_primary_tangle_branch "$root" 2>/dev/null || true)
  [ -n "$branch" ] || return 0
  default=$(mx_default_branch "$root" 2>/dev/null || echo main)
  printf '%s\t%s\n' "$branch" "$default"
}

mx_probe_bootstrap_tangle() {
  local root=$1 read_only=${2:-0} record branch default
  record=$(mx_probe_tangle_record "$root")
  [ -n "$record" ] || return 0
  IFS=$'\t' read -r branch default <<EOF
$record
EOF
  if [ "$read_only" = 1 ]; then
    printf "TANGLE: primary checkout on feature branch '%s' (expected '%s'); the work is safe on that ref - read-only session must leave restore work to the session holding the system lock\n" \
      "$branch" "$default"
  else
    printf "TANGLE: primary checkout on feature branch '%s' (expected '%s'); the work is safe on that ref - restore the primary with: git -C %s checkout %s, then re-validate the branch in a proper worktree\n" \
      "$branch" "$default" "$root" "$default"
  fi
}
