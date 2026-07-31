#!/usr/bin/env bash
# Render one task's append-only observability journal.
#
# Usage:
#   mx-timeline.sh <task-id> [--since <duration|iso-time>] [--event <glob>]
#                            [--json] [--html]
#
# Text output preserves append order and renders one source-attributed row per
# valid event.
# --json preserves each matching JSONL record.
# --since accepts an ISO-8601 timestamp or an integer plus s, m, h, d, or w.
# --event uses shell-glob matching against the closed event name.
# --html writes data/<task-id>/timeline.html using the installed vplan visual
# module's self-check as its availability gate, then prints the artifact path.
# Malformed lines are skipped and reported once on stderr.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"

# shellcheck source=bin/mx-journal-lib.sh
. "$SCRIPT_DIR/mx-journal-lib.sh"

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'mx-timeline: %s\n' "$*" >&2
  exit 1
}

ID=${1:-}
case "$ID" in
  -h|--help|'') usage; [ -n "$ID" ] && exit 0 || exit 2 ;;
esac
shift
mx_journal_task_valid "$ID" || fail "invalid task id: $ID"

SINCE=
EVENT_GLOB='*'
MODE=text
while [ "$#" -gt 0 ]; do
  case "$1" in
    --since)
      [ "$#" -ge 2 ] || fail "--since requires a value"
      SINCE=$2
      shift 2
      ;;
    --event)
      [ "$#" -ge 2 ] || fail "--event requires a value"
      EVENT_GLOB=$2
      shift 2
      ;;
    --json)
      [ "$MODE" = text ] || fail "choose only one output mode"
      MODE=json
      shift
      ;;
    --html)
      [ "$MODE" = text ] || fail "choose only one output mode"
      MODE=html
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

JOURNAL="$STATE/$ID.journal"
[ -f "$JOURNAL" ] && [ ! -L "$JOURNAL" ] || fail "journal not found: $JOURNAL"
command -v jq >/dev/null 2>&1 || fail "jq is required"
command -v node >/dev/null 2>&1 || fail "node is required"

SINCE_MS=
if [ -n "$SINCE" ]; then
  SINCE_MS=$(node - "$SINCE" "${MX_TIMELINE_NOW_MS:-}" <<'NODE'
const value = process.argv[2];
const nowOverride = process.argv[3];
const now = nowOverride === "" ? Date.now() : Number(nowOverride);
if (!Number.isFinite(now)) process.exit(1);
const duration = /^([0-9]+)([smhdw])$/.exec(value);
if (duration) {
  const factor = {s: 1000, m: 60000, h: 3600000, d: 86400000, w: 604800000}[duration[2]];
  const amount = Number(duration[1]);
  if (!Number.isSafeInteger(amount)) process.exit(1);
  process.stdout.write(String(now - amount * factor));
} else {
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) process.exit(1);
  process.stdout.write(String(parsed));
}
NODE
  ) || fail "invalid --since value: $SINCE"
fi

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/mx-timeline.XXXXXX") || fail "could not create scratch directory"
trap 'rm -rf "$TMP_DIR"' EXIT
FILTERED="$TMP_DIR/filtered.jsonl"
ROWS="$TMP_DIR/rows.tsv"
: >"$FILTERED"
: >"$ROWS"
MALFORMED=0

render_detail() {
  jq -r '
    . as $e |
    if .event == "task.spawned" then
      "kind=\(.detail.kind) backend=\(.detail.backend) branch=\(.detail.branch)"
    elif .event == "status.reported" then
      "\(.detail.raw)" + (if .detail.validated then " [validated]" else "" end)
    elif .event == "status.classified" then
      "\(.detail.verdict) (tier: \(.detail.tier); conflicts: \(.detail.conflicts | length))"
    elif .event == "gate.step.started" then
      "step=\(.detail.step) round=\(.detail.round)"
    elif .event == "gate.step.finished" then
      "step=\(.detail.step) round=\(.detail.round) findings=\(.detail.findings) outcome=\(.detail.outcome)"
    elif .event == "hold.opened" then
      "\(.detail.hold_id): \(.detail.title)"
    elif .event == "hold.resolved" then
      "\(.detail.hold_id) -> \(.detail.routed_to | join(", "))"
    elif .event == "workflow.stage.entered" then
      "run=\(.detail.run) stage=\(.detail.stage)"
    elif .event == "workflow.stage.gated" then
      "run=\(.detail.run) stage=\(.detail.stage) gate=\(.detail.gate) outcome=\(.detail.outcome)"
    elif .event == "delivery.queued" or .event == "delivery.pushed" then
      "branch=\(.detail.branch) sha=\(.detail.sha)"
    elif .event == "delivery.pr_opened" then
      .detail.pr_url
    else
      (.detail | tojson)
    end
  '
}

