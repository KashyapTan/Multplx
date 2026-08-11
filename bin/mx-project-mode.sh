#!/usr/bin/env bash
# Resolve a project's delivery mode and yolo flag from the data/projects.md registry.
# Prints two words to stdout: "<mode> <yolo>" where mode is one of
# deep-review|direct-PR|local-only and yolo is on|off.
#
# Registry line format (data/projects.md):
#   - <name> - <desc> (added <date>)                  -> deep-review off
#   - <name> [<mode>] - <desc> (added <date>)          -> <mode> off
#   - <name> [<mode> +yolo] - <desc> (added <date>)    -> <mode> on
#
# mode = how a finished change reaches main:
#   deep-review  local validation -> delivery service -> PR -> maintainer merge (default)
#   direct-PR    push + PR via official gh, no pipeline -> maintainer merge
#   local-only   local branch, no remote/PR -> maintainer approve -> guarded local merge
# yolo (orthogonal) = when on, broker may make routine approval decisions itself.
#   AGENTS.md section 7 is the single owner of authority exceptions, including
#   ask-user contract expansion and stronger maintainer boundaries.
#
# An unknown/missing project or unknown mode falls back to "deep-review off" and warns
# to stderr, so a typo never silently drops the gate.
# Usage: mx-project-mode.sh <project-name>
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=bin/mx-rust-runtime.sh
. "$SCRIPT_DIR/mx-rust-runtime.sh"
NAME=${1:?usage: mx-project-mode.sh <project-name>}
implementation=$(mx_local_state_implementation) || exit $?
if [ "$implementation" = rust ]; then
  rust_bin=$(mx_rust_runtime_bin) || exit $?
  exec "$rust_bin" project-mode "$NAME"
fi
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
REG="$DATA/projects.md"
if [ ! -f "$REG" ]; then
  echo "warn: no registry at $REG; defaulting $NAME to deep-review off" >&2
  echo "deep-review off"
  exit 0
fi

# awk emits "<mode> <yolo>" (one line) or nothing if the project is absent.
parsed=$(awk -v n="$NAME" '
  $1=="-" && $2==n {
    mode="deep-review"; yolo="off";
    if ($3 ~ /^\[/) {
      s="";
      for (i=3; i<=NF; i++) { s = s (s==""?"":" ") $i; if ($i ~ /\]$/) break }
      gsub(/^\[|\]$/, "", s);           # strip the surrounding brackets
      k = split(s, a, " ");
      if (a[1] != "" && a[1] != "+yolo") mode = a[1];
      for (j=1; j<=k; j++) if (a[j]=="+yolo") yolo="on";
    }
    print mode, yolo; exit
  }
' "$REG")

if [ -z "$parsed" ]; then
  echo "warn: project \"$NAME\" not in registry; defaulting to deep-review off" >&2
  echo "deep-review off"
  exit 0
fi

mode=${parsed%% *}
yolo=${parsed##* }
case "$mode" in
  deep-review|direct-PR|local-only) ;;
  *) echo "warn: unknown mode \"$mode\" for $NAME; defaulting to deep-review off" >&2; mode=deep-review; yolo=off ;;
esac
case "$yolo" in on|off) ;; *) yolo=off ;; esac
echo "$mode $yolo"
