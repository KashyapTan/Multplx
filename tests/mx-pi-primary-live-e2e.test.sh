#!/usr/bin/env bash
# Opt-in credentialed Pi continuity regression on a private tmux socket and
# isolated project/home state. It uses the existing shared Pi auth store without
# copying credentials and pins the maintainer-approved openai-codex model.
set -u

if [ "${MX_PI_LIVE_E2E:-0}" != 1 ]; then
  echo "skip: set MX_PI_LIVE_E2E=1 to run the isolated interactive Pi regression"
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
unset DEEP_REVIEW_GATE

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

command -v pi >/dev/null 2>&1 || fail "pi not found"
command -v tmux >/dev/null 2>&1 || fail "tmux not found"

TMUX=$(command -v tmux)
SOCKET="mx-pi-live-e2e-$$"
SESSION=pi-live-e2e
LAB="$ROOT/.pi-live-e2e.$$"
PROJECT="$LAB/project"
RECAP_PROJECT="$LAB/recap-project"
HOME_DIR="$LAB/fmhome"
PI_VERSION=$(pi --version)
# shellcheck source=/dev/null
. "$ROOT/bin/mx-operational-input.sh"
# shellcheck disable=SC2016 # Backticks are literal prompt markup.
LEGACY_START='Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.'
LEGACY_AWAY=$'\xE2\x81\xA3Supervisor escalate (1 event(s)): done: legacy rollout'
MARKER_NEAR_MISS=$'\xE2\x81\xA3Maintainer note: this invisible separator is intentional.'
# shellcheck disable=SC2016 # Backticks are literal prompt markup.
START_NEAR_MISS='Maintainer quote: Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.'
mx_operational_input_encode watcher "CURRENT_RECAP_WATCHER_BODY" CURRENT_WATCHER \
  || fail "could not construct current Recap watcher fixture"
QUOTED_CURRENT="Maintainer quote: $CURRENT_WATCHER"
ASCII_ONLY='MULTPLX_OP: v1 watcher: maintainer-authored text'

capture() {
  "$TMUX" -L "$SOCKET" capture-pane -p -t "$SESSION" -S -600 2>/dev/null || true
}

wait_for_text() {
  local expected=$1 attempts=${2:-120} i=0
  while [ "$i" -lt "$attempts" ]; do
    if capture | grep -Fq "$expected"; then
      return 0
    fi
    sleep 0.5
    i=$((i + 1))
  done
  capture >&2
  return 1
}

wait_for_exact_line() {
  local expected=$1 attempts=${2:-120} i=0
  while [ "$i" -lt "$attempts" ]; do
    if capture | grep -Fxq " $expected"; then
      return 0
    fi
    sleep 0.5
    i=$((i + 1))
  done
  capture >&2
  return 1
}

lab_pid_is_safe() {
  local pid=$1 command
  command=$(ps -p "$pid" -o command= 2>/dev/null || true)
  case "$command" in
    *"$LAB"*) return 0 ;;
    *) return 1 ;;
  esac
}

cleanup() {
  local pid_file watcher_pid arm_pid
  pid_file=$(find "$HOME_DIR/state" -maxdepth 3 -type f -name pid 2>/dev/null | head -1 || true)
  watcher_pid=
  arm_pid=
  if [ -n "$pid_file" ]; then
    watcher_pid=$(sed -n '1p' "$pid_file" 2>/dev/null || true)
    arm_pid=$(ps -p "$watcher_pid" -o ppid= 2>/dev/null | tr -d ' ' || true)
  fi
  "$TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  sleep 0.1
  if [ -n "$watcher_pid" ] && lab_pid_is_safe "$watcher_pid"; then
    kill -TERM "$watcher_pid" 2>/dev/null || true
  fi
  if [ -n "$arm_pid" ] && lab_pid_is_safe "$arm_pid"; then
    kill -TERM "$arm_pid" 2>/dev/null || true
  fi
  rm -rf "$LAB"
}
trap cleanup EXIT

send_prompt() {
  local prompt=$1
  "$TMUX" -L "$SOCKET" send-keys -t "$SESSION" -l "$prompt"
  "$TMUX" -L "$SOCKET" send-keys -t "$SESSION" Enter
}

wait_pid_dead() {
  local pid=$1 i=0
  while [ "$i" -lt 50 ]; do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.1
    i=$((i + 1))
  done
  return 1
}

