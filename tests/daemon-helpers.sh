#!/usr/bin/env bash
# tests/daemon-helpers.sh - shared fixtures and mocks for the daemon
# suites (mx-daemon-lifecycle-e2e and mx-daemon-safety).
#
# Endpoint operations use a recording tmux mock. A failing retired-provider
# sentinel proves these suites use the built-in home and worktree owners.
# Generic Git, identity and metadata primitives come from lib.sh.

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# Fake tmux records window operations and reports fixture endpoint state.
# The retired provider sentinel logs and fails every invocation.
make_fake_tmux() {
  local dir=$1 fakebin capture
  fakebin=$(mx_fakebin "$dir")
  capture="$dir/pane.txt"
  printf 'idle prompt\n' > "$capture"
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
case "${1:-}" in
  new-window)
    printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
    printf '@1\n'
    exit 0
    ;;
  has-session|new-session|kill-window)
    printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
    exit 0
    ;;
  send-keys)
    printf '%s\n' "$*" >> "$MX_FAKE_TMUX_LOG"
    prev=
    for argument in "$@"; do
      if [ "$prev" = -l ]; then
        launch_script=${argument#\'}
        launch_script=${launch_script%\'}
        [ ! -f "$launch_script" ] || cat "$launch_script" >> "$MX_FAKE_TMUX_LOG"
      fi
      prev=$argument
    done
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
printf 'unexpected retired provider invocation: treehouse %s\n' "$*" >> "${MX_FAKE_TMUX_LOG:-/dev/null}"
exit 99
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

# Clone the Multplx tree and activate the disposable broker fixture.
make_activated_broker_clone() {
  local home=$1 contract="$ROOT/AGENTS_E.md"
  [ -f "$contract" ] || contract="$ROOT/AGENTS.md"
  [ -f "$contract" ] || { printf 'error: source contract is missing\n' >&2; return 1; }
  git clone --quiet "$ROOT" "$home"
  # Activation is confined to this disposable fixture; production discovery stays exact.
  cp "$contract" "$home/AGENTS.md"
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
