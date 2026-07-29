#!/usr/bin/env bash
# mx-test-isolation-proof.sh - repeatable conflict-matrix and leak proof for
# the resource-aware Multplx behavior-test scheduler.
#
# The resource manifest is owned by bin/mx-test-run.sh. This harness consumes
# that manifest, prints its conflict matrix, and repeatedly executes every
# portable non-global/non-live script through the production scheduler.
#
# Usage:
#   mx-test-isolation-proof.sh [--jobs N] [--repeats N] [--json path]
#   mx-test-isolation-proof.sh --list
#   mx-test-isolation-proof.sh --list-resources
#   mx-test-isolation-proof.sh --list-conflicts
#   mx-test-isolation-proof.sh --list-exclusions
#
# Options:
#   --jobs N          scheduler worker cap (default: 4)
#   --repeats N       complete stress rounds (default: 2)
#   --json path       write the combined proof artifact
#   --list            list scripts covered by stress rounds
#   --list-resources  print the complete runner-owned resource manifest
#   --list-conflicts  print every pair that must not overlap
#   --list-exclusions print scripts kept out of portable stress and why
#   -h, --help        print this header
#
# Markers:
#   MX_ISOLATION_BEGIN <iso8601> concurrency=<n> candidates=<n> repeats=<n>
#   MX_ISOLATION_ROUND_END repeat=<n> exit=<n> duration_ms=<n>
#   MX_ISOLATION_SUMMARY total=<n> failed_rounds=<n> concurrency=<n> repeats=<n> duration_ms=<n> leaks=<n>
set -eu

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNNER="$ROOT/bin/mx-test-run.sh"
BASELINE="$ROOT/docs/mx-test-performance-baseline.json"
cd "$ROOT" || exit 1

JOBS=4
REPEATS=2
JSON_PATH=
MODE=run

usage() {
  awk '
    NR == 1 { next }
    /^#/ { sub(/^# ?/, ""); print; next }
    { exit }
  ' "$0" >&2
}

die() {
  printf 'mx-test-isolation-proof: %s\n' "$*" >&2
  exit 2
}

now_iso() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

now_ms() {
  python3 -c 'import time; print(int(time.time() * 1000))'
}

list_resources() {
  "$RUNNER" --list-resources --all
}

list_candidates() {
  list_resources | awk -F '\t' '
    $2 !~ /(^|,)(global|live-harness|herdr-session|cmux-app)(,|$)/ { print $1 }
  '
}

list_exclusions() {
  list_resources | awk -F '\t' '
    $1 == "tests/mx-pr-check-security-publication-migration.test.sh" && $2 ~ /(^|,)global(,|$)/ {
      print $1 "\tload-sensitive publication race retains its ten-second hang tripwire"
      next
    }
    $2 ~ /(^|,)global(,|$)/ { print $1 "\tglobal scheduler owner/self-contract" ; next }
    $2 ~ /(^|,)live-harness(,|$)/ { print $1 "\tlive harness opt-in is not a portable stress resource" ; next }
    $2 ~ /(^|,)herdr-session(,|$)/ { print $1 "\treal Herdr remains in its dedicated owned lab lane" ; next }
    $2 ~ /(^|,)cmux-app(,|$)/ { print $1 "\tGUI-owned cmux resource is environment gated" ; next }
  '
}

list_conflicts() {
  local manifest
  manifest=$(mktemp "${TMPDIR:-/tmp}/mx-isolation-manifest.XXXXXX")
  list_resources >"$manifest"
  python3 - "$manifest" <<'PY'
import itertools
import sys

rows = []
for line in open(sys.argv[1], encoding="utf-8"):
    path, raw = line.rstrip("\n").split("\t")
    rows.append((path, set(raw.split(","))))
for (left, lres), (right, rres) in itertools.combinations(rows, 2):
    shared = set()
    if "global" in lres or "global" in rres:
        shared.add("global")
    else:
        shared = (lres - {"none"}) & (rres - {"none"})
    if shared:
        print(f"{left}\t{right}\t{','.join(sorted(shared))}")
PY
  rm -f "$manifest"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --jobs)
      [ "$#" -gt 1 ] || die "--jobs requires a positive integer"
      JOBS=$2
      shift 2
      ;;
    --jobs=*)
      JOBS=${1#--jobs=}
      shift
      ;;
    --repeats)
      [ "$#" -gt 1 ] || die "--repeats requires a positive integer"
      REPEATS=$2
      shift 2
      ;;
    --repeats=*)
      REPEATS=${1#--repeats=}
      shift
      ;;
    --json)
      [ "$#" -gt 1 ] || die "--json requires a path"
      JSON_PATH=$2
      shift 2
      ;;
    --json=*)
      JSON_PATH=${1#--json=}
      shift
      ;;
    --list) MODE=list; shift ;;
    --list-resources) MODE=resources; shift ;;
    --list-conflicts) MODE=conflicts; shift ;;
    --list-exclusions) MODE=exclusions; shift ;;
    -h|--help) usage; exit 0 ;;
    -*) die "unknown option: $1" ;;
    *) die "unexpected argument: $1" ;;
  esac
