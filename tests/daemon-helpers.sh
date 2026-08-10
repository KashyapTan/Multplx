#!/usr/bin/env bash
# tests/daemon-helpers.sh - shared fixtures and mocks for the daemon
# suites (mx-daemon-lifecycle-e2e and mx-daemon-safety).
#
# These mocks encode daemon-lifecycle behavior (fake tmux that logs window
# ops, fake treehouse that leases/returns homes, fake deep-review that records
# init/doctor), so they live here rather than in the generic tests/lib.sh. The
# generic git/identity/meta primitives come from lib.sh, which this file pulls in.

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# A fake tmux (window ops are logged to MX_FAKE_TMUX_LOG, list-windows returns
# MX_FAKE_TMUX_WINDOW, capture-pane echoes MX_FAKE_TMUX_CAPTURE) plus a fake
# treehouse (durable lease of MX_FAKE_TREEHOUSE_HOME, recording the lease holder
# to MX_FAKE_TREEHOUSE_LEASE_FILE; `return` removes the target and lease unless
# MX_FAKE_TREEHOUSE_RETURN_FAIL is set). Echoes the fakebin dir.
make_fake_tmux() {
  local dir=$1 fakebin capture
  fakebin=$(mx_fakebin "$dir")
  capture="$dir/pane.txt"
  printf 'idle prompt\n' > "$capture"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "${1:-}" in
  has-session|new-session|new-window|send-keys|kill-window)
    printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
    exit 0
    ;;
  list-windows)
    if [ -n "${MX_FAKE_TMUX_WINDOW:-}" ]; then
      printf '%s\n' "$MX_FAKE_TMUX_WINDOW"
    fi
    exit 0
    ;;
  display-message)
    printf 'broker\n'
    exit 0
    ;;
  capture-pane)
    printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
    cat "$MX_FAKE_TMUX_CAPTURE"
    exit 0
    ;;
esac
exit 1
SH
  cat > "$fakebin/treehouse" <<'SH'
#!/usr/bin/env bash
set -u
printf 'treehouse %s\n' "$*" >> "${MX_FAKE_TMUX_LOG:-/dev/null}"
case "${1:-}" in
  get)
    # Durable lease: print only the worktree path to stdout (banners to stderr),
    # and record the lease holder so tests can assert it is set and later cleared.
    shift
    holder=
    while [ $# -gt 0 ]; do
      case "$1" in
        --lease) ;;
        --lease-holder) shift; holder=${1:-} ;;
        --lease-holder=*) holder=${1#--lease-holder=} ;;
      esac
      shift
    done
    if [ -n "${MX_FAKE_TREEHOUSE_HOME:-}" ]; then
      mkdir -p "$MX_FAKE_TREEHOUSE_HOME"
      [ -n "${MX_FAKE_TREEHOUSE_LEASE_FILE:-}" ] && printf '%s\n' "$holder" > "$MX_FAKE_TREEHOUSE_LEASE_FILE"
      printf 'leased worktree for %s\n' "${holder:-unknown}" >&2
      printf '%s\n' "$MX_FAKE_TREEHOUSE_HOME"
    fi
    exit 0
    ;;
  return)
    shift
    target=
    while [ $# -gt 0 ]; do
      case "$1" in
        --force) ;;
        *) target=$1 ;;
      esac
      shift
    done
    [ -z "${MX_FAKE_TREEHOUSE_RETURN_FAIL:-}" ] || exit 17
    [ -n "${MX_FAKE_TREEHOUSE_LEASE_FILE:-}" ] && rm -f "$MX_FAKE_TREEHOUSE_LEASE_FILE"
    [ -n "$target" ] && rm -rf -- "$target"
    exit 0
    ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
  chmod +x "$fakebin/treehouse"
  : > "$dir/tmux.log"
  printf '%s\n' "$fakebin"
}

# Make a directory look like a minimal Multplx home (AGENTS.md + bin/).
mark_broker_home() {
  local home=$1
  mkdir -p "$home/bin"
  printf '# Multplx\n' > "$home/AGENTS.md"
}

# A Multplx home that is also a real git repo (so it can host detached
# worktrees for teardown/lease tests).
make_broker_git_root() {
  local home=$1
  mkdir -p "$home/bin"
  printf '# Multplx\n' > "$home/AGENTS.md"
  cat > "$home/bin/mx-guard.sh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$home/bin/mx-guard.sh"
  git -C "$home" init -q
  git -C "$home" add AGENTS.md bin/mx-guard.sh
  git -C "$home" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm initial
}

# Clone the Multplx tree and activate only the disposable broker fixture.
# During the Rust port, commit the standard contract name inside the fixture so
# nested daemon-home clones retain it without relaxing production validation.
make_activated_broker_clone() {
  local home=$1
  git clone --quiet "$ROOT" "$home"
  if [ -f "$home/AGENTS-PORTING.md" ]; then
    [ ! -e "$home/AGENTS.md" ] || {
      printf 'error: disposable broker fixture contains both root contracts\n' >&2
      return 1
    }
    git -C "$home" mv AGENTS-PORTING.md AGENTS.md
    git -C "$home" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' \
      commit -qm 'test: activate broker fixture'
  fi
  [ -f "$home/AGENTS.md" ] || {
    printf 'error: disposable broker fixture is missing AGENTS.md\n' >&2
    return 1
  }
}

# Scaffold a filled daemon charter brief under <home>/data/<id>/brief.md.
# Args: home id charter [project...]
scaffold_daemon_charter() {
  local home=$1 id=$2 charter=$3
  shift 3
  MX_HOME="$home" MX_DAEMON_CHARTER="$charter" "$ROOT/bin/mx-brief.sh" "$id" --daemon "$@" >/dev/null
}

# Make a directory look like a genuine seeded daemon home (for handoff tests).
seed_daemon_home_marker() {
  local home=$1 id=$2
  mark_broker_home "$home"
  mkdir -p "$home/data"
  printf '%s\n' "$id" > "$home/.mx-daemon-home"
}

# Wait up to <limit> 0.1s ticks while <pid> stays alive. Returns 1 if it dies.
wait_live() {
  local pid=$1 limit=${2:-30} i=0
  while [ "$i" -lt "$limit" ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 1
    fi
    sleep 0.1
    i=$((i + 1))
  done
  return 0
}
