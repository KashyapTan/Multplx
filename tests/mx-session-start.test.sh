#!/usr/bin/env bash
# tests/mx-session-start.test.sh - behavior tests for bin/mx-session-start.sh,
# the single command that collapses AGENTS.md sections 3 (bootstrap) and 5
# (recovery) into one ordered digest.
#
# Coverage:
#   - absent-file markers vs empty-but-present files in the context digest
#   - the lock-refusal read-only path: banner leads, every mutating step is
#     skipped (including bootstrap's five mutating sweeps, verified by their
#     ABSENCE), the digest still completes
#   - output section ordering: diagnostics/banners lead, bulk file dumps follow
#   - context-aware next-step guidance for read-only, AFK, and normal
#     watcher ownership
#   - status-tail bounding, default and MX_SESSION_START_STATUS_TAIL override
#   - orphan status logs whose task meta has already disappeared
#   - per-task endpoint-liveness lines for a live and a dead recorded target,
#     tmux and herdr both
#   - composition: the script invokes the real mx-lock.sh/mx-bootstrap.sh/
#     mx-wake-drain.sh (their real, distinctive output appears verbatim), it
#     does not reimplement their logic
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
# shellcheck source=tests/wake-helpers.sh
. "$(dirname "${BASH_SOURCE[0]}")/wake-helpers.sh"

SESSION_START="$ROOT/bin/mx-session-start.sh"
BASE_PATH=${MX_TEST_BASE_PATH:-/usr/bin:/bin:/usr/sbin:/sbin}
TMP_ROOT=$(mx_test_tmproot mx-session-start-tests)
SESSION_START_DAEMON_ID="fmtest-sm-${TMP_ROOT##*.}"
SESSION_START_DAEMON_TMP="/tmp/mx-$SESSION_START_DAEMON_ID"
SESSION_START_HERDR_DAEMON_ID="fmtest-herdr-${TMP_ROOT##*.}"
SESSION_START_HERDR_DAEMON_TMP="/tmp/mx-$SESSION_START_HERDR_DAEMON_ID"
MX_TEST_CLEANUP_DIRS+=("$TMP_ROOT" "$SESSION_START_DAEMON_TMP" "$SESSION_START_HERDR_DAEMON_TMP")
trap mx_test_cleanup EXIT
mx_git_identity fmtest fmtest@example.invalid

# --- world builders ----------------------------------------------------------

# new_world <name>: a real, throwaway git repo on `main` (so the worktree-tangle
# and default-branch checks behave exactly as they do against the real
# Multplx repo) to use as MX_ROOT_OVERRIDE, plus an empty MX_HOME with
# state/, data/, config/, and a fakebin. Echoes "<root-dir>|<home-dir>|<fakebin>".
new_world() {
  local name=$1 w root home fakebin
  w="$TMP_ROOT/$name"
  root="$w/root"
  home="$w/home"
  fakebin="$w/fakebin"
  mkdir -p "$home/state" "$home/data" "$home/config" "$fakebin"
  git init -q -b main "$root"
  git -C "$root" commit -q --allow-empty -m init
  printf '%s|%s|%s\n' "$root" "$home" "$fakebin"
}

# make_fake_toolchain <fakebin>: every tool mx-bootstrap.sh detects, present
# and compatible, so its own detect-only section stays quiet except where a
# test deliberately breaks one. Mirrors mx-bootstrap.test.sh's fixture.
make_fake_toolchain() {
  local fakebin=$1
  mx_fake_exit0 "$fakebin" tmux node gh-axi chrome-devtools-axi lavish-axi
  cat > "$fakebin/gh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$fakebin/gh"
  cat > "$fakebin/treehouse" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = get ] && [ "${2:-}" = --help ]; then
  printf '%s\n' 'Usage: treehouse get [--lease]'
  exit 0
fi
exit 0
SH
  chmod +x "$fakebin/treehouse"
  cat > "$fakebin/no-mistakes" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = --version ]; then
  printf '%s\n' 'no-mistakes version v1.31.2 (fake) 2026-06-27T00:02:18Z'
  exit 0
fi
exit 0
SH
  chmod +x "$fakebin/no-mistakes"
  printf '%s\n' manual > "${fakebin%/*}/home-placeholder" 2>/dev/null || true
}

make_fake_tasks_axi_compact() {
  local fakebin=$1
  cat > "$fakebin/tasks-axi" <<'SH'
#!/usr/bin/env bash
set -u
log=${MX_FAKE_TASKS_AXI_LOG:-}
[ -n "$log" ] && printf '%s\n' "$*" >> "$log"
case "${1:-}" in
  --version|-v|-V)
    printf '%s\n' '0.2.3'
    exit 0
    ;;
  update)
    if [ "${2:-}" = --help ]; then
      printf '%s\n' 'usage: tasks-axi update <id> [--archive-body]'
      exit 0
    fi
    ;;
  mv)
    if [ "${2:-}" = --help ]; then
      printf '%s\n' 'usage: tasks-axi mv <dest> [<id>...]'
      exit 0
    fi
    ;;
  list)
    case "$*" in
      *'--fields '*'body'*|*'--fields='*'body'*)
        printf '%s\n' 'unexpected body field requested' >&2
        exit 9
        ;;
    esac
    case "$*" in *'--limit 80'*) : ;; *) printf '%s\n' 'missing compact limit' >&2; exit 9 ;; esac
    case "$*" in *'--file '*) : ;; *) printf '%s\n' 'missing explicit backlog file' >&2; exit 9 ;; esac
    cat <<'OUT'
count: 2
tasks[2]{id,state,kind,repo,title,blocked_by,hold_kind,hold_reason}:
  compact-startup,in_flight,delivery,broker,Compact startup digest,none,maintainer,maintainer choice pending
  blocked-followup,queued,scout,broker,Follow compact startup,compact-startup,"-","-"
help[2]:
  - Run `tasks-axi show <id> --full` for full notes on a task
  - Run `tasks-axi ready` to see unblocked queued work
OUT
    exit 0
    ;;
esac
exit 1
SH
  chmod +x "$fakebin/tasks-axi"
}

# make_fake_ps_claude <fakebin>: harness_pid()/holder_alive() (mx-lock.sh) walk
# `ps` output looking for a harness command name; this fake reports EVERY
# queried pid as a live `claude` harness, so the very first ancestry check
# (this test process's own pid) matches and lock acquisition succeeds
# deterministically, via a fake ps.
make_fake_ps_claude() {
  local fakebin=$1
  make_fake_ps_harness "$fakebin" claude
}

make_fake_ps_harness() {
  local fakebin=$1 harness=$2
  cat > "$fakebin/ps" <<'SH'
#!/usr/bin/env bash
set -u
harness=${MX_FAKE_HARNESS:-claude}
case "$*" in
  *"comm="*) printf '/usr/local/bin/%s\n' "$harness"; exit 0 ;;
  *"args="*) printf '%s\n' "$harness"; exit 0 ;;
esac
exit 1
SH
  chmod +x "$fakebin/ps"
  printf '%s\n' "$harness" > "$fakebin/.harness-name"
}

make_fake_ps_pi_holder() {
  local fakebin=$1 holder_pid=$2
  cat > "$fakebin/ps" <<SH
#!/usr/bin/env bash
set -u
pid=""
prev=""
for arg in "\$@"; do
  [ "\$prev" = "-p" ] && pid="\$arg"
  prev="\$arg"
done
case "\$*" in
  *"comm="*)
    if [ "\$pid" = "$holder_pid" ]; then
      printf '/usr/local/bin/pi\n'
    else
      printf '/bin/zsh\n'
    fi
    exit 0
    ;;
  *"args="*)
    if [ "\$pid" = "$holder_pid" ]; then
      printf 'pi\n'
    else
      printf 'zsh\n'
    fi
    exit 0
    ;;
  *"ppid="*) printf '%s\n' "$holder_pid"; exit 0 ;;
