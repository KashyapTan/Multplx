#!/usr/bin/env bash
# bin/mx-backend-hometag-lib.sh - shared per-installation home-tag derivation
# for session-provider backends whose container has ONE namespace shared by
# every Multplx home on the machine, with no native per-home split (cmux's
# one app-global workspace list). Without a per-home discriminator embedded in the actual
# title/name, two Multplx homes (two daemons, a primary plus a
# daemon, or two independent primary installations) whose task ids
# happen to collide can send/peek/close each other's tabs - the gap a
# maintainer-directed deep review caught for cmux
# (docs/cmux-backend.md).
#
# mx_backend_hometag() derives a short, stable tag: a readable prefix
# ("broker" for the primary home, "daemon-<id>" for a daemon home
# carrying .mx-daemon-home) plus a short hash of the resolved MX_ROOT
# path, so distinct installations - including multiple primaries on one
# machine - never collide even though they share one backend-global
# namespace. Callers source this file AFTER resolving their own
# MX_HOME/MX_ROOT fallbacks (both adapters already do this for their own
# purposes before any other function runs).
#
# Moving/relocating a Multplx installation changes its MX_ROOT path and
# therefore its tag; titles created under the old tag simply stop matching -
# an accepted limitation, no worse than the existing fact that a task's
# recorded absolute worktree path does not survive a move either.

MX_BACKEND_HOMETAG_DAEMON_MARKER=".mx-daemon-home"

mx_backend_hometag() {
  local marker="$MX_HOME/$MX_BACKEND_HOMETAG_DAEMON_MARKER" id prefix root hash
  if [ -f "$marker" ]; then
    id=$(tr -d '[:space:]' < "$marker" 2>/dev/null)
    if [ -n "$id" ]; then
      prefix="daemon-$id"
    else
      prefix="broker"
    fi
  else
    prefix="broker"
  fi
  root=$(cd "$MX_ROOT" 2>/dev/null && pwd -P) || root=$MX_ROOT
  if command -v shasum >/dev/null 2>&1; then
    hash=$(printf '%s' "$root" | shasum -a 256 | awk '{print substr($1,1,8)}')
  elif command -v sha256sum >/dev/null 2>&1; then
    hash=$(printf '%s' "$root" | sha256sum | awk '{print substr($1,1,8)}')
  else
    hash=$(printf '%s' "$root" | cksum | awk '{printf "%08x", $1}')
  fi
  printf '%s-%s' "$prefix" "$hash"
}
