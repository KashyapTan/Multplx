#!/usr/bin/env bash
# Test-only tmux input/Enter adapter. Existing fixture logging and lifecycle
# responses remain in the caller; only a submitted owned script is executed.
set -eu
[ "${1:-}" = send-keys ] || exit 0
fixture_dir=$(cd "$(dirname "$0")" && pwd -P)
input="$fixture_dir/.inert-launch-input"
previous=
for argument in "$@"; do
  if [ "$previous" = -l ]; then printf '%s' "$argument" > "$input"; fi
  previous=$argument
done
for argument in "$@"; do
  if [ "$argument" = Enter ] && [ -f "$input" ]; then
    bash "$fixture_dir/inert-terminal-start.sh" "$(cat "$input")"
    rm -f "$input"
  fi
done
