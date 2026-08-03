#!/usr/bin/env bash
# Install the global `multplx` bootstrap and register one code root and home.
#
# Existing-checkout mode (default):
#   mx-launcher-install.sh [--root PATH] [--home PATH]
#
# Managed mode:
#   mx-launcher-install.sh --managed [--source GIT-URL]
#
# Shared options:
#   --bin-dir PATH       default ${XDG_BIN_HOME:-$HOME/.local/bin}
#   --config-dir PATH    default ${XDG_CONFIG_HOME:-$HOME/.config}/multplx
#   --data-dir PATH      default ${XDG_DATA_HOME:-$HOME/.local/share}/multplx
#   --uninstall          remove only the owned bootstrap and root/home records
#   -h, --help
#
# Existing mode records the selected plain checkout and uses it as MX_HOME by
# default.  It never moves or hashes private data.  Managed mode atomically
# clones a clean plain runtime under DATA_DIR/runtime and creates a separate
# DATA_DIR/home.  Repeating a compatible installation is idempotent.  Existing
# symlinks, unrelated bootstrap files, or conflicting root/home records refuse
# without overwrite.  Uninstall never removes either runtime or operational
# data; those remain explicit user-owned cleanup choices.
#
# Exit 0 means installed/already installed/uninstalled, 2 means validation or
# usage failure, and 1 means a filesystem or git operation failed.
set -u

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
DEFAULT_ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
# shellcheck source=bin/mx-launcher-lib.sh disable=SC1091
. "$SCRIPT_DIR/mx-launcher-lib.sh"

usage() {
  sed -n '2,/^set -u$/s/^# \{0,1\}//p' "$0"
}

die() {
  mx_launcher_error "$*"
  exit 2
}

MODE=existing
ROOT_ARG=
HOME_ARG=
SOURCE_ARG=
BIN_DIR=${XDG_BIN_HOME:-${HOME:?HOME is not set}/.local/bin}
CONFIG_DIR=${XDG_CONFIG_HOME:-${HOME:?HOME is not set}/.config}/multplx
DATA_DIR=${XDG_DATA_HOME:-${HOME:?HOME is not set}/.local/share}/multplx
UNINSTALL=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --managed) MODE=managed ;;
    --root) shift; [ "$#" -gt 0 ] || die "--root requires a path"; ROOT_ARG=$1 ;;
    --home) shift; [ "$#" -gt 0 ] || die "--home requires a path"; HOME_ARG=$1 ;;
    --source) shift; [ "$#" -gt 0 ] || die "--source requires a git URL or path"; SOURCE_ARG=$1 ;;
    --bin-dir) shift; [ "$#" -gt 0 ] || die "--bin-dir requires a path"; BIN_DIR=$1 ;;
    --config-dir) shift; [ "$#" -gt 0 ] || die "--config-dir requires a path"; CONFIG_DIR=$1 ;;
    --data-dir) shift; [ "$#" -gt 0 ] || die "--data-dir requires a path"; DATA_DIR=$1 ;;
    --uninstall) UNINSTALL=1 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
  shift
done

