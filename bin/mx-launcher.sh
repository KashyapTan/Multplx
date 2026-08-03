#!/usr/bin/env bash
# Activate or operate one globally configured Multplx control plane.
#
# Usage:
#   multplx [shell]
#   multplx [--backend auto|tmux|herdr|cmux] [shell]
#   multplx [--backend auto|tmux|herdr|cmux] claude|codex|pi [args...]
#   multplx doctor [args...]
#   multplx update
#   multplx paths
#   multplx --help
#   multplx --version
#
# Commands return the delegated program's status.  Launcher validation and
# usage failures return 2; a known competing live broker returns 3; a missing
# selected harness returns 127.  Bare invocation replaces this process with an
# interactive child shell in the caller's current directory.  The shell is not
# a broker and acquires no lock.  Harness shims change only their child process
# to the verified code root, then exec the previously captured real binary.
#
# --backend auto removes a session override so normal runtime detection owns
# selection.  The other values set MX_BACKEND only for the activated/direct
# child.  Ambient tmux, Herdr, cmux, terminal, locale, proxy, and authentication
# variables otherwise pass through unchanged.
set -u

SCRIPT_PATH=${BASH_SOURCE[0]}
case "$SCRIPT_PATH" in */*) ;; *) SCRIPT_PATH=$PWD/$SCRIPT_PATH ;; esac
SCRIPT_DIR=$(cd "${SCRIPT_PATH%/*}" && pwd -P)
MX_LAUNCHER_DEFAULT_ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
# shellcheck source=bin/mx-launcher-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-launcher-lib.sh"

usage() {
  sed -n '2,/^set -u$/s/^# \{0,1\}//p' "$0"
}

die() {
  mx_launcher_error "$*"
  exit 2
}

CONFIG_DIR=${MX_LAUNCH_CONFIG_DIR:-}
if [ "${1:-}" = --config-dir ]; then
  [ "$#" -ge 2 ] || die "--config-dir requires a path"
  CONFIG_DIR=$2
  shift 2
fi

BACKEND_SEEN=0
BACKEND_VALUE=
MX_LAUNCH_BACKEND_EXPLICIT=0
MX_LAUNCH_BACKEND_VALUE=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --backend)
      [ "$#" -ge 2 ] || die "--backend requires auto, tmux, herdr, or cmux"
      BACKEND_SEEN=1
      BACKEND_VALUE=$2
      shift 2
      ;;
    --backend=*)
      BACKEND_SEEN=1
      BACKEND_VALUE=${1#--backend=}
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --version)
      revision=$(git -C "$MX_LAUNCHER_DEFAULT_ROOT" describe --tags --always 2>/dev/null || printf unknown)
      printf 'multplx %s\n' "$revision"
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*) die "unknown option: $1" ;;
    *) break ;;
  esac
done

case "$BACKEND_VALUE" in
  ''|auto|tmux|herdr|cmux) ;;
  *) die "unsupported backend '$BACKEND_VALUE'; use auto, tmux, herdr, or cmux" ;;
esac

COMMAND=${1:-shell}
[ "$#" -eq 0 ] || shift

if [ -n "$CONFIG_DIR" ]; then
  CONFIG_DIR=$(mx_launcher_canonical_dir "$CONFIG_DIR" "launcher config") || exit 2
  mx_launcher_read_path_file "$CONFIG_DIR/root" configured_root || exit 2
  mx_launcher_read_path_file "$CONFIG_DIR/home" configured_home || exit 2
elif [ -n "${MX_ROOT_OVERRIDE:-}" ] || [ -n "${MX_HOME:-}" ]; then
  configured_root=${MX_ROOT_OVERRIDE:-$MX_LAUNCHER_DEFAULT_ROOT}
  configured_home=${MX_HOME:-$configured_root}
else
  default_config=${XDG_CONFIG_HOME:-${HOME:?HOME is not set}/.config}/multplx
  if [ -f "$default_config/root" ] && [ -f "$default_config/home" ]; then
    CONFIG_DIR=$(mx_launcher_canonical_dir "$default_config" "launcher config") || exit 2
    mx_launcher_read_path_file "$CONFIG_DIR/root" configured_root || exit 2
    mx_launcher_read_path_file "$CONFIG_DIR/home" configured_home || exit 2
  else
    configured_root=$MX_LAUNCHER_DEFAULT_ROOT
    configured_home=$MX_LAUNCHER_DEFAULT_ROOT
  fi
fi

MX_ROOT_OVERRIDE=$(mx_launcher_canonical_dir "$configured_root" "code root") || exit 2
MX_HOME=$(mx_launcher_canonical_dir "$configured_home" "operational home") || exit 2
export MX_ROOT_OVERRIDE MX_HOME
mx_launcher_validate_root "$MX_ROOT_OVERRIDE" || exit 2
mx_launcher_validate_home "$MX_HOME" || exit 2
if mx_launcher_runtime_is_managed "$MX_ROOT_OVERRIDE"; then
  mx_launcher_validate_managed_clean "$MX_ROOT_OVERRIDE" "$MX_HOME" || exit 2
fi
MX_LAUNCH_VALIDATED=1
export MX_LAUNCH_VALIDATED

MX_SHIM_DIR=$MX_ROOT_OVERRIDE/share/shell/shims
[ -d "$MX_SHIM_DIR" ] || die "code root is missing harness shims: $MX_SHIM_DIR"
export MX_SHIM_DIR

if [ "$BACKEND_SEEN" -eq 1 ]; then
  MX_LAUNCH_BACKEND_EXPLICIT=1
  MX_LAUNCH_BACKEND_VALUE=$BACKEND_VALUE
  if [ "$BACKEND_VALUE" = auto ]; then
    unset MX_BACKEND
  else
    MX_BACKEND=$BACKEND_VALUE
    export MX_BACKEND
  fi
fi

capture_harnesses() {
  if [ "${MULTPLX_ACTIVE:-}" = 1 ]; then
    MX_REAL_CLAUDE=${MX_REAL_CLAUDE:-}
    MX_REAL_CODEX=${MX_REAL_CODEX:-}
    MX_REAL_PI=${MX_REAL_PI:-}
  else
    MX_REAL_CLAUDE=$(mx_launcher_find_executable claude "$MX_SHIM_DIR" || true)
    MX_REAL_CODEX=$(mx_launcher_find_executable codex "$MX_SHIM_DIR" || true)
    MX_REAL_PI=$(mx_launcher_find_executable pi "$MX_SHIM_DIR" || true)
  fi
  export MX_REAL_CLAUDE MX_REAL_CODEX MX_REAL_PI
}

launch_direct() {
  capture_harnesses
  exec "$MX_ROOT_OVERRIDE/bin/mx-launch-harness.sh" "$COMMAND" "$@"
}

case "$COMMAND" in
  paths)
    [ "$#" -eq 0 ] || die "paths accepts no arguments"
    printf 'root=%s\nhome=%s\nbin=%s\nconfig=%s\n' \
      "$MX_ROOT_OVERRIDE" "$MX_HOME" \
      "${MX_LAUNCH_BIN_PATH:-${XDG_BIN_HOME:-${HOME:?HOME is not set}/.local/bin}/multplx}" \
      "${CONFIG_DIR:-unregistered}"
    ;;
  doctor)
    exec "$MX_ROOT_OVERRIDE/bin/mx-doctor.sh" "$@"
    ;;
  update)
    [ "$#" -eq 0 ] || die "update accepts no arguments"
    exec "$MX_ROOT_OVERRIDE/bin/mx-update.sh"
    ;;
  claude|codex|pi)
    launch_direct "$@"
    ;;
  shell)
    [ "$#" -eq 0 ] || die "shell accepts no arguments"
    [ "${MULTPLX_ACTIVE:-}" != 1 ] || die "a Multplx shell is already active; exit it before activating another"
    capture_harnesses
    MULTPLX_ACTIVE=1
    export MULTPLX_ACTIVE
    mx_launcher_prepend_path_once "$MX_SHIM_DIR"
    shell_path=${MX_LAUNCH_SHELL:-${SHELL:-}}
    [ -n "$shell_path" ] || die "SHELL is not set; choose Bash or Zsh with MX_LAUNCH_SHELL"
    case "$shell_path" in
      /*) ;;
      *) die "interactive shell path must be absolute: $shell_path" ;;
    esac
    [ -f "$shell_path" ] && [ -x "$shell_path" ] || die "interactive shell is not executable: $shell_path"
    shell_name=${shell_path##*/}
    export MX_LAUNCH_BACKEND_EXPLICIT MX_LAUNCH_BACKEND_VALUE
    case "$shell_name" in
      bash)
        exec "$shell_path" --rcfile "$MX_ROOT_OVERRIDE/share/shell/multplx.bash" -i
        ;;
      zsh)
        adapter_dir=$(mktemp -d "${TMPDIR:-/tmp}/multplx-zsh.XXXXXX") || die "could not create Zsh adapter directory"
        chmod 700 "$adapter_dir" || { rmdir "$adapter_dir" 2>/dev/null || true; die "could not secure Zsh adapter directory"; }
        cp "$MX_ROOT_OVERRIDE/share/shell/multplx.zsh" "$adapter_dir/.zshrc" || {
          rmdir "$adapter_dir" 2>/dev/null || true
          die "could not prepare Zsh adapter"
        }
        chmod 600 "$adapter_dir/.zshrc" || {
          rm -f "$adapter_dir/.zshrc"
          rmdir "$adapter_dir" 2>/dev/null || true
          die "could not secure Zsh adapter"
        }
        if [ "${ZDOTDIR+x}" = x ]; then
          MX_ORIGINAL_ZDOTDIR_SET=1
          MX_ORIGINAL_ZDOTDIR=$ZDOTDIR
        else
          MX_ORIGINAL_ZDOTDIR_SET=0
          MX_ORIGINAL_ZDOTDIR=
        fi
        MX_ZSH_ADAPTER_DIR=$adapter_dir
        ZDOTDIR=$adapter_dir
        export MX_ORIGINAL_ZDOTDIR_SET MX_ORIGINAL_ZDOTDIR MX_ZSH_ADAPTER_DIR ZDOTDIR
        exec "$shell_path" -i
        ;;
      sh|dash|ksh|ksh93)
        printf 'multplx: activated (prompt integration is available for Bash and Zsh)\n' >&2
        exec "$shell_path" -i
        ;;
      *)
        die "unsupported interactive shell '$shell_name'; use Bash or Zsh"
        ;;
    esac
    ;;
  *)
    die "unknown command '$COMMAND'; run multplx --help"
    ;;
esac
