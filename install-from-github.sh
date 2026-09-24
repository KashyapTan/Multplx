#!/usr/bin/env bash
# One-command source bootstrap for the public GitHub repository.
set -euo pipefail

usage() {
  cat <<'HELP'
Clone and install the latest public Multplx source release.

Usage:
  curl -fsSL https://raw.githubusercontent.com/KashyapTan/Multplx/main/install-from-github.sh | bash -s -- [install options]

Install options are forwarded to the cloned repository's ./install.sh, for
example --home PATH, --bin-dir PATH, --config-dir PATH, --data-dir PATH, or
--upgrade. This builds from source and requires Git, Cargo/Rust, and network
access. No system packages are installed automatically.

For a local clone, use ./install.sh directly.
HELP
}

for argument in "$@"; do
  case "$argument" in
    -h|--help) usage; exit 0 ;;
  esac
done

if ! command -v git >/dev/null 2>&1; then
  printf 'multplx: Git is required to clone the source. Install Git, then rerun this command.\n' >&2
  exit 2
fi

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/multplx-bootstrap.XXXXXX")
cleanup() { rm -rf -- "$work_dir"; }
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

checkout="$work_dir/source"
repository=${MULTPLX_REPOSITORY:-https://github.com/KashyapTan/Multplx.git}
printf 'Cloning Multplx source from %s\n' "$repository"
git clone --depth 1 --branch main "$repository" "$checkout"
if [ ! -x "$checkout/install.sh" ]; then
  printf 'multplx: cloned main branch does not contain executable install.sh; use a published Multplx revision.\n' >&2
  exit 2
fi
"$checkout/install.sh" "$@"