esac
exit 1
SH
  chmod +x "$fakebin/ps"
}

# make_fake_tmux <fakebin> <live-target>: display-message succeeds only for
# the given "session:window" target - the exact primitive
# mx_backend_target_exists uses for a tmux endpoint liveness read.
make_fake_tmux() {
  local fakebin=$1 live=$2
  cat > "$fakebin/tmux" <<SH
#!/usr/bin/env bash
set -u
case "\${1:-}" in
  display-message)
    target=""
    prev=""
    for a in "\$@"; do
      [ "\$prev" = "-t" ] && target="\$a"
      prev="\$a"
    done
    [ "\$target" = "$live" ] && { printf '%%1\n'; exit 0; }
    exit 1
    ;;
esac
exit 1
SH
  chmod +x "$fakebin/tmux"
}

# make_fake_tmux_daemon_recovery <fakebin>: a stateful tmux boundary
# fixture for the real session-start -> bootstrap -> spawn path.
# MX_FAKE_TMUX_MODE selects missing, ambiguous, unreadable, or shell; missing
# reproduces real tmux's active-window fallback while inventory omits the mate.
make_fake_tmux_daemon_recovery() {
  local fakebin=$1
  cat > "$fakebin/tmux" <<'SH'
#!/usr/bin/env bash
set -u
mode=${MX_FAKE_TMUX_MODE:?}
log=${MX_FAKE_TMUX_LOG:?}
spawned=${MX_FAKE_TMUX_SPAWNED:?}
killed=${spawned}.killed
mate_home=${MX_FAKE_DAEMON_HOME:?}
mate_id=${MX_FAKE_DAEMON_ID:?}
mate_window="mx-$mate_id"
case "${1:-}" in
  display-message)
    target=
    format=
    prev=
    for arg in "$@"; do
      [ "$prev" = -t ] && target=$arg
      prev=$arg
      case "$arg" in '#{'*) format=$arg ;; esac
    done
    if [ "${target#%}" != "$target" ]; then
      case "$format" in
        *pane_current_path*) printf '%s\n' "$mate_home" ;;
        *pane_current_command*) printf '%s\n' node ;;
        *) printf '%s\n' "$target" ;;
      esac
      exit 0
    fi
    if [ -e "$spawned" ]; then
      case "$format" in
        *pane_current_command*) printf '%s\n' node ;;
        *) printf '%%1\n' ;;
      esac
      exit 0
    fi
    case "$mode" in
      ambiguous)
        case "$format" in *pane_current_command*) printf '%s\n' node ;; *) printf '%%1\n' ;; esac
        exit 0
        ;;
      shell)
        case "$format" in *pane_current_command*) printf '%s\n' zsh ;; *) printf '%%1\n' ;; esac
        exit 0
        ;;
      missing)
        case "$format" in *pane_current_command*) printf '%s\n' node ;; *) printf '%%fallback\n' ;; esac
        exit 0
        ;;
      unreadable) exit 1 ;;
    esac
    ;;
  list-windows)
    if [ "$mode" = unreadable ] && [ ! -e "$spawned" ] && [ ! -e "$killed" ]; then
      exit 1
    fi
    if [ -e "$spawned" ]; then
      printf '%s\n' "$mate_window"
    elif [ ! -e "$killed" ] && { [ "$mode" = ambiguous ] || [ "$mode" = shell ]; }; then
      printf '%s\n' "$mate_window"
    else
      printf '%s\n' main
    fi
    exit 0
    ;;
  has-session) exit 0 ;;
  kill-window)
    printf '%s\n' "$*" >> "$log"
    : > "$killed"
    exit 0
    ;;
  new-window)
    printf '%s\n' "$*" >> "$log"
    : > "$spawned"
    printf '%%1\n'
    exit 0
    ;;
  set-window-option|send-keys) exit 0 ;;
esac
exit 0
SH
  chmod +x "$fakebin/tmux"
}

make_fake_herdr_daemon_recovery() {
  local fakebin=$1
  cat > "$fakebin/herdr" <<'SH'
#!/usr/bin/env bash
set -u
log=${MX_FAKE_HERDR_LOG:?}
state=${MX_FAKE_HERDR_STATE:?}
mate_id=${MX_FAKE_DAEMON_ID:?}
killed="${state}.killed"
spawned="${state}.spawned"
printf '%s\n' "$*" >> "$log"
case "${1:-} ${2:-}" in
  "status --json")
    printf '%s\n' '{"client":{"protocol":14,"version":"test"},"server":{"running":true}}'
    ;;
  "workspace list")
    printf '{"result":{"workspaces":[{"workspace_id":"ws1","label":"daemon-%s"}]}}\n' "$mate_id"
    ;;
  "tab list")
    if [ -e "$spawned" ]; then
      printf '{"result":{"tabs":[{"tab_id":"t-new","workspace_id":"ws1","label":"mx-%s"}]}}\n' "$mate_id"
    elif [ -e "$killed" ]; then
      printf '%s\n' '{"result":{"tabs":[]}}'
    else
      printf '{"result":{"tabs":[{"tab_id":"t-old","workspace_id":"ws1","label":"mx-%s"}]}}\n' "$mate_id"
    fi
    ;;
  "tab create")
    : > "$spawned"
    printf '%s\n' '{"result":{"tab":{"tab_id":"t-new"},"root_pane":{"pane_id":"p-new"}}}'
    ;;
  "pane list")
    if [ -e "$spawned" ]; then
      printf '%s\n' '{"result":{"panes":[{"pane_id":"p-new","tab_id":"t-new"}]}}'
    elif [ ! -e "$killed" ]; then
      printf '%s\n' '{"result":{"panes":[{"pane_id":"p-old","tab_id":"t-old"}]}}'
    else
      printf '%s\n' '{"result":{"panes":[]}}'
    fi
    ;;
  "pane get")
    pane=${3:-}
    if [ "$pane" = p-new ] && [ -e "$spawned" ]; then
      printf '%s\n' '{"result":{"pane":{"pane_id":"p-new"}}}'
    elif [ "$pane" = p-old ] && [ ! -e "$killed" ]; then
      printf '%s\n' '{"result":{"pane":{"pane_id":"p-old"}}}'
    else
      printf '%s\n' '{"error":{"code":"pane_not_found"}}' >&2
      exit 1
    fi
    ;;
  "agent get")
    if [ "${3:-}" = p-new ] && [ -e "$spawned" ]; then
      printf '%s\n' '{"result":{"agent":{"agent_status":"idle"}}}'
    else
      printf '%s\n' '{"error":{"code":"agent_not_found"}}' >&2
      exit 1
    fi
    ;;
  "pane close")
    [ "${3:-}" = p-old ] && : > "$killed"
    ;;
  "pane run"|"pane send-text"|"pane send-keys"|"tab close")
    ;;
  *)
    exit 1
    ;;
esac
exit 0
SH
  chmod +x "$fakebin/herdr"
}

# make_fake_herdr <fakebin> <live-pane>: `herdr pane get <pane>` succeeds only
# for the given pane id - the exact primitive mx_backend_target_exists uses
# for a herdr endpoint liveness read. No version/server-start calls: a
# liveness check must never auto-start a server (mx-backend.sh's contract).
make_fake_herdr() {
  local fakebin=$1 live=$2
  cat > "$fakebin/herdr" <<SH
#!/usr/bin/env bash
set -u
if [ "\${1:-}" = pane ] && [ "\${2:-}" = get ]; then
  [ "\${3:-}" = "$live" ] && exit 0
  exit 1
fi
exit 1
SH
  chmod +x "$fakebin/herdr"
}