run_recap_case() {
  local label=$1 preceding=$2 expected=$3 out status=0
  out=$(
    cd "$PROJECT" &&
      pi --print --approve --no-session --no-context-files --no-extensions \
        --no-skills --skill .agents/skills --tools read \
        --model openai-codex/gpt-5.6-sol --thinking low \
        "$preceding" "/recap"
  ) || status=$?
  [ "$status" -eq 0 ] || fail "Pi Recap $label case exited $status: $out"
  case "$expected" in
    catchup)
      printf '%s\n' "$out" | grep -Fq "RECAP_STATUS_BRANCH" \
        || fail "Pi Recap $label case did not take Catchup: $out"
      ;;
    boundary)
      printf '%s\n' "$out" | grep -Fq "RECAP_STATUS_BRANCH" \
        && fail "Pi Recap $label near miss was treated as operational: $out"
      ;;
  esac
}

run_recap_transcript_regressions() {
  mkdir -p "$PROJECT/.agents/skills/recap" "$PROJECT/.agents/skills/catchup"
  cp "$ROOT/.agents/skills/recap/SKILL.md" "$PROJECT/.agents/skills/recap/SKILL.md"
  # shellcheck disable=SC2016 # Backticks are literal prompt markup.
  printf '%s\n' \
    '---' \
    'name: catchup' \
    'description: Test-only Catchup branch sentinel.' \
    '---' \
    '' \
    '# catchup' \
    '' \
    'Respond exactly `RECAP_STATUS_BRANCH`.' \
    > "$PROJECT/.agents/skills/catchup/SKILL.md"

  run_recap_case legacy-start "$LEGACY_START" catchup
  run_recap_case legacy-away "$LEGACY_AWAY" catchup
  run_recap_case marker-near-miss "$MARKER_NEAR_MISS" boundary
  run_recap_case startup-near-miss "$START_NEAR_MISS" boundary
  run_recap_case quoted-current "$QUOTED_CURRENT" boundary
  run_recap_case ascii-only "$ASCII_ONLY" boundary
}

run_native_recap_regressions() {
  local first_home="$LAB/pi-recap-first-home"
  local later_home="$LAB/pi-recap-later-home"
  local first_out later_out

  mkdir -p \
    "$RECAP_PROJECT/.pi/extensions/lib" \
    "$RECAP_PROJECT/.agents/skills/recap" \
    "$RECAP_PROJECT/.agents/skills/catchup" \
    "$RECAP_PROJECT/bin" \
    "$first_home/state" "$first_home/config" \
    "$later_home/state" "$later_home/config"
  git init -q "$RECAP_PROJECT"
  cp "$ROOT/.pi/extensions/mx-primary-turnend-guard.ts" "$RECAP_PROJECT/.pi/extensions/"
  cp "$ROOT/.pi/extensions/lib/mx-operational-input.ts" "$RECAP_PROJECT/.pi/extensions/lib/"
  cp \
    "$ROOT/bin/mx-sessionstart-nudge.sh" \
    "$ROOT/bin/mx-primary-scope-lib.sh" \
    "$ROOT/bin/mx-gate-refuse-lib.sh" \
    "$ROOT/bin/mx-operational-input.sh" \
    "$RECAP_PROJECT/bin/"
  cp "$ROOT/.agents/skills/recap/SKILL.md" "$RECAP_PROJECT/.agents/skills/recap/SKILL.md"
  chmod +x "$RECAP_PROJECT/bin/mx-sessionstart-nudge.sh"
  # shellcheck disable=SC2016 # Variables expand in the generated script, not this test shell.
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'set -u' \
    'file="${MX_HOME:?}/state/session-start-count"' \
    'count=0' \
    '[ ! -f "$file" ] || count=$(sed -n "1p" "$file")' \
    'count=$((count + 1))' \
    'printf "%s\n" "$count" > "$file"' \
    'printf "SESSION_START_DONE count=%s\n" "$count"' \
    > "$RECAP_PROJECT/bin/mx-session-start.sh"
  chmod +x "$RECAP_PROJECT/bin/mx-session-start.sh"
  # shellcheck disable=SC2016 # Backticks are literal prompt markup.
  printf '%s\n' \
    '---' \
    'name: catchup' \
    'description: Test-only Catchup branch sentinel.' \
    '---' \
    '' \
    '# catchup' \
    '' \
    'Respond exactly `RECAP_STATUS_BRANCH`.' \
    > "$RECAP_PROJECT/.agents/skills/catchup/SKILL.md"
  # shellcheck disable=SC2016 # Backticks are literal prompt markup.
  printf '%s\n' \
    '# Native Pi Recap regression fixture' \
    '' \
    'Run `bin/mx-session-start.sh` exactly once at session start.' \
    > "$RECAP_PROJECT/AGENTS.md"

  first_out=$(
    cd "$RECAP_PROJECT" &&
      MX_HOME="$first_home" pi --print --approve --no-session --no-context-files --no-extensions \
        -e .pi/extensions/mx-primary-turnend-guard.ts \
        --no-skills --skill .agents/skills \
        --model openai-codex/gpt-5.6-sol --thinking low \
        "/recap"
  )
  printf '%s\n' "$first_out" | grep -Fq "RECAP_STATUS_BRANCH" \
    || fail "Pi native first-message Recap did not take Catchup: $first_out"
  [ "$(sed -n '1p' "$first_home/state/session-start-count")" = 1 ] \
    || fail "Pi native first-message Recap did not preserve one session-start execution"

  later_out=$(
    cd "$RECAP_PROJECT" &&
      MX_HOME="$later_home" pi --print --approve --no-session --no-context-files --no-extensions \
        -e .pi/extensions/mx-primary-turnend-guard.ts \
        --no-skills --skill .agents/skills \
        --model openai-codex/gpt-5.6-sol --thinking low \
        "Respond exactly PRIOR_BOUNDARY_ACK." "/recap"
  )
  printf '%s\n' "$later_out" | grep -Fq "PRIOR_BOUNDARY_ACK" \
    || fail "Pi native later-message setup did not preserve the genuine maintainer boundary: $later_out"
  printf '%s\n' "$later_out" | grep -Fq "RECAP_STATUS_BRANCH" \
    && fail "Pi native later-message Recap gathered Catchup: $later_out"
  [ "$(sed -n '1p' "$later_home/state/session-start-count")" = 1 ] \
    || fail "Pi native later-message Recap reran session start"
}

