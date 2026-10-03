#!/usr/bin/env bash
# Fixture-only terminal execution: run the real receipt preamble, then an inert
# named interpreter harness. Captured provider command construction stays intact.
set -eu
script=${1#\'}
script=${script%\'}
[ -f "$script" ] || exit 0
case "$(tail -n 1 "$script")" in
  *' hooks.'*|*' MX_CODEX_IDLE_CLI='*) harness=codex ;;
  *CLAUDE_CODE_ENABLE*) harness=claude ;;
  *--plugin-dir*) harness=agent ;;
  *) harness=pi ;;
esac
# Keep the synthetic interpreter script name independently readable by ps, even
# when the owned launch script lives in a spaced task root. It removes its own
# private directory immediately after Python opens it.
inert_dir=$(mktemp -d /tmp/mx-inert-provider.XXXXXX)
stub="$inert_dir/$harness"
printf 'import os,time\nos.unlink(__file__)\nos.rmdir(os.path.dirname(__file__))\ntime.sleep(2)\n' > "$stub"
launch="$(dirname "$script")/inert-launch.sh"
sed '$d' "$script" > "$launch"
printf 'exec -a %s python3 "%s"\n' "$harness" "$stub" >> "$launch"
bash "$launch" >/dev/null 2>&1 </dev/null &
if [ -n "${MX_TEST_INERT_PID_LOG:-}" ]; then
  printf '%s\n' "$!" >> "$MX_TEST_INERT_PID_LOG"
fi