case "$BIN_DIR:$CONFIG_DIR:$DATA_DIR" in
  /*:/*:/*) ;;
  *) die "bin, config, and data directories must be absolute paths" ;;
esac

ensure_dir() {
  local path=$1 private=${2:-0}
  if [ -L "$path" ] || { [ -e "$path" ] && [ ! -d "$path" ]; }; then
    die "refusing linked or non-directory installation path: $path"
  fi
  mkdir -p "$path" || {
    mx_launcher_error "could not create directory: $path"
    exit 1
  }
  if [ "$private" -eq 1 ]; then
    chmod 700 "$path" || {
      mx_launcher_error "could not secure directory: $path"
      exit 1
    }
  fi
}

path_uid() {
  if [ "$(uname)" = Darwin ]; then
    stat -f %u "$1" 2>/dev/null
  else
    stat -c %u "$1" 2>/dev/null
  fi
}

require_owned_dir() {
  local path=$1 label=$2 owner current
  owner=$(path_uid "$path") || die "cannot inspect ownership of $label: $path"
  current=$(id -u) || die "cannot determine installer user identity"
  [ "$owner" = "$current" ] || die "$label must be owned by uid $current: $path"
}

ensure_dir "$BIN_DIR"
ensure_dir "$CONFIG_DIR" 1
ensure_dir "$DATA_DIR" 1
BIN_DIR=$(mx_launcher_canonical_dir "$BIN_DIR" "binary installation") || exit 2
CONFIG_DIR=$(mx_launcher_canonical_dir "$CONFIG_DIR" "launcher config") || exit 2
DATA_DIR=$(mx_launcher_canonical_dir "$DATA_DIR" "launcher data") || exit 2
require_owned_dir "$BIN_DIR" "binary installation directory"
require_owned_dir "$CONFIG_DIR" "launcher config directory"
require_owned_dir "$DATA_DIR" "launcher data directory"
BOOTSTRAP=$BIN_DIR/multplx

shell_quote() {
  local value=$1
  value=${value//\'/\'\\\'\'}
  printf "'%s'" "$value"
}

write_bootstrap_payload() {
  printf '%s\n' '#!/usr/bin/env bash' 'set -u'
  printf 'CONFIG_DIR=%s\n' "$(shell_quote "$CONFIG_DIR")"
  cat <<'SH'
fail() { printf 'multplx: %s\n' "$*" >&2; exit 2; }
read_path() {
  local LC_ALL=C file=$1 value bytes
  [ ! -L "$file" ] && [ -f "$file" ] || fail "invalid path file: $file"
  bytes=$(LC_ALL=C wc -c <"$file" 2>/dev/null) || fail "cannot read path file: $file"
  bytes=${bytes//[[:space:]]/}
  LC_ALL=C IFS= read -r value <"$file" || fail "invalid path file: $file"
  [ "$bytes" -eq "$(( ${#value} + 1 ))" ] || fail "invalid path file: $file"
  case "$value" in /*) ;; *) fail "path is not absolute in $file" ;; esac
  printf '%s\n' "$value"
}
root=$(read_path "$CONFIG_DIR/root") || exit 2
[ -x "$root/bin/mx-launcher.sh" ] || fail "configured launcher is missing: $root/bin/mx-launcher.sh"
export MX_LAUNCH_CONFIG_DIR="$CONFIG_DIR"
export MX_LAUNCH_BIN_PATH="$0"
exec "$root/bin/mx-launcher.sh" "$@"
SH
}

payload_file=$(mktemp "${TMPDIR:-/tmp}/multplx-bootstrap.XXXXXX") || {
  mx_launcher_error "could not create bootstrap buffer"
  exit 1
}
root_payload=
home_payload=
clone_tmp=
cleanup_payload() {
  rm -f "$payload_file" ${root_payload:+"$root_payload"} ${home_payload:+"$home_payload"}
  [ -z "$clone_tmp" ] || rm -rf "$clone_tmp"
}
trap cleanup_payload EXIT
trap 'exit 130' HUP INT TERM
write_bootstrap_payload >"$payload_file" || exit 1
chmod 755 "$payload_file" || exit 1

if [ "$UNINSTALL" -eq 1 ]; then
  [ "$MODE" = existing ] && [ -z "$ROOT_ARG$HOME_ARG$SOURCE_ARG" ] \
    || die "--uninstall cannot be combined with root, home, managed, or source options"
  if [ -L "$BOOTSTRAP" ]; then
    die "refusing to remove linked bootstrap: $BOOTSTRAP"
  fi
  if [ -e "$BOOTSTRAP" ] && ! cmp -s "$BOOTSTRAP" "$payload_file"; then
    die "refusing to remove an unrecognized bootstrap: $BOOTSTRAP"
  fi
  for path_file in "$CONFIG_DIR/root" "$CONFIG_DIR/home"; do
    [ ! -L "$path_file" ] || die "refusing to remove linked path record: $path_file"
    [ ! -e "$path_file" ] || rm -f "$path_file" || exit 1
  done
  [ ! -e "$BOOTSTRAP" ] || rm -f "$BOOTSTRAP" || exit 1
  printf 'multplx: launcher removed; runtime and operational data preserved\n'
  exit 0
fi

atomic_publish() {
  local target=$1 mode=$2 source=$3 name temp
  name=${target##*/}
  if [ -L "$target" ] || { [ -e "$target" ] && [ ! -f "$target" ]; }; then
    die "refusing linked or non-regular installation target: $target"
  fi
  if [ -f "$target" ]; then
    if cmp -s "$target" "$source"; then
      return 0
    fi
    die "refusing to overwrite incompatible installation target: $target"
  fi
  temp=$(mktemp "${target%/*}/.$name.tmp.XXXXXX") || {
    mx_launcher_error "could not create atomic temporary file for $target"
    exit 1
  }
  if ! cp "$source" "$temp" || ! chmod "$mode" "$temp"; then
    rm -f "$temp"
    mx_launcher_error "could not prepare installation target: $target"
    exit 1
  fi
  if [ "${MX_LAUNCHER_INSTALL_FAIL_BEFORE:-}" = "$name" ]; then
    rm -f "$temp"
    mx_launcher_error "injected interruption before publishing $name"
    exit 1
  fi
  mv "$temp" "$target" || {
    rm -f "$temp"
    mx_launcher_error "could not publish installation target: $target"
    exit 1
  }
}