done

case "$JOBS:$REPEATS" in
  *[!0-9:]*|:*|*:) die "--jobs and --repeats must be positive integers" ;;
esac
[ "$JOBS" -ge 1 ] || die "--jobs must be >= 1"
[ "$JOBS" -le 8 ] || die "--jobs is capped at 8"
[ "$REPEATS" -ge 1 ] || die "--repeats must be >= 1"

case "$MODE" in
  list) list_candidates; exit 0 ;;
  resources) list_resources; exit 0 ;;
  conflicts) list_conflicts; exit 0 ;;
  exclusions) list_exclusions; exit 0 ;;
esac

command -v python3 >/dev/null 2>&1 || die "python3 is required"
CANDIDATES=()
while IFS= read -r script; do
  [ -n "$script" ] || continue
  CANDIDATES+=("$script")
done < <(list_candidates)
[ "${#CANDIDATES[@]}" -gt 0 ] || die "portable proof candidate set is empty"

PROOF_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mx-isolation-proof.XXXXXX")
chmod 0700 "$PROOF_ROOT"
RUNNER_PID=

cleanup() {
  trap - EXIT INT TERM
  if [ -n "$RUNNER_PID" ] && kill -0 "$RUNNER_PID" 2>/dev/null; then
    kill "$RUNNER_PID" 2>/dev/null || true
    wait "$RUNNER_PID" 2>/dev/null || true
  fi
  chmod -R u+w "$PROOF_ROOT" 2>/dev/null || true
  rm -rf "$PROOF_ROOT"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

git_snapshot() {
  git config --global --list 2>/dev/null | LC_ALL=C sort || true
}

MANIFEST="$PROOF_ROOT/manifest.tsv"
CONFLICTS="$PROOF_ROOT/conflicts.tsv"
ROUNDS="$PROOF_ROOT/rounds.tsv"
list_resources >"$MANIFEST"
list_conflicts >"$CONFLICTS"
: >"$ROUNDS"

GIT_BEFORE=$(git_snapshot)
STARTED_ISO=$(now_iso)
STARTED_MS=$(now_ms)
FAILED_ROUNDS=0
LEAKS=0
KNOWN_FAILURE_OBSERVATIONS=0
repeat=1

printf 'MX_ISOLATION_BEGIN %s concurrency=%s candidates=%s repeats=%s\n' \
  "$STARTED_ISO" "$JOBS" "${#CANDIDATES[@]}" "$REPEATS"

while [ "$repeat" -le "$REPEATS" ]; do
  round_json="$PROOF_ROOT/round-$repeat.json"
  round_log="$PROOF_ROOT/round-$repeat.log"
  round_started=$(now_ms)
  set +e
  "$RUNNER" --jobs "$JOBS" --json "$round_json" "${CANDIDATES[@]}" >"$round_log" 2>&1 &
  RUNNER_PID=$!
  wait "$RUNNER_PID"
  round_rc=$?
  RUNNER_PID=
  set -e
  contract_rc=$round_rc
  known_in_round=0
  if [ -s "$round_json" ]; then
    set +e
    known_in_round=$(python3 - "$round_json" "$BASELINE" <<'PY'
import json
import sys

run = json.load(open(sys.argv[1], encoding="utf-8"))
baseline = json.load(open(sys.argv[2], encoding="utf-8"))
known = baseline.get("known_failure", {}).get("path")
failed = [row["path"] for row in run.get("scripts", []) if int(row.get("exit", 0)) != 0]
unexpected = [path for path in failed if path != known]
if unexpected:
    print(0)
    raise SystemExit(1)
print(sum(path == known for path in failed))
PY
    )
    acceptance_rc=$?
    set -e
    [ -n "$known_in_round" ] || known_in_round=0
    if [ "$acceptance_rc" -eq 0 ]; then
      contract_rc=0
    fi
  fi
  KNOWN_FAILURE_OBSERVATIONS=$((KNOWN_FAILURE_OBSERVATIONS + known_in_round))
  round_finished=$(now_ms)
  round_duration=$((round_finished - round_started))
  [ "$round_duration" -ge 0 ] || round_duration=0
  [ "$contract_rc" -eq 0 ] || FAILED_ROUNDS=$((FAILED_ROUNDS + 1))
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "$repeat" "$round_rc" "$contract_rc" "$round_duration" "$round_json" >>"$ROUNDS"
  printf 'MX_ISOLATION_ROUND_END repeat=%s exit=%s runner_exit=%s duration_ms=%s\n' \
    "$repeat" "$contract_rc" "$round_rc" "$round_duration"
  if [ "$contract_rc" -ne 0 ]; then
    tail -n 80 "$round_log" >&2 || true
  fi
  repeat=$((repeat + 1))
done

GIT_AFTER=$(git_snapshot)
if [ "$GIT_BEFORE" != "$GIT_AFTER" ]; then
  printf '%s\n' 'mx-test-isolation-proof: global git config changed during proof' >&2
  LEAKS=$((LEAKS + 1))
fi

MX_ISOLATION_LEAK_ROOT=$PROOF_ROOT
export MX_ISOLATION_LEAK_ROOT
process_leaks=$(ps -axo pid=,ppid=,command= 2>/dev/null \
  | awk -v self="$$" 'index($0, ENVIRON["MX_ISOLATION_LEAK_ROOT"]) && $1 != self { print }' || true)
unset MX_ISOLATION_LEAK_ROOT
if [ -n "$process_leaks" ]; then
  printf 'mx-test-isolation-proof: leaked proof-owned processes:\n%s\n' "$process_leaks" >&2
  LEAKS=$((LEAKS + 1))
fi

FINISHED_ISO=$(now_iso)
FINISHED_MS=$(now_ms)
DURATION=$((FINISHED_MS - STARTED_MS))

if [ -n "$JSON_PATH" ]; then
  mkdir -p "$(dirname "$JSON_PATH")"
  python3 - "$JSON_PATH" "$STARTED_ISO" "$FINISHED_ISO" "$JOBS" "$REPEATS" \
    "$DURATION" "$FAILED_ROUNDS" "$LEAKS" "$KNOWN_FAILURE_OBSERVATIONS" \
    "$MANIFEST" "$CONFLICTS" "$ROUNDS" <<'PY'
import hashlib
import json
import sys

(out, started, finished, jobs, repeats, duration, failed, leaks, known_failures,
 manifest_path, conflicts_path, rounds_path) = sys.argv[1:]
manifest_bytes = open(manifest_path, "rb").read()
resources = []
for line in manifest_bytes.decode().splitlines():
    path, raw = line.split("\t")
    resources.append({"path": path, "resources": raw.split(",")})
rounds = []
scripts = []
for line in open(rounds_path, encoding="utf-8"):
    repeat, runner_exit, contract_exit, duration_ms, path = line.rstrip("\n").split("\t")
    doc = json.load(open(path, encoding="utf-8"))
    rounds.append({
        "repeat": int(repeat),
        "exit": int(contract_exit),
        "runner_exit": int(runner_exit),
        "duration_ms": int(duration_ms),
        "run_id": doc.get("run_id"),
    })
    scripts.extend(dict(row, repeat=int(repeat)) for row in doc.get("scripts", []))
doc = {
    "kind": "resource-isolation-proof",
    "started_at": started,
    "finished_at": finished,
    "concurrency": int(jobs),
    "repeats": int(repeats),
    "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
    "resource_manifest": resources,
    "conflict_pairs": sum(1 for _ in open(conflicts_path, encoding="utf-8")),
    "rounds": rounds,
    "scripts": scripts,
    "summary": {
        "candidates_per_round": len(scripts) // max(int(repeats), 1),
        "failed_rounds": int(failed),
        "duration_ms": int(duration),
        "leaks": int(leaks),
        "known_failure_observations": int(known_failures),
    },
}
with open(out, "w", encoding="utf-8") as stream:
    json.dump(doc, stream, indent=2, sort_keys=True)
    stream.write("\n")
PY
fi

printf 'MX_ISOLATION_SUMMARY total=%s failed_rounds=%s concurrency=%s repeats=%s duration_ms=%s leaks=%s\n' \
  "${#CANDIDATES[@]}" "$FAILED_ROUNDS" "$JOBS" "$REPEATS" "$DURATION" "$LEAKS"

[ "$FAILED_ROUNDS" -eq 0 ] && [ "$LEAKS" -eq 0 ]