# run_session_start <home> <root> <path>
# Drop every harness env marker from bin/mx-harness.sh detect_own so the
# surrounding interactive shell cannot leak past the suite's fake ps harness.
# Markers today: CLAUDECODE (claude), PI_CODING_AGENT (pi).
# codex has no env marker (ancestry only). Without this, a local
# claude/pi session fails cases that pin a different fake harness while CI
# (no ambient markers) still passes.
run_session_start() {
  local home=$1 root=$2 path=$3
  env -u CLAUDECODE -u PI_CODING_AGENT \
    MX_HOME="$home" MX_ROOT_OVERRIDE="$root" PATH="$path" \
    "$SESSION_START"
}

# prepare_session_start_daemon <name>: a throwaway main home and Pi
# daemon home wired to the real spawn implementation through the fixture
# root. Echoes root|home|fakebin|mate|log|spawned.
prepare_session_start_daemon() {
  local name=$1 rec root home fakebin w mate log spawned id=$SESSION_START_DAEMON_ID
  rec=$(new_world "$name")
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  w=${root%/root}
  mate="$w/daemon-$id"
  log="$w/tmux.log"
  spawned="$w/tmux.spawned"
  mkdir -p "$mate/bin" "$mate/data" "$mate/state" "$mate/config" "$mate/projects"
  printf '%s\n' "$id" > "$mate/.mx-daemon-home"
  printf '# Multplx\n' > "$mate/AGENTS.md"
  printf 'Daemon charter.\n' > "$mate/data/charter.md"
  printf '%s\n' pi > "$home/config/daemon-harness"
  printf '%s\n' manual > "$home/config/backlog-backend"
  touch "$home/state/.last-watcher-beat"
  {
    printf 'window=broker:mx-%s\n' "$id"
    printf 'kind=daemon\n'
    printf 'harness=pi\n'
    printf 'home=%s\n' "$mate"
  } > "$home/state/$id.meta"
  ln -s "$ROOT/bin" "$root/bin"
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  make_fake_tmux_daemon_recovery "$fakebin"
  : > "$log"
  printf '%s|%s|%s|%s|%s|%s\n' "$root" "$home" "$fakebin" "$mate" "$log" "$spawned"
}

run_session_start_daemon() {
  local root=$1 home=$2 fakebin=$3 mate=$4 log=$5 spawned=$6 mode=$7
  MX_BACKEND=tmux MX_FAKE_TMUX_MODE="$mode" MX_FAKE_TMUX_LOG="$log" \
    MX_FAKE_TMUX_SPAWNED="$spawned" MX_FAKE_DAEMON_HOME="$mate" \
    MX_FAKE_DAEMON_ID="$SESSION_START_DAEMON_ID" \
    run_session_start "$home" "$root" "$fakebin:$BASE_PATH"
}

prepare_session_start_herdr_daemon() {
  local name=$1 rec root home fakebin w mate log state id=$SESSION_START_HERDR_DAEMON_ID
  rec=$(new_world "$name")
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  w=${root%/root}
  mate="$w/daemon-$id"
  log="$w/herdr.log"
  state="$w/herdr.state"
  mkdir -p "$mate/bin" "$mate/data" "$mate/state" "$mate/config" "$mate/projects"
  printf '%s\n' "$id" > "$mate/.mx-daemon-home"
  printf '# Multplx\n' > "$mate/AGENTS.md"
  printf 'Daemon charter.\n' > "$mate/data/charter.md"
  printf '%s\n' herdr > "$home/config/backend"
  printf '%s\n' pi > "$home/config/daemon-harness"
  printf '%s\n' manual > "$home/config/backlog-backend"
  touch "$home/state/.last-watcher-beat"
  {
    printf 'window=default:p-old\n'
    printf 'kind=daemon\n'
    printf 'harness=pi\n'
    printf 'home=%s\n' "$mate"
    printf 'backend=herdr\n'
    printf 'herdr_session=default\n'
    printf 'herdr_workspace_id=ws1\n'
    printf 'herdr_tab_id=t-old\n'
    printf 'herdr_pane_id=p-old\n'
  } > "$home/state/$id.meta"
  ln -s "$ROOT/bin" "$root/bin"
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  make_fake_herdr_daemon_recovery "$fakebin"
  : > "$log"
  printf '%s|%s|%s|%s|%s|%s\n' "$root" "$home" "$fakebin" "$mate" "$log" "$state"
}

run_session_start_herdr_daemon() {
  local root=$1 home=$2 fakebin=$3 mate=$4 log=$5 state=$6
  MX_BACKEND=herdr MX_FAKE_HERDR_LOG="$log" MX_FAKE_HERDR_STATE="$state" \
    MX_FAKE_DAEMON_ID="$SESSION_START_HERDR_DAEMON_ID" \
    run_session_start "$home" "$root" "$fakebin:$BASE_PATH"
}

hash_file_for_test() {
  local file=$1
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print "sha256:" $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print "sha256:" $1}'
  else
    cksum "$file" | awk '{print "cksum:" $1 ":" $2}'
  fi
}

install_pi_turnend_extension_fixture() {
  local root=$1
  mkdir -p "$root/.pi/extensions"
  cp "$ROOT/.pi/extensions/mx-primary-turnend-guard.ts" "$root/.pi/extensions/mx-primary-turnend-guard.ts"
}

install_pi_watch_extension_fixture() {
  local root=$1
  mkdir -p "$root/.pi/extensions"
  cp "$ROOT/.pi/extensions/mx-primary-pi-watch.ts" "$root/.pi/extensions/mx-primary-pi-watch.ts"
}

write_pi_watch_loaded_marker() {
  local home=$1 root=$2 pid=$3 version
  version=$(hash_file_for_test "$root/.pi/extensions/mx-primary-pi-watch.ts")
  printf '%s\n%s\n' "$version" "$pid" > "$home/state/.pi-watch-extension-loaded"
}

write_pi_turnend_loaded_marker() {
  local home=$1 root=$2 pid=$3 version
  version=$(hash_file_for_test "$root/.pi/extensions/mx-primary-turnend-guard.ts")
  printf '%s\n%s\n' "$version" "$pid" > "$home/state/.pi-turnend-extension-loaded"
}

write_pi_loaded_markers() {
  local home=$1 root=$2 pid=$3
  write_pi_watch_loaded_marker "$home" "$root" "$pid"
  write_pi_turnend_loaded_marker "$home" "$root" "$pid"
}

# --- context digest: absent vs empty vs present -----------------------------

test_context_digest_absent_empty_present() {
  local rec root home fakebin out
  rec=$(new_world context-digest)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"

  printf '%s\n' '- demo [no-mistakes] - a demo project (added 2026-07-01)' > "$home/data/projects.md"
  : > "$home/data/maintainer.md"
  # daemons.md, maintainer-shared.md, and learnings.md deliberately absent

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  assert_contains "$out" "data/projects.md" "digest did not label the projects.md section"
  assert_contains "$out" "- demo [no-mistakes] - a demo project (added 2026-07-01)" "digest did not print projects.md content"

  assert_contains "$out" "data/maintainer.md" "digest did not label the maintainer.md section"
  assert_contains "$out" "data/maintainer-shared.md (shared, main-authoritative, read-only in daemon homes)" \
    "digest did not label the shared maintainer section"

  assert_contains "$out" "data/daemons.md" "digest did not label the daemons.md section"
  assert_contains "$out" "data/learnings.md" "digest did not label the learnings.md section"

  # Exactly four context ABSENT markers (daemons.md, maintainer-shared.md,
  # learnings.md; backlog.md is covered by its own test) - and the
  # present-but-empty maintainer.md must NOT print ABSENT.
  absent_count=$(printf '%s\n' "$out" | grep -c '^ABSENT$')
  [ "$absent_count" -eq 4 ] || fail "expected 4 ABSENT markers (daemons.md, maintainer-shared.md, learnings.md, backlog.md), got $absent_count: $out"

  cap_section=$(printf '%s\n' "$out" | awk '/^data\/maintainer\.md$/{flag=1;next}/^data\//{flag=0}flag')
  assert_contains "$cap_section" "(present, empty)" "empty-but-present maintainer.md was not distinguished from ABSENT"

  pass "context digest distinguishes ABSENT, empty-but-present, and populated files"
}