canonical_existing_dir() {
  local path=$1 label=$2
  case "$path" in /*) ;; *) path=$PWD/$path ;; esac
  mx_launcher_canonical_dir "$path" "$label"
}

if [ "$MODE" = managed ]; then
  [ -z "$ROOT_ARG$HOME_ARG" ] || die "--managed cannot be combined with --root or --home"
  runtime=$DATA_DIR/runtime
  home=$DATA_DIR/home
  if [ -z "$SOURCE_ARG" ]; then
    SOURCE_ARG=$(git -C "$DEFAULT_ROOT" remote get-url origin 2>/dev/null || true)
    [ -n "$SOURCE_ARG" ] || SOURCE_ARG=https://github.com/KashyapTan/Multplx.git
  fi
  if [ -L "$runtime" ] || { [ -e "$runtime" ] && [ ! -d "$runtime" ]; }; then
    die "refusing linked or non-directory managed runtime: $runtime"
  fi
  if [ ! -e "$runtime" ]; then
    clone_tmp=$(mktemp -d "$DATA_DIR/.runtime.clone.XXXXXX") || exit 1
    rmdir "$clone_tmp" || exit 1
    if ! git clone --quiet -- "$SOURCE_ARG" "$clone_tmp"; then
      rm -rf "$clone_tmp"
      mx_launcher_error "managed runtime clone failed"
      exit 1
    fi
    clone_tmp=$(mx_launcher_canonical_dir "$clone_tmp" "managed runtime candidate") || exit 2
    chmod 700 "$clone_tmp" || { mx_launcher_error "could not secure managed runtime candidate"; exit 1; }
    mx_launcher_validate_root "$clone_tmp" || {
      rm -rf "$clone_tmp"
      mx_launcher_error "managed source did not produce a valid Multplx runtime"
      exit 2
    }
    git -C "$clone_tmp" config --local multplx.managed true || {
      rm -rf "$clone_tmp"
      mx_launcher_error "could not mark managed runtime ownership"
      exit 1
    }
    mx_launcher_validate_managed_clean "$clone_tmp" "$DATA_DIR/home" || {
      rm -rf "$clone_tmp"
      exit 2
    }
    if [ "${MX_LAUNCHER_INSTALL_FAIL_BEFORE:-}" = runtime ]; then
      rm -rf "$clone_tmp"
      mx_launcher_error "injected interruption before publishing runtime"
      exit 1
    fi
    mv "$clone_tmp" "$runtime" || exit 1
    clone_tmp=
  fi
  root=$(canonical_existing_dir "$runtime" "managed runtime") || exit 2
  mx_launcher_runtime_is_managed "$root" \
    || die "existing runtime was not created by the managed launcher installer: $root"
  chmod 700 "$root" || { mx_launcher_error "could not secure managed runtime: $root"; exit 1; }
  ensure_dir "$home" 1
  home=$(canonical_existing_dir "$home" "operational home") || exit 2
else
  [ -z "$SOURCE_ARG" ] || die "--source requires --managed"
  root=$(canonical_existing_dir "${ROOT_ARG:-$DEFAULT_ROOT}" "code root") || exit 2
  if [ -n "$HOME_ARG" ]; then
    home_path=$HOME_ARG
    case "$home_path" in /*) ;; *) home_path=$PWD/$home_path ;; esac
    ensure_dir "$home_path" 1
    home=$(canonical_existing_dir "$home_path" "operational home") || exit 2
  else
    home=$root
  fi
fi

require_owned_dir "$root" "code root"
require_owned_dir "$home" "operational home"

for private_part in config data projects state; do
  ensure_dir "$home/$private_part" 1
done
mx_launcher_validate_root "$root" || exit 2
mx_launcher_validate_home "$home" || exit 2
if [ "$MODE" = managed ]; then
  mx_launcher_validate_managed_clean "$root" "$home" || exit 2
fi

root_payload=$(mktemp "${TMPDIR:-/tmp}/multplx-root.XXXXXX") || exit 1
home_payload=$(mktemp "${TMPDIR:-/tmp}/multplx-home.XXXXXX") || exit 1
printf '%s\n' "$root" >"$root_payload"
printf '%s\n' "$home" >"$home_payload"
chmod 600 "$root_payload" "$home_payload" || exit 1

atomic_publish "$CONFIG_DIR/root" 600 "$root_payload"
atomic_publish "$CONFIG_DIR/home" 600 "$home_payload"
atomic_publish "$BOOTSTRAP" 755 "$payload_file"
rm -f "$root_payload" "$home_payload"
root_payload=
home_payload=

printf 'multplx: installed %s\n' "$BOOTSTRAP"
printf 'multplx: root %s\n' "$root"
printf 'multplx: home %s\n' "$home"
case :$PATH: in
  *:"$BIN_DIR":*) ;;
  *) printf 'multplx: add %s to PATH\n' "$BIN_DIR" ;;
esac
