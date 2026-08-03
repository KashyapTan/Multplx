#!/usr/bin/env bash
# Launch one verified primary harness from the Multplx code root.
# Usage: mx-launch-harness.sh claude|codex|pi [arguments...]
# Exit 2 means launcher validation failed, 3 means another live broker owns the
# home, and 127 means the selected real harness was not captured or disappeared.
set -u

SCRIPT_PATH=${BASH_SOURCE[0]}
case "$SCRIPT_PATH" in */*) ;; *) SCRIPT_PATH=$PWD/$SCRIPT_PATH ;; esac
SCRIPT_DIR=$(cd "${SCRIPT_PATH%/*}" && pwd -P)
# shellcheck source=bin/mx-launcher-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-launcher-lib.sh"

harness=${1:-}
[ "$#" -eq 0 ] || shift
case "$harness" in
  claude) real=${MX_REAL_CLAUDE:-} ;;
  codex) real=${MX_REAL_CODEX:-} ;;
  pi) real=${MX_REAL_PI:-} ;;
  *) mx_launcher_error "harness must be claude, codex, or pi"; exit 2 ;;
esac

root=${MX_ROOT_OVERRIDE:-}
home=${MX_HOME:-}
[ -n "$root" ] && [ -n "$home" ] || {
  mx_launcher_error "harness launch requires MX_ROOT_OVERRIDE and MX_HOME from the launcher"
  exit 2
}
if [ "${MX_LAUNCH_VALIDATED:-}" = 1 ]; then
  case "$root:$home" in /*:/*) ;; *) mx_launcher_error "validated launcher paths must be absolute"; exit 2 ;; esac
  [ -f "$root/AGENTS.md" ] && [ -x "$root/bin/mx-lock.sh" ] && [ -d "$home/state" ] || {
    mx_launcher_error "validated launcher root or home disappeared before harness start"
    exit 2
  }
else
  root=$(mx_launcher_canonical_dir "$root" "code root") || exit 2
  home=$(mx_launcher_canonical_dir "$home" "operational home") || exit 2
  mx_launcher_validate_root "$root" || exit 2
  mx_launcher_validate_home "$home" || exit 2
fi

if [ -z "$real" ] || [ "${real#/}" = "$real" ] || [ ! -f "$real" ] || [ ! -x "$real" ]; then
  mx_launcher_error "$harness is not installed or its captured executable is no longer available"
  exit 127
fi
shim=$root/share/shell/shims/$harness
if [ -e "$shim" ] && [ "$real" -ef "$shim" ]; then
  mx_launcher_error "refusing recursive $harness shim resolution"
  exit 127
fi

lock_status=$(MX_ROOT_OVERRIDE="$root" MX_HOME="$home" "$root/bin/mx-lock.sh" status 2>&1) || {
  mx_launcher_error "could not inspect the broker session lock"
  exit 2
}
case "$lock_status" in
  'lock: held by live harness pid '*)
    holder=${lock_status##* }
    mx_launcher_error "another live broker already owns this home (pid $holder)"
    exit 3
    ;;
esac

MX_ROOT_OVERRIDE=$root
MX_HOME=$home
export MX_ROOT_OVERRIDE MX_HOME
cd -P "$root" || {
  mx_launcher_error "cannot enter code root: $root"
  exit 2
}
exec "$real" "$@"
