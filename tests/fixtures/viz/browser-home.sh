#!/bin/sh
# Build an isolated canonical-schema home for browser-only dashboard acceptance.
set -eu

[ "$#" -eq 1 ] || { printf '%s\n' "usage: $0 <empty-destination>" >&2; exit 2; }
destination=$1
[ ! -e "$destination" ] || { printf '%s\n' "destination already exists: $destination" >&2; exit 1; }
mkdir -p "$destination/state/brief-revisions" "$destination/data" "$destination/config" "$destination/projects"
destination=$(cd "$destination" && pwd -P)
repo=$(cd "$(dirname "$0")/../../.." && pwd -P)
brief="$destination/state/brief-revisions/task-1.md"
printf '%s\n' '# Accepted fixture brief' '' 'Archived acceptance marker: phase10-browser-brief' >"$brief"
node "$repo/tests/fixtures/viz/portfolio.mjs" 20 | jq --arg home "$destination" --arg brief "$brief" '
  .portfolio.tasks[0].owner.home=$home |
  .portfolio.tasks[0].key=($home+"#task:task-1") |
  .portfolio.tasks[0].brief.path=$brief
' >"$destination/snapshot.json"
cat >"$destination/snapshot-reader.sh" <<'SH'
#!/bin/sh
cat "$MX_VIZ_FIXTURE"
SH
chmod +x "$destination/snapshot-reader.sh"