mkdir -p "$LAB"
git clone -q "$ROOT" "$PROJECT"
run_recap_transcript_regressions
run_native_recap_regressions
mkdir -p "$PROJECT/.pi/extensions/lib"
cp "$ROOT/.pi/extensions/mx-calm.ts" "$PROJECT/.pi/extensions/mx-calm.ts"
cp "$ROOT/.pi/extensions/mx-primary-pi-watch.ts" "$PROJECT/.pi/extensions/mx-primary-pi-watch.ts"
cp "$ROOT/.pi/extensions/lib/mx-calm-assistant-layout.ts" "$PROJECT/.pi/extensions/lib/mx-calm-assistant-layout.ts"
cp "$ROOT/.pi/extensions/lib/mx-calm-operational-user-layout.ts" "$PROJECT/.pi/extensions/lib/mx-calm-operational-user-layout.ts"
cp "$ROOT/.pi/extensions/lib/mx-calm-visibility.ts" "$PROJECT/.pi/extensions/lib/mx-calm-visibility.ts"
cp "$ROOT/.pi/extensions/lib/mx-operational-input.ts" "$PROJECT/.pi/extensions/lib/mx-operational-input.ts"
cp "$ROOT/.pi/extensions/mx-primary-turnend-guard.ts" "$PROJECT/.pi/extensions/mx-primary-turnend-guard.ts"
cp "$ROOT/bin/mx-watch-arm.sh" "$PROJECT/bin/mx-watch-arm.sh"
cp "$ROOT/bin/mx-operational-input.sh" "$PROJECT/bin/mx-operational-input.sh"
cp "$ROOT/bin/mx-supervision-instructions.sh" "$PROJECT/bin/mx-supervision-instructions.sh"
chmod +x "$PROJECT/bin/mx-operational-input.sh"
mkdir -p "$HOME_DIR/state" "$HOME_DIR/config"

"$TMUX" -L "$SOCKET" new-session -d -s "$SESSION" -c "$PROJECT" \
  "env MX_HOME='$HOME_DIR' MX_ROOT_OVERRIDE='$PROJECT' MX_POLL=1 MX_SIGNAL_GRACE=0 MX_HEARTBEAT=600 bash -lc 'printf \"%s\\n\" \"\$\$\" > \"\$MX_HOME/state/.lock\"; pi --approve --no-session --no-context-files --no-extensions -e .pi/extensions/mx-calm.ts -e .pi/extensions/mx-primary-turnend-guard.ts -e .pi/extensions/mx-primary-pi-watch.ts --model openai-codex/gpt-5.6-sol --thinking low; rc=\$?; printf \"PI_EXIT=%s\\n\" \"\$rc\"; sleep 300'"