# --- lock refusal: read-only path --------------------------------------------

test_lock_refusal_read_only_path() {
  local rec root home fakebin holder_pid out status
  rec=$(new_world lock-refusal)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"

  # A live daemon meta with a window pointed at nothing real - if the
  # bootstrap sweep's daemon_sync ran (a MUTATING step), it would try to
  # fast-forward this "home" and/or report a DAEMON_SYNC/NUDGE_DAEMONS
  # line. Absence of any such line is this test's proof that
  # MX_BOOTSTRAP_DETECT_ONLY=1 actually suppressed the mutating sweep.
  mkdir -p "$home/other-daemon/state"
  mx_write_daemon_meta "$home/state/sm-x.meta" "$home/other-daemon" "broker:mx-sm-x" alpha
  append_wake "$home/state" signal sm-x "done: surfaced before refusal" || fail "seed wake failed"
  git -C "$root" checkout -q -B mx/read-only-tangle

  sleep 300 &
  holder_pid=$!
  printf '%s\n' "$holder_pid" > "$home/state/.lock"

  status=0
  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH") || status=$?
  kill "$holder_pid" 2>/dev/null || true
  wait "$holder_pid" 2>/dev/null || true

  expect_code 0 "$status" "mx-session-start.sh must exit 0 even on a lock refusal"
  assert_contains "$out" "READ-ONLY SESSION" "read-only banner missing on lock refusal"
  assert_contains "$out" "another live broker session holds the lock" "read-only banner did not surface mx-lock.sh's own error text"
  assert_contains "$out" "Skipping every mutating step" "read-only banner did not explain what was skipped"
  assert_contains "$out" "skipped (read-only session)" "wake-queue section did not report itself skipped"
  assert_contains "$out" "WATCHER DOWN - SUPERVISION IS OFF" "read-only guard did not surface watcher-liveness alarm"
  assert_contains "$out" "queued wakes pending - left untouched because this session lacks verified system-lock ownership" "read-only guard did not leave queued wakes untouched without verified lock ownership"
  assert_contains "$out" "TANGLE: primary checkout on feature branch 'mx/read-only-tangle'" "read-only bootstrap did not surface the tangle diagnostic"
  assert_contains "$out" "read-only session must leave restore work" "read-only tangle diagnostic did not explain restore ownership"
  assert_contains "$out" "Stay read-only: do not arm" "read-only next step did not block direct watcher repair"
  assert_not_contains "$out" "drain them with bin/mx-wake-drain.sh" "read-only guard printed a mutating drain instruction"
  assert_not_contains "$out" "After draining queued wakes" "read-only guard printed a drain-then-rearm instruction"
  assert_not_contains "$out" "run bin/mx-watch-arm.sh" "read-only guard printed a mutating watcher-arm instruction"
  assert_not_contains "$out" "git -C $root checkout main" "read-only bootstrap printed a state-changing checkout remediation"

  # Detect-only bootstrap diagnostics still ran (the fakebin's PATH excludes
  # tasks-axi, so bootstrap's own read-only tool-detection line fires
  # deterministically regardless of what is installed on the test host).
  assert_contains "$out" "MISSING: tasks-axi (install:" "detect-only bootstrap diagnostics did not run on the read-only path"

  # The mutating daemon sweep must NOT have run: no DAEMON_SYNC/
  # NUDGE_DAEMONS line, and the sowed daemon meta's target dir is
  # untouched (mx-ff-lib would have tried to fast-forward it otherwise).
  assert_not_contains "$out" "DAEMON_SYNC" "mutating daemon sweep ran during a lock refusal"
  assert_not_contains "$out" "NUDGE_DAEMONS" "mutating daemon sweep ran during a lock refusal"

  # The rest of the digest (read-only-safe) still completed.
  assert_contains "$out" "SYSTEM STATE" "system-state digest section missing on the read-only path"
  assert_contains "$out" "NEXT STEP" "closing reminder missing on the read-only path"

  pass "a lock refusal prints a loud read-only banner, skips every mutating step, and still completes the digest"
}

test_lock_write_failure_read_only_path() {
  local rec root home fakebin out status
  rec=$(new_world lock-write-failure)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  append_wake "$home/state" signal task-a "done: must remain queued" || fail "seed wake failed"
  chmod 0500 "$home/state"

  status=0
  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH") || status=$?
  chmod 0700 "$home/state"

  expect_code 0 "$status" "mx-session-start.sh must exit 0 when lock publication fails"
  assert_contains "$out" "cannot write session lock" "lock publication failure was not surfaced"
  assert_contains "$out" "READ-ONLY SESSION" "lock publication failure did not force a read-only session"
  assert_contains "$out" "SYSTEM LOCK OWNERSHIP WAS NOT VERIFIED" "lock publication failure was misreported as a live holder"
  assert_contains "$out" "lacks verified system-lock ownership" "lock publication failure did not explain why queued wakes remain untouched"
  assert_not_contains "$out" "ANOTHER LIVE MULTPLX SESSION HOLDS THE SYSTEM LOCK" "lock publication failure falsely claimed a live lock holder"
  [ -s "$home/state/.wake-queue" ] || fail "lock publication failure allowed the wake queue to mutate"

  pass "session start stays read-only when lock ownership cannot be published"
}

test_session_lock_concurrent_single_winner() {
  local rec root home fakebin ready completed winners pids i pid count
  rec=$(new_world lock-concurrency)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  ready="$home/ready"
  completed="$home/done"
  winners="$home/winners"
  mkdir -p "$ready" "$completed"
  : > "$winners"
  cat > "$fakebin/ps" <<'SH'
#!/usr/bin/env bash
set -u
pid=
previous=
for argument in "$@"; do
  [ "$previous" = -p ] && pid=$argument
  previous=$argument
done
case "$*" in
  *"comm="*)
    if [ -f "$MX_FAKE_LOCK_STATE/harness-$pid" ]; then
      printf '%s\n' /usr/local/bin/claude
    else
      printf '%s\n' /bin/bash
    fi
    ;;
  *"args="*)
    if [ -f "$MX_FAKE_LOCK_STATE/harness-$pid" ]; then
      printf '%s\n' claude
    else
      printf '%s\n' bash
    fi
    ;;
  *"ppid="*) printf '%s\n' "$MX_FAKE_HARNESS_PID" ;;
  *) exit 1 ;;
esac
SH
  chmod +x "$fakebin/ps"

  lock_worker="$root/lock-worker.sh"
  cat > "$lock_worker" <<'SH'
#!/bin/bash
home=$1
ready=$2
index=$3
completed=$4
fakebin=$5
base_path=$6
repo_root=$7
winners=$8
harness_pid=$$
: > "$home/state/harness-$harness_pid"
: > "$ready/$index"
while [ "$(find "$ready" -type f | wc -l | tr -d ' ')" -lt 40 ]; do
  sleep 0.01
done
if MX_HOME="$home" MX_FAKE_LOCK_STATE="$home/state" \
  MX_FAKE_HARNESS_PID="$harness_pid" PATH="$fakebin:$base_path" \
  "$repo_root/bin/mx-lock.sh" >/dev/null 2>&1; then
  printf '%s\n' "$harness_pid" >> "$winners"
