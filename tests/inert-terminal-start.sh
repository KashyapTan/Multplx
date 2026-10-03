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
stub="$(dirname "$script")/$harness"
printf 'import time\ntime.sleep(2)\n' > "$stub"
launch="$(dirname "$script")/inert-launch.sh"
sed '$d' "$script" > "$launch"
printf 'exec python3 "%s"\n' "$stub" >> "$launch"
/bin/sh "$launch" >/dev/null 2>&1 </dev/null &