while IFS= read -r line || [ -n "$line" ]; do
  if ! printf '%s' "$line" | jq -e '
    type == "object" and
    (.ts | type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")) and
    (.task | type == "string") and
    (.source | type == "string") and
    (.event | type == "string") and
    (.detail | type == "object")
  ' >/dev/null 2>&1; then
    MALFORMED=$((MALFORMED + 1))
    continue
  fi
  LINE_TASK=$(printf '%s' "$line" | jq -r '.task')
  [ "$LINE_TASK" = "$ID" ] || {
    MALFORMED=$((MALFORMED + 1))
    continue
  }
  LINE_EVENT=$(printf '%s' "$line" | jq -r '.event')
  if ! mx_journal_event_valid "$LINE_EVENT"; then
    MALFORMED=$((MALFORMED + 1))
    continue
  fi
  case "$LINE_EVENT" in
    $EVENT_GLOB) ;;
    *) continue ;;
  esac
  LINE_TS=$(printf '%s' "$line" | jq -r '.ts')
  LINE_MS=$(node -e '
    const parsed = Date.parse(process.argv[1]);
    if (!Number.isFinite(parsed)) process.exit(1);
    process.stdout.write(String(parsed));
  ' "$LINE_TS" 2>/dev/null) || {
    MALFORMED=$((MALFORMED + 1))
    continue
  }
  if [ -n "$SINCE_MS" ] && [ "$LINE_MS" -lt "$SINCE_MS" ]; then
    continue
  fi
  printf '%s\n' "$line" >>"$FILTERED"
  LINE_SOURCE=$(printf '%s' "$line" | jq -r '.source')
  LINE_DETAIL=$(printf '%s' "$line" | render_detail)
  printf '%s\t%s\t%s\t%s\n' "${LINE_TS#*T}" "$LINE_SOURCE" "$LINE_EVENT" "$LINE_DETAIL" \
    | sed 's/Z	/	/' >>"$ROWS"
done <"$JOURNAL"

[ "$MALFORMED" -eq 0 ] \
  || printf 'mx-timeline: skipped %s malformed journal line(s)\n' "$MALFORMED" >&2

case "$MODE" in
  json)
    cat "$FILTERED"
    ;;
  text)
    while IFS=$'\t' read -r time source event detail; do
      [ -n "$time" ] || continue
      printf '%-8s  %-20s  %-24s  %s\n' "$time" "$source" "$event" "$detail"
    done <"$ROWS"
    ;;
  html)
    VPLAN=${MX_VPLAN_BIN:-$SCRIPT_DIR/mx-vplan.sh}
    [ -x "$VPLAN" ] || fail "vplan module is unavailable"
    "$VPLAN" --self-check >/dev/null 2>&1 || fail "vplan module is unavailable or invalid"
    ARTIFACT="$DATA/$ID/timeline.html"
    mkdir -p "$(dirname "$ARTIFACT")" || fail "could not create timeline artifact directory"
    node - "$ID" "$FILTERED" "$ARTIFACT" <<'NODE' || fail "could not render timeline artifact"
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const [id, input, destination] = process.argv.slice(2);
const events = fs.readFileSync(input, "utf8").split(/\n/).filter(Boolean).map(JSON.parse);
const escape = value => String(value)
  .replaceAll("&", "&amp;").replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;").replaceAll('"', "&quot;");
const rows = events.map(event => `<tr>
<td><time datetime="${escape(event.ts)}">${escape(event.ts)}</time></td>
<td><code>${escape(event.source)}</code></td>
<td><code>${escape(event.event)}</code></td>
<td><pre>${escape(JSON.stringify(event.detail, null, 2))}</pre></td>
</tr>`).join("\n");
const html = `<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${escape(id)} timeline · Multplx</title>
<style>
:root{color-scheme:dark;--bg:#0d1117;--surface:#161b22;--border:#30363d;--text:#e6edf3;--muted:#8b949e;--accent:#79c0ff}
*{box-sizing:border-box}body{margin:0;padding:40px;background:var(--bg);color:var(--text);font:15px/1.5 system-ui,sans-serif}
main{max-width:1200px;margin:auto}h1{margin:0 0 8px}.lede{color:var(--muted);margin:0 0 28px}
.table{overflow:auto;border:1px solid var(--border);border-radius:10px}table{width:100%;border-collapse:collapse;background:var(--surface)}
th,td{padding:12px;text-align:left;vertical-align:top;border-bottom:1px solid var(--border)}th{color:var(--accent)}
code,pre{font:12px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace}pre{margin:0;white-space:pre-wrap}
</style></head><body><main><h1>${escape(id)}</h1>
<p class="lede">Append-only task timeline · ${events.length} event${events.length === 1 ? "" : "s"} · observability only</p>
<div class="table"><table><thead><tr><th>Time</th><th>Source</th><th>Event</th><th>Detail</th></tr></thead>
<tbody>${rows}</tbody></table></div></main></body></html>\n`;
const temporary = path.join(path.dirname(destination), `.${path.basename(destination)}.${process.pid}.${crypto.randomBytes(4).toString("hex")}.tmp`);
fs.writeFileSync(temporary, html, {encoding: "utf8", mode: 0o600});
fs.renameSync(temporary, destination);
NODE
    printf '%s\n' "$ARTIFACT"
    ;;
esac