fi
: > "$completed/$index"
while [ "$(find "$completed" -type f | wc -l | tr -d ' ')" -lt 40 ]; do
  sleep 0.01
done
SH
  chmod +x "$lock_worker"

  pids=
  i=1
  while [ "$i" -le 40 ]; do
    "$lock_worker" "$home" "$ready" "$i" "$completed" "$fakebin" \
      "$BASE_PATH" "$ROOT" "$winners" &
    pids="$pids $!"
    i=$((i + 1))
  done
  for pid in $pids; do
    wait "$pid" 2>/dev/null || true
  done
  count=$(awk 'NF { count++ } END { print count + 0 }' "$winners")
  [ "$count" -eq 1 ] || fail "concurrent session-lock acquisition produced $count winners"

  pass "concurrent session-lock acquisition admits exactly one live harness"
}

# --- output ordering ----------------------------------------------------------

test_output_ordering_diagnostics_lead() {
  local rec root home fakebin out lock_line boot_line wake_line context_line system_line next_line
  rec=$(new_world ordering)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  # Force a MISSING diagnostic line so the bootstrap section is non-trivial.
  rm -f "$fakebin/node"

  printf 'window=mx-sess:w1\nkind=delivery\n' > "$home/state/task-a.meta"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  lock_line=$(printf '%s\n' "$out" | grep -n '^LOCK$' | head -1 | cut -d: -f1)
  boot_line=$(printf '%s\n' "$out" | grep -n '^BOOTSTRAP$' | head -1 | cut -d: -f1)
  wake_line=$(printf '%s\n' "$out" | grep -n '^WAKE QUEUE$' | head -1 | cut -d: -f1)
  context_line=$(printf '%s\n' "$out" | grep -n '^CONTEXT$' | head -1 | cut -d: -f1)
  system_line=$(printf '%s\n' "$out" | grep -n '^SYSTEM STATE$' | head -1 | cut -d: -f1)
  next_line=$(printf '%s\n' "$out" | grep -n '^NEXT STEP$' | head -1 | cut -d: -f1)

  if [ -z "$lock_line" ] || [ -z "$boot_line" ] || [ -z "$wake_line" ] || [ -z "$context_line" ] || [ -z "$system_line" ] || [ -z "$next_line" ]; then
    fail "one or more section headers missing from digest: $out"
  fi

  [ "$lock_line" -lt "$boot_line" ] || fail "LOCK did not precede BOOTSTRAP"
  [ "$boot_line" -lt "$wake_line" ] || fail "BOOTSTRAP did not precede WAKE QUEUE"
  [ "$wake_line" -lt "$context_line" ] || fail "WAKE QUEUE did not precede CONTEXT"
  [ "$context_line" -lt "$system_line" ] || fail "CONTEXT did not precede SYSTEM STATE"
  [ "$system_line" -lt "$next_line" ] || fail "SYSTEM STATE did not precede NEXT STEP"

  missing_line=$(printf '%s\n' "$out" | grep -n 'MISSING: node' | head -1 | cut -d: -f1)
  [ -n "$missing_line" ] || fail "MISSING diagnostic did not appear at all"
  [ "$missing_line" -lt "$system_line" ] || fail "actionable MISSING diagnostic was buried after the bulk system-state digest"

  pass "digest sections are ordered diagnostics-first, bulk-context-last"
}

