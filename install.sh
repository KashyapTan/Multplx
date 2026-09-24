#!/usr/bin/env bash
# Build and install a matching Multplx binary and runtime from this checkout.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(cd -- "$SCRIPT_DIR" && pwd -P)

usage() {
  cat <<'HELP'
Build and install Multplx from this checkout.

Usage:
  ./install.sh [--upgrade] [--home PATH] [--bin-dir PATH]
               [--config-dir PATH] [--data-dir PATH]

This builds the locked Rust workspace, packages the matching runtime assets,
and installs the package through Multplx's transactional global installer.
The default operational home is separate from the installed runtime.
Pass --upgrade to replace an existing installation. The supported forwarded
options are --home, --bin-dir, --config-dir and --data-dir. This script does
not install Rust or system tools.

After installation, ensure the binary directory is on PATH, then run:
  multplx
  multplx --help
HELP
}

for argument in "$@"; do
  case "$argument" in
    -h|--help) usage; exit 0 ;;
  esac
done

install_args=("$@")
install_bin_dir=${XDG_BIN_HOME:-"$HOME/.local/bin"}
index=0
while [ "$index" -lt "${#install_args[@]}" ]; do
  case "${install_args[$index]}" in
    --upgrade)
      index=$((index + 1))
      ;;
    --home|--bin-dir|--config-dir|--data-dir)
      if [ "$((index + 1))" -ge "${#install_args[@]}" ]; then
        printf 'multplx: %s requires a path\n' "${install_args[$index]}" >&2
        exit 2
      fi
      if [ "${install_args[$index]}" = --bin-dir ]; then
        install_bin_dir=${install_args[$((index + 1))]}
      fi
      index=$((index + 2))
      ;;
    *)
      printf 'multplx: unsupported install option: %s\n' "${install_args[$index]}" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! command -v cargo >/dev/null 2>&1; then
  printf 'multplx: Cargo is required to install from a source checkout. Install Rust with rustup (https://rustup.rs), then rerun ./install.sh.\n' >&2
  exit 2
fi
if ! command -v git >/dev/null 2>&1; then
  printf 'multplx: Git is required to build the verified runtime package. Install Git, then rerun ./install.sh.\n' >&2
  exit 2
fi

printf 'Building the locked Multplx release from %s\n' "$ROOT"
target_dir=${CARGO_TARGET_DIR:-$ROOT/target}
case "$target_dir" in
  /*) ;;
  *) target_dir="$ROOT/$target_dir" ;;
esac
(cd -- "$ROOT" && cargo build --release --workspace --locked --target-dir "$target_dir")

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/multplx-install.XXXXXX")
cleanup() { rm -rf -- "$work_dir"; }
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

package="$work_dir/package"
"$ROOT/bin/mx-release-package.sh" "$package" "$target_dir/release/mx" >/dev/null
"$target_dir/release/mx" launcher-install --package "$package" "$@"

printf '\nMultplx is installed. Ensure %s is on PATH, then run `multplx` to start.\n' \
  "$install_bin_dir"
