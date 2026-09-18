#!/bin/sh
# Create an isolated 20-record home for the real system-snapshot collector.
set -eu

[ "$#" -eq 1 ] || { printf '%s\n' "usage: $0 <empty-destination>" >&2; exit 2; }
destination=$1
[ ! -e "$destination" ] || { printf '%s\n' "destination already exists: $destination" >&2; exit 1; }
mkdir -p "$destination/state" "$destination/data" "$destination/config" "$destination/projects"
{
  printf '%s\n\n%s\n\n' '# Backlog' '## In flight'
  ordinal=1
  while [ "$ordinal" -le 20 ]; do
    printf '%s\n' "- [ ] task-$ordinal - Task $ordinal (repo: repo-$ordinal, kind: delivery)"
    ordinal=$((ordinal + 1))
  done
} >"$destination/data/backlog.md"
ordinal=1
while [ "$ordinal" -le 20 ]; do
  {
    printf '%s\n' 'backend=tmux'
    printf '%s\n' "project=repo-$ordinal"
    printf '%s\n' 'kind=delivery'
    printf '%s\n' 'mode=delivery'
  } >"$destination/state/task-$ordinal.meta"
  printf '%s\n' "working: fixture task $ordinal" >"$destination/state/task-$ordinal.status"
  ordinal=$((ordinal + 1))
done