test_herdr_backend_diagnostics_follow_real_session_start() {
  local mode rec root home fakebin mask out
  for mode in configured autodetected; do
    rec=$(new_world "herdr-$mode")
    IFS='|' read -r root home fakebin <<EOF
$rec
EOF
    make_fake_toolchain "$fakebin"
    make_fake_ps_claude "$fakebin"
    rm -f "$fakebin/tmux"
    mx_fake_exit0 "$fakebin" herdr jq
    printf '%s\n' manual > "$home/config/backlog-backend"
    mask="$home/mask-tmux.bash"
    cat > "$mask" <<'SH'
command() {
  if [ "${1:-}" = -v ] && [ "${2:-}" = tmux ]; then
    return 1
  fi
  builtin command "$@"
}
SH
    if [ "$mode" = configured ]; then
      printf '%s\n' herdr > "$home/config/backend"
      out=$(TMUX='' HERDR_ENV='' BASH_ENV="$mask" run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
      assert_not_contains "$out" "NOTICE: auto-detected herdr runtime" \
        "an explicit Herdr home should not be reported as auto-detected"
    else
      out=$(TMUX='' HERDR_ENV=1 BASH_ENV="$mask" run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
      assert_contains "$out" "NOTICE: auto-detected herdr runtime (HERDR_ENV=1)" \
        "session start did not preserve the Herdr runtime auto-detection fallback"
    fi
    assert_contains "$out" "SESSION START - $home" "the real session-start path did not run in the throwaway home"
    assert_not_contains "$out" "MISSING: tmux" "Herdr session start falsely required masked tmux"
    assert_not_contains "$out" "MISSING: herdr" "Herdr session start missed its available session CLI"
    assert_not_contains "$out" "MISSING: jq" "Herdr session start missed its available JSON dependency"
    assert_not_contains "$out" "MISSING: treehouse" "Herdr session start missed its available worktree provider"
  done
  pass "session start: configured and auto-detected Herdr homes never require tmux"
}

# --- status tail bounding -----------------------------------------------------

test_status_tail_bounding() {
  local rec root home fakebin out
  rec=$(new_world status-tail)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  make_fake_tmux "$fakebin" "mx-sess:live"

  printf 'window=mx-sess:live\nkind=delivery\n' > "$home/state/task-a.meta"
  printf 'working: step 1\nworking: step 2\nworking: step 3\nworking: step 4\nworking: step 5\nworking: step 6\nworking: step 7\n' \
    > "$home/state/task-a.status"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  assert_contains "$out" "working: step 7" "default status tail missing the most recent line"
  assert_contains "$out" "working: step 3" "default status tail (5 lines) missing an expected recent line"
  assert_not_contains "$out" "working: step 1" "default status tail (5 lines) leaked an older line"
  assert_contains "$out" "$home/state/task-a.status" "digest did not print the full status log path for a deeper read"
  assert_contains "$out" "Do NOT bulk-read state/*.status now either: their bounded tails were just" "closing reminder does not distinguish bounded status tails"
  assert_not_contains "$out" "state/*.status now - they were just" "closing reminder still describes status logs as fully printed"

  out=$(MX_SESSION_START_STATUS_TAIL=2 run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  assert_contains "$out" "working: step 7" "MX_SESSION_START_STATUS_TAIL=2 tail missing the most recent line"
  assert_not_contains "$out" "working: step 5" "MX_SESSION_START_STATUS_TAIL=2 did not bound the tail to 2 lines"

  pass "status tail is bounded to the configured line count, with the full log path always printed"
}

test_orphan_status_logs_are_printed() {
  local rec root home fakebin out matched_count orphan_count
  rec=$(new_world orphan-status)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"

  printf 'kind=delivery\n' > "$home/state/task-a.meta"
  printf 'matched: surfaced once\n' > "$home/state/task-a.status"
  printf 'orphan: step 1\norphan: step 2\norphan: step 3\norphan: step 4\norphan: step 5\norphan: step 6\n' \
    > "$home/state/task-orphan.status"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  assert_contains "$out" "Orphan status logs (state/*.status without matching .meta)" "digest did not label orphan status logs"
  assert_contains "$out" "--- task-orphan ---" "digest did not print the orphan status id"
  assert_contains "$out" "orphan: step 6" "orphan status tail missing the newest line"
  assert_not_contains "$out" "orphan: step 1" "orphan status tail was not bounded"
  assert_contains "$out" "$home/state/task-orphan.status" "orphan status tail did not print the full log path"

  matched_count=$(printf '%s\n' "$out" | grep -F -c 'matched: surfaced once')
  orphan_count=$(printf '%s\n' "$out" | grep -F -c 'orphan: step 6')
  [ "$matched_count" -eq 1 ] || fail "matched status log was printed $matched_count times: $out"
  [ "$orphan_count" -eq 1 ] || fail "orphan status log was printed $orphan_count times: $out"

  pass "orphan status logs are printed once with bounded tails"
}

# --- session-start daemon recovery boundary -----------------------------

test_session_start_relaunches_missing_pi_daemon() {
  local rec root home fakebin mate log spawned out first_calls second_calls
  rec=$(prepare_session_start_daemon daemon-missing-pi)
  IFS='|' read -r root home fakebin mate log spawned <<EOF
$rec
EOF

  out=$(run_session_start_daemon "$root" "$home" "$fakebin" "$mate" "$log" "$spawned" missing)

  assert_not_contains "$out" "DAEMON_LIVENESS:" "successful missing-window recovery should stay non-actionable"
  assert_contains "$(cat "$log")" "new-window" "session start did not relaunch the missing Pi daemon"
  assert_not_contains "$(cat "$log")" "kill-window" "session start tried to kill an already-absent window"
  assert_contains "$out" "endpoint: alive (backend=tmux window=broker:mx-$SESSION_START_DAEMON_ID)" \
    "the later system read did not confirm the relaunched window"
  assert_grep 'harness=pi' "$home/state/$SESSION_START_DAEMON_ID.meta" \
    "the real respawn path did not preserve the Pi harness: $(cat "$home/state/$SESSION_START_DAEMON_ID.meta")"

  first_calls=$(grep -c 'new-window' "$log" || true)
  rm -f "$home/state/.lock"
  run_session_start_daemon "$root" "$home" "$fakebin" "$mate" "$log" "$spawned" missing >/dev/null
  second_calls=$(grep -c 'new-window' "$log" || true)
  [ "$first_calls" -eq 1 ] && [ "$second_calls" -eq 1 ] \
    || fail "a second session-start pass duplicated the relaunched Pi daemon: $(cat "$log")"
  pass "session start: an absent recorded tmux window relaunches its Pi daemon exactly once"
}

test_session_start_preserves_ambiguous_pi_process() {
  local rec root home fakebin mate log spawned out
  rec=$(prepare_session_start_daemon daemon-ambiguous-pi)
  IFS='|' read -r root home fakebin mate log spawned <<EOF
$rec
EOF

  out=$(run_session_start_daemon "$root" "$home" "$fakebin" "$mate" "$log" "$spawned" ambiguous)

  assert_contains "$out" "DAEMON_LIVENESS: daemon $SESSION_START_DAEMON_ID: skipped: existing endpoint has ambiguous agent process (backend=tmux)" \
    "session start did not distinguish an existing Pi-shaped process from a missing window"
  [ ! -s "$log" ] || fail "session start touched an ambiguous existing Pi process: $(cat "$log")"
  assert_contains "$out" "endpoint: alive (backend=tmux window=broker:mx-$SESSION_START_DAEMON_ID)" \
    "the later system read should still see the ambiguous endpoint"
  pass "session start: an existing ambiguous Pi process prevents duplicate recovery"
}

test_session_start_preserves_transiently_unreadable_tmux() {
  local rec root home fakebin mate log spawned out
  rec=$(prepare_session_start_daemon daemon-unreadable-pi)
  IFS='|' read -r root home fakebin mate log spawned <<EOF
$rec
EOF

  out=$(run_session_start_daemon "$root" "$home" "$fakebin" "$mate" "$log" "$spawned" unreadable)

  assert_contains "$out" "DAEMON_LIVENESS: daemon $SESSION_START_DAEMON_ID: skipped: endpoint probe unreadable (backend=tmux)" \
    "session start did not distinguish transient unreadability from absence"
  [ ! -s "$log" ] || fail "session start touched a transiently unreadable target: $(cat "$log")"
  assert_contains "$out" "endpoint: dead (backend=tmux window=broker:mx-$SESSION_START_DAEMON_ID)" \
    "the later cheap presence read should preserve the visible offline symptom"
  pass "session start: transient tmux unreadability never licenses a relaunch"
}

test_session_start_preserves_proven_bare_shell_recovery() {
  local rec root home fakebin mate log spawned out
  rec=$(prepare_session_start_daemon daemon-bare-shell)
  IFS='|' read -r root home fakebin mate log spawned <<EOF
$rec
EOF

  out=$(run_session_start_daemon "$root" "$home" "$fakebin" "$mate" "$log" "$spawned" shell)

  assert_not_contains "$out" "DAEMON_LIVENESS:" "successful bare-shell recovery should stay non-actionable"
  assert_contains "$(cat "$log")" "kill-window -t broker:mx-$SESSION_START_DAEMON_ID" \
    "the proven bare-shell path did not remove its existing dead endpoint"
  assert_contains "$(cat "$log")" "new-window" "the proven bare-shell path did not relaunch"
  assert_contains "$out" "endpoint: alive (backend=tmux window=broker:mx-$SESSION_START_DAEMON_ID)" \
    "the later system read did not confirm the bare-shell relaunch"
  pass "session start: the proven bare-shell recovery path remains intact"
}

test_session_start_relaunches_herdr_husk_daemon() {
  local rec root home fakebin mate log state out
  rec=$(prepare_session_start_herdr_daemon daemon-herdr-husk)
  IFS='|' read -r root home fakebin mate log state <<EOF
$rec
EOF

  out=$(run_session_start_herdr_daemon "$root" "$home" "$fakebin" "$mate" "$log" "$state")

  assert_not_contains "$out" "DAEMON_LIVENESS:" "successful Herdr husk recovery should stay non-actionable"
  assert_contains "$(cat "$log")" "pane close p-old" "session start did not close the confirmed Herdr husk"
  assert_contains "$(cat "$log")" "tab create" "session start did not relaunch the Herdr daemon"
  assert_contains "$out" "endpoint: alive (backend=herdr window=default:p-new)" \
    "the later system read did not confirm the relaunched Herdr endpoint"
  assert_grep 'herdr_pane_id=p-new' "$home/state/$SESSION_START_HERDR_DAEMON_ID.meta" \
    "the real respawn path did not record the replacement Herdr pane"
  pass "session start: a confirmed Herdr husk is closed and relaunched"
}

# --- endpoint liveness: tmux and herdr, live and dead ------------------------

test_endpoint_liveness_tmux() {
  local rec root home fakebin out
  rec=$(new_world liveness-tmux)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  make_fake_tmux "$fakebin" "mx-sess:live-window"

  printf 'window=mx-sess:live-window\nkind=delivery\n' > "$home/state/task-live.meta"
  printf 'window=mx-sess:dead-window\nkind=delivery\n' > "$home/state/task-dead.meta"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  assert_contains "$out" "endpoint: alive (backend=tmux window=mx-sess:live-window)" "live tmux endpoint not reported alive"
  assert_contains "$out" "endpoint: dead (backend=tmux window=mx-sess:dead-window)" "dead tmux endpoint not reported dead"

  pass "tmux endpoint liveness is reported per task: alive for a live window, dead for a gone one"
}

test_endpoint_liveness_herdr() {
  local rec root home fakebin out
  rec=$(new_world liveness-herdr)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  make_fake_herdr "$fakebin" "p-live"

  printf 'window=sess:p-live\nkind=delivery\nbackend=herdr\n' > "$home/state/task-live.meta"
  printf 'window=sess:p-dead\nkind=delivery\nbackend=herdr\n' > "$home/state/task-dead.meta"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  assert_contains "$out" "endpoint: alive (backend=herdr window=sess:p-live)" "live herdr endpoint not reported alive"
  assert_contains "$out" "endpoint: dead (backend=herdr window=sess:p-dead)" "dead herdr endpoint not reported dead"

  pass "herdr endpoint liveness is reported per task: alive for a live pane, dead for a gone one"
}

# --- composition: real scripts run, not reimplemented ------------------------

test_composition_invokes_real_scripts() {
  local rec root home fakebin out
  rec=$(new_world composition)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  rm -f "$fakebin/node"

  printf 'needs-decision: pick a library\n' > "$home/state/task-z.status"
  append_wake "$home/state" signal task-z.status "needs-decision: pick a library"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  # mx-lock.sh's own exact success text.
  assert_contains "$out" "lock acquired: harness pid" "mx-lock.sh's real output did not appear (composition, not reimplementation)"
  # mx-bootstrap.sh's own exact MISSING-tool line format.
  assert_contains "$out" "MISSING: node (install:" "mx-bootstrap.sh's real detect line did not appear verbatim"
  # mx-wake-drain.sh's real drained record (raw tab-separated queue line).
  assert_contains "$out" "$(printf 'signal\ttask-z.status\tneeds-decision: pick a library')" "mx-wake-drain.sh's real drained record did not appear"
  assert_contains "$out" "wake annotation: latest wake-EVENT observed at drain, not current state: task-z.status: needs-decision: pick a library" "mx-session-start.sh did not preserve the drain's separate annotation line"

  pass "mx-session-start.sh composes the real mx-lock.sh, mx-bootstrap.sh, and mx-wake-drain.sh output verbatim"
}

# --- system-state digest: compact backlog rendering --------------------------

write_long_body_backlog() {
  local path=$1
  cat > "$path" <<'EOF'
# Backlog

## In flight
- [ ] compact-startup - Compact startup digest (repo: broker) (kind: delivery) (since 2026-07-15) (hold: maintainer choice pending) (hold-kind: maintainer)
  OVERSIZED-BODY-LINE current startup leaks task note bodies into the session digest.
  Another long body line that should not be printed after the fix.

## Queued
- [ ] blocked-followup - Follow compact startup blocked-by: compact-startup - waits for implementation (repo: broker) (kind: scout) (since 2026-07-15)
  QUEUED-BODY-LINE this is another long multiline note.

## Done
EOF
}

test_backlog_compact_tasks_axi_omits_bodies_and_keeps_metadata() {
  local rec root home fakebin out log
  rec=$(new_world backlog-compact-tasks-axi)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_tasks_axi_compact "$fakebin"
  make_fake_ps_claude "$fakebin"
  write_long_body_backlog "$home/data/backlog.md"
  mkdir -p "$home/projects/broker"
  printf 'window=mx-sess:compact\nworktree=%s\nproject=broker\nkind=delivery\n' "$home/projects/broker" \
    > "$home/state/compact-startup.meta"
  log="$home/tasks-axi.log"

  out=$(MX_FAKE_TASKS_AXI_LOG="$log" run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  assert_contains "$out" "compact backlog listing (tasks-axi; max 80 item(s); task bodies omitted)" \
    "compatible tasks-axi backend did not render the compact backlog listing"
  assert_contains "$out" "tasks[2]{id,state,kind,repo,title,blocked_by,hold_kind,hold_reason}:" \
    "tasks-axi compact listing omitted the expected structured field header"
  assert_contains "$out" "compact-startup,in_flight,delivery,broker,Compact startup digest,none,maintainer,maintainer choice pending" \
    "tasks-axi compact listing omitted in-flight identity, state, or hold metadata"
  assert_contains "$out" 'blocked-followup,queued,scout,broker,Follow compact startup,compact-startup,"-","-"' \
    "tasks-axi compact listing omitted blocked-by metadata"
  assert_not_contains "$out" "OVERSIZED-BODY-LINE" "tasks-axi compact digest leaked an in-flight task body"
  assert_not_contains "$out" "QUEUED-BODY-LINE" "tasks-axi compact digest leaked a queued task body"
  assert_contains "$out" "--- compact-startup ---" "in-flight meta identity disappeared from startup recovery digest"
  assert_contains "$out" "worktree=$home/projects/broker" "in-flight recovery worktree identity disappeared from startup digest"
  assert_contains "$out" "Full task bodies remain available on demand: tasks-axi show <id> --full" \
    "compact digest omitted the full-body lookup pointer"
  assert_grep "list --file $home/data/backlog.md --limit 80 --fields blocked_by,hold_kind,hold_reason" "$log" \
    "session start did not ask tasks-axi for the bounded compact field set"

  pass "compatible tasks-axi backlog rendering is compact, bounded, and preserves recovery metadata"
}

test_backlog_compact_manual_backend_skips_indented_bodies() {
  local rec root home fakebin out
  rec=$(new_world backlog-compact-manual)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  printf '%s\n' manual > "$home/config/backlog-backend"
  write_long_body_backlog "$home/data/backlog.md"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  assert_contains "$out" "compact backlog listing (manual backend; max 80 item(s); indented task bodies omitted)" \
    "manual backend did not use compact title-line rendering"
  assert_contains "$out" "## In flight" "manual compact rendering omitted the in-flight section heading"
  assert_contains "$out" "- [ ] compact-startup - Compact startup digest" \
    "manual compact rendering omitted the in-flight title line"
  assert_contains "$out" "(hold: maintainer choice pending) (hold-kind: maintainer)" \
    "manual compact rendering omitted hold metadata"
  assert_contains "$out" "blocked-by: compact-startup - waits for implementation" \
    "manual compact rendering omitted blocker metadata"
  assert_not_contains "$out" "OVERSIZED-BODY-LINE" "manual compact digest leaked an in-flight task body"
  assert_not_contains "$out" "QUEUED-BODY-LINE" "manual compact digest leaked a queued task body"
  assert_contains "$out" "(shown 2 of 2 backlog item title line(s))" \
    "manual compact rendering did not report its bound accounting"
  assert_contains "$out" "or data/backlog.md" "manual compact digest omitted the data/backlog.md full-body pointer"

  pass "manual backlog rendering prints only title lines with hold and blocker metadata"
}

test_backlog_compact_tasks_axi_unavailable_uses_manual_fallback() {
  local rec root home fakebin out
  rec=$(new_world backlog-compact-unavailable)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  write_long_body_backlog "$home/data/backlog.md"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  assert_contains "$out" "compact backlog listing (tasks-axi unavailable or incompatible; max 80 item(s); indented task bodies omitted)" \
    "unavailable tasks-axi did not fall back to compact title-line rendering"
  assert_contains "$out" "- [ ] compact-startup - Compact startup digest" \
    "unavailable tasks-axi fallback omitted a backlog title line"
  assert_not_contains "$out" "OVERSIZED-BODY-LINE" "unavailable tasks-axi fallback leaked an in-flight task body"

  pass "unavailable or incompatible tasks-axi falls back to compact manual backlog rendering"
}

# --- system-state digest: no in-flight tasks ----------------------------------

test_system_digest_empty_system() {
  local rec root home fakebin out
  rec=$(new_world empty-system)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  assert_contains "$out" "(none)" "empty system did not report (none) for in-flight tasks"
  assert_contains "$out" "absent" "empty system's AFK section did not report absent"

  pass "an empty system reports (none) for in-flight tasks and an absent AFK flag"
}

test_next_step_afk_delegates_to_daemon() {
  local rec root home fakebin out
  rec=$(new_world next-step-afk)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_claude "$fakebin"
  : > "$home/state/.afk"

  out=$(run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  assert_contains "$out" "away-mode supervision is active" "AFK digest did not report away mode"
  assert_contains "$out" "Away mode is active" "next step did not switch to AFK guidance"
  assert_contains "$out" "daemon owns the watcher" "next step did not delegate watcher ownership to the daemon"
  assert_contains "$out" "- Away mode: active" "supervision block did not include active AFK state"
  assert_not_contains "$out" "  bin/mx-watch-arm.sh" "AFK next step still told the agent to arm the watcher directly"

  pass "next step delegates watcher ownership to the AFK daemon"
}

test_supervision_block_exactly_one_and_pi_diagnostic() {
  local rec root home fakebin out block_count wake_line sup_line context_line
  rec=$(new_world pi-supervision-block)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"
  make_fake_ps_harness "$fakebin" pi

  out=$(MX_FAKE_HARNESS=pi run_session_start "$home" "$root" "$fakebin:$BASE_PATH")

  block_count=$(printf '%s\n' "$out" | grep -c '^SUPERVISION OPERATING INSTRUCTIONS - primary harness:')
  [ "$block_count" -eq 1 ] || fail "expected exactly one supervision block, got $block_count"
  assert_contains "$out" "SUPERVISION OPERATING INSTRUCTIONS - primary harness: pi" "pi supervision block missing"
  assert_contains "$out" "Mode: Pi extension background wake." "pi snippet missing from session start"
  assert_contains "$out" "PI_WATCH_EXTENSION: not loaded" "pi extension load diagnostic missing"
  assert_contains "$out" "restart plain pi so $root/.pi/extensions/mx-primary-turnend-guard.ts and $root/.pi/extensions/mx-primary-pi-watch.ts auto-load" "pi extension load diagnostic omits the turn-end guard extension"

  wake_line=$(printf '%s\n' "$out" | grep -n '^WAKE QUEUE$' | head -1 | cut -d: -f1)
  sup_line=$(printf '%s\n' "$out" | grep -n '^SUPERVISION OPERATING INSTRUCTIONS' | head -1 | cut -d: -f1)
  context_line=$(printf '%s\n' "$out" | grep -n '^CONTEXT$' | head -1 | cut -d: -f1)
  [ "$wake_line" -lt "$sup_line" ] || fail "supervision block did not follow wake queue"
  [ "$sup_line" -lt "$context_line" ] || fail "supervision block did not precede context"

  pass "session start emits exactly one detected harness block and reports Pi extension load state"
}

test_pi_diagnostic_rejects_stale_loaded_marker() {
  local rec root home fakebin out marker holder_pid
  rec=$(new_world pi-stale-loaded-marker)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"

  sleep 300 &
  holder_pid=$!
  make_fake_ps_pi_holder "$fakebin" "$holder_pid"
  install_pi_turnend_extension_fixture "$root"
  install_pi_watch_extension_fixture "$root"
  marker="$home/state/.pi-watch-extension-loaded"
  printf 'stale-extension-version\n%s\n' "$holder_pid" > "$marker"
  write_pi_turnend_loaded_marker "$home" "$root" "$holder_pid"
  touch -t 203001010000 "$marker" 2>/dev/null || touch "$marker"

  out=$(MX_FAKE_HARNESS=pi run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  kill "$holder_pid" 2>/dev/null || true
  wait "$holder_pid" 2>/dev/null || true

  assert_contains "$out" "PI_WATCH_EXTENSION: not loaded" "pi diagnostic trusted a stale loaded marker"

  pass "session start rejects stale Pi loaded markers"
}

test_pi_diagnostic_accepts_prelock_loaded_marker() {
  local rec root home fakebin out holder_pid
  rec=$(new_world pi-prelock-loaded-marker)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"

  sleep 300 &
  holder_pid=$!
  make_fake_ps_pi_holder "$fakebin" "$holder_pid"
  install_pi_turnend_extension_fixture "$root"
  install_pi_watch_extension_fixture "$root"

  write_pi_loaded_markers "$home" "$root" "$holder_pid"

  out=$(MX_FAKE_HARNESS=pi run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  kill "$holder_pid" 2>/dev/null || true
  wait "$holder_pid" 2>/dev/null || true

  assert_not_contains "$out" "PI_WATCH_EXTENSION: not loaded" "pi diagnostic rejected a current pre-lock loaded marker"

  pass "session start accepts current Pi markers written before lock acquisition"
}

test_pi_diagnostic_rejects_missing_turnend_guard_marker() {
  local rec root home fakebin out holder_pid
  rec=$(new_world pi-missing-turnend-marker)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"

  sleep 300 &
  holder_pid=$!
  make_fake_ps_pi_holder "$fakebin" "$holder_pid"
  install_pi_turnend_extension_fixture "$root"
  install_pi_watch_extension_fixture "$root"

  write_pi_watch_loaded_marker "$home" "$root" "$holder_pid"

  out=$(MX_FAKE_HARNESS=pi run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  kill "$holder_pid" 2>/dev/null || true
  wait "$holder_pid" 2>/dev/null || true

  assert_contains "$out" "PI_WATCH_EXTENSION: not loaded" "pi diagnostic trusted a session without the turn-end guard extension"

  pass "session start rejects Pi sessions missing the turn-end guard marker"
}

test_pi_diagnostic_rejects_previous_session_loaded_marker() {
  local rec root home fakebin out marker version holder_pid
  rec=$(new_world pi-previous-session-loaded-marker)
  IFS='|' read -r root home fakebin <<EOF
$rec
EOF
  make_fake_toolchain "$fakebin"

  sleep 300 &
  holder_pid=$!
  make_fake_ps_pi_holder "$fakebin" "$holder_pid"
  install_pi_turnend_extension_fixture "$root"
  install_pi_watch_extension_fixture "$root"
  marker="$home/state/.pi-watch-extension-loaded"
  version=$(hash_file_for_test "$root/.pi/extensions/mx-primary-pi-watch.ts")
  printf '%s\n999999\n' "$version" > "$marker"
  write_pi_turnend_loaded_marker "$home" "$root" "$holder_pid"

  out=$(MX_FAKE_HARNESS=pi run_session_start "$home" "$root" "$fakebin:$BASE_PATH")
  kill "$holder_pid" 2>/dev/null || true
  wait "$holder_pid" 2>/dev/null || true

  assert_contains "$out" "PI_WATCH_EXTENSION: not loaded" "pi diagnostic trusted a marker from a previous Pi process"

  pass "session start rejects Pi loaded markers from previous sessions"
}

test_context_digest_absent_empty_present
test_lock_refusal_read_only_path
test_lock_write_failure_read_only_path
test_session_lock_concurrent_single_winner
test_output_ordering_diagnostics_lead
test_herdr_backend_diagnostics_follow_real_session_start
test_session_start_relaunches_missing_pi_daemon
test_session_start_preserves_ambiguous_pi_process
test_session_start_preserves_transiently_unreadable_tmux
test_session_start_preserves_proven_bare_shell_recovery
test_session_start_relaunches_herdr_husk_daemon
test_status_tail_bounding
test_orphan_status_logs_are_printed
test_endpoint_liveness_tmux
test_endpoint_liveness_herdr
test_composition_invokes_real_scripts
test_backlog_compact_tasks_axi_omits_bodies_and_keeps_metadata
test_backlog_compact_manual_backend_skips_indented_bodies
test_backlog_compact_tasks_axi_unavailable_uses_manual_fallback
test_system_digest_empty_system
test_next_step_afk_delegates_to_daemon
test_supervision_block_exactly_one_and_pi_diagnostic
test_pi_diagnostic_rejects_stale_loaded_marker
test_pi_diagnostic_accepts_prelock_loaded_marker
test_pi_diagnostic_rejects_missing_turnend_guard_marker
test_pi_diagnostic_rejects_previous_session_loaded_marker
