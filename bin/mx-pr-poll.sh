#!/usr/bin/env bash
# Static watcher program for a validated GitHub PR poll sidecar.
# It emits exactly one merged line for a merged PR and stays silent otherwise,
# including on every error, so a failed lookup can never be read as a merge.
# The provider-tagged identity is data in the sidecar and is never
# interpolated into this source: these bytes are identical for every task.
# GitHub is the only supported provider and is read through its standard CLI,
# gh, so an upstream checkout needs no extra tooling to follow a PR.
set -u
LC_ALL=C
export LC_ALL

# Portion 11 Rust-default static poll adapter.  The repo copy self-locates,
# while a published state copy uses the established home/root variables and
# passes its invoked path as inert data.
MX_POLL_SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
if [ -f "$MX_POLL_SCRIPT_DIR/mx-rust-runtime.sh" ]; then
  MX_POLL_ROOT=$(CDPATH='' cd -- "$MX_POLL_SCRIPT_DIR/.." && pwd -P)
else
  MX_POLL_ROOT=${MX_ROOT_OVERRIDE:-${MX_HOME:-}}
fi
if [ -n "$MX_POLL_ROOT" ] && [ -f "$MX_POLL_ROOT/bin/mx-rust-runtime.sh" ]; then
  # shellcheck source=bin/mx-rust-runtime.sh
  . "$MX_POLL_ROOT/bin/mx-rust-runtime.sh"
  implementation=$(mx_review_delivery_implementation) || exit $?
  if [ "$implementation" = rust ]; then
    MX_RUST_SOURCE_ROOT=${MX_RUST_SOURCE_ROOT:-$MX_POLL_ROOT}; export MX_RUST_SOURCE_ROOT
    rust_bin=$(mx_rust_runtime_bin) || exit $?
    MX_PR_POLL_CHECK_PATH=$0; export MX_PR_POLL_CHECK_PATH
    exec "$rust_bin" review mx-pr-poll.sh "$@"
  fi
fi

if [ "$#" -eq 6 ] && [ "$1" = --validated ]; then
  provider=$2
  url=$3
  host=$4
  path=$5
  number=$6
elif [ "$#" -eq 0 ]; then
  case "$0" in
    *.check.sh) data=${0%.check.sh}.pr-poll ;;
    *) exit 0 ;;
  esac

  [ -f "$data" ] && [ ! -L "$data" ] || exit 0
  { exec 3< "$data"; } 2>/dev/null || exit 0
  IFS= read -r provider <&3 || exit 0
  IFS= read -r url <&3 || exit 0
  IFS= read -r host <&3 || exit 0
  IFS= read -r path <&3 || exit 0
  IFS= read -r number <&3 || exit 0
  if IFS= read -r _extra <&3; then
    exit 0
  fi
  exec 3<&-
else
  exit 0
fi

case "$number" in
  [1-9]*) ;;
  *) exit 0 ;;
esac
case "$number" in
  *[!0-9]*) exit 0 ;;
esac

# Every component is revalidated here rather than trusted from the sidecar, and
# the stored URL must then be exactly reconstructible from those components, so
# a doctored sidecar cannot redirect this poll at another host or project.
case "$provider" in
  github)
    [ "$host" = github.com ] || exit 0
    owner=${path%%/*}
    repo=${path#*/}
    [ "${#owner}" -ge 1 ] && [ "${#owner}" -le 39 ] || exit 0
    case "$owner" in
      *[!A-Za-z0-9-]*|-*|*-|*--*) exit 0 ;;
    esac
    [ "${#repo}" -ge 1 ] && [ "${#repo}" -le 100 ] || exit 0
    case "$repo" in
      .|..|*[!A-Za-z0-9._-]*) exit 0 ;;
    esac
    [ "$url" = "https://github.com/$owner/$repo/pull/$number" ] || exit 0
    state=$(gh pr view "$url" --json state -q .state 2>/dev/null) || exit 0
    [ "$state" = MERGED ] && printf '%s\n' merged
    ;;
  *)
    # GitHub is the only supported provider. A record carrying any other
    # provider tag (for example one written by an older install that watched
    # another forge) can never be polled here, so report it on stderr where a
    # watcher log makes it visible; stdout stays silent because only an exact
    # merged line may ever wake a task.
    printf 'error: unsupported PR provider "%s" in poll sidecar\n' "$provider" >&2
    exit 0
    ;;
esac
exit 0