i=0
while [ "$i" -lt 120 ]; do
  [ -f "$HOME_DIR/state/.pi-turnend-extension-loaded" ] && [ -f "$HOME_DIR/state/.pi-watch-extension-loaded" ] && break
  sleep 0.5
  i=$((i + 1))
done
[ -f "$HOME_DIR/state/.pi-turnend-extension-loaded" ] || fail "Pi turn-end extension did not load"
[ -f "$HOME_DIR/state/.pi-watch-extension-loaded" ] || fail "Pi watcher extension did not load"
wait_for_text "(openai-codex)" 120 || fail "Pi did not reach its ready composer"
sleep 1

send_prompt "/calm"
sleep 0.2
send_prompt "Reply exactly CALM_LIVE_WORKING_VISIBLE"
i=0
while [ "$i" -lt 240 ]; do
  pane=$(capture)
  if printf '%s\n' "$pane" | grep -Fq "Working..."; then
    break
  fi
  sleep 0.05
  i=$((i + 1))
done
printf '%s\n' "$pane" | grep -Fq "Working..." \
  || fail "Calm hid Pi's built-in Working row on the credentialed provider path"
wait_for_exact_line "CALM_LIVE_WORKING_VISIBLE" 120 \
  || fail "Pi did not settle the Calm Working-row provider probe"
pane=$(capture)
printf '%s\n' "$pane" | grep -Fq "calm transcript" \
  && fail "Calm added a persistent Calm status row on the credentialed provider path"
send_prompt "/calm"
sleep 0.2

: > "$HOME_DIR/state/pi-e2e.meta"
send_prompt "Start supervision with mx_watch_arm_pi and never use bash to arm supervision. After the watcher wake arrives, run bin/mx-wake-drain.sh and reply exactly HANDLED."
wait_for_text "watcher: started Pi extension arm child 1" || fail "Pi did not render the initial watcher tool result"

printf 'done: pi live e2e watcher fire\n' > "$HOME_DIR/state/pi-e2e.status"
i=0
while [ "$i" -lt 240 ]; do
  grep -Eq 'reason=actionable-signal.*successor=started:[0-9]+' "$HOME_DIR/state/.watch-cycle-exits.log" 2>/dev/null && break
  sleep 0.5
  i=$((i + 1))
done
grep -Eq 'reason=actionable-signal.*successor=started:[0-9]+' "$HOME_DIR/state/.watch-cycle-exits.log" 2>/dev/null \
  || fail "Pi extension did not start and ledger-link a successor after the actionable close"
wait_for_exact_line "HANDLED" 120 || fail "Pi did not drain and settle after its extension-owned successor started"

pane=$(capture)
guard_count=$(printf '%s\n' "$pane" | grep -Fc "TURN WOULD END BLIND - supervision is off." || true)
[ "$guard_count" -eq 0 ] || fail "successor was not protecting Pi before its next turn end (guard count $guard_count)"
foreground_arm='$ bin/mx-watch-arm.sh'
if printf '%s\n' "$pane" | grep -Fq "$foreground_arm"; then
  fail "Pi used a foreground bash watcher arm"
fi
arm_tool_result_count=$(printf '%s\n' "$pane" | grep -Ec 'watcher: (started|unchanged|not armed|read-only)' || true)
[ "$arm_tool_result_count" -eq 1 ] || fail "Pi model re-armed from memory instead of the extension (tool-result count $arm_tool_result_count)"

pid_file=$(find "$HOME_DIR/state" -maxdepth 3 -type f -name pid | head -1)
[ -n "$pid_file" ] || fail "re-armed watcher pid was not recorded"
watcher_pid=$(sed -n '1p' "$pid_file")
arm_pid=$(ps -p "$watcher_pid" -o ppid= | tr -d ' ')
[ -n "$arm_pid" ] || fail "re-armed watcher parent was not live"

"$TMUX" -L "$SOCKET" send-keys -t "$SESSION" -l '/quit'
sleep 1
"$TMUX" -L "$SOCKET" send-keys -t "$SESSION" Enter
wait_for_text "PI_EXIT=0" 60 || fail "Pi did not exit cleanly"
wait_pid_dead "$watcher_pid" || fail "watcher child survived clean Pi exit"
wait_pid_dead "$arm_pid" || fail "arm child survived clean Pi exit"

printf 'ok - Pi %s live E2E covered native Calm Working visibility, Recap first/later messages, legacy transcripts, near misses, and watcher continuity\n' "$PI_VERSION"
