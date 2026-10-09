#!/usr/bin/env bash
# Fixture-only terminal execution: run the real receipt preamble, then an inert
# named interpreter harness. Captured provider command construction stays intact.
set -eu
trace_inert_stage() {
  if [ -n "${MX_FAKE_SEND_LOG:-}" ]; then
    printf '%s\t%s\tinert-stage\t%s\n' "$$" "$SECONDS" "$1" >> "$MX_FAKE_SEND_LOG.commands"
  fi
}
trace_inert_stage entry
script=${1#\'}
script=${script%\'}
[ -f "$script" ] || exit 0
trace_inert_stage tail-begin
case "$(tail -n 1 "$script")" in
  *' hooks.'*|*' MX_CODEX_IDLE_CLI='*) harness=codex ;;
  *CLAUDE_CODE_ENABLE*) harness=claude ;;
  *--plugin-dir*) harness=agent ;;
  *) harness=pi ;;
esac
trace_inert_stage tail-end
# Keep the synthetic interpreter script name independently readable by ps, even
# when the owned launch script lives in a spaced task root. It removes its own
# private directory immediately after Python opens it.
trace_inert_stage mktemp-begin
inert_dir=$(mktemp -d /tmp/mx-inert-provider.XXXXXX)
trace_inert_stage mktemp-end
stub="$inert_dir/$harness"
printf 'import os,time\nos.unlink(__file__)\nos.rmdir(os.path.dirname(__file__))\ntime.sleep(2)\n' > "$stub"
trace_inert_stage dirname-begin
launch="$(dirname "$script")/inert-launch.sh"
trace_inert_stage dirname-end
trace_inert_stage sed-begin
sed '$d' "$script" > "$launch"
trace_inert_stage sed-end
printf 'exec -a %s python3 "%s"\n' "$harness" "$stub" >> "$launch"
trace_inert_stage launch-begin
bash "$launch" >/dev/null 2>&1 </dev/null &
trace_inert_stage launch-detached
if [ -n "${MX_TEST_INERT_PID_LOG:-}" ]; then
  printf '%s\n' "$!" >> "$MX_TEST_INERT_PID_LOG"
fi
