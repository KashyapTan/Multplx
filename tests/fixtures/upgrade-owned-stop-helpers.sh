#!/usr/bin/env bash
# Isolated upgrade acceptance: real tmux sessions belong only to this fixture.
setup_upgrade_real_tmux() {
  trap 'if [ -n "${MX_UPGRADE_TMUX_SOCKET:-}" ]; then "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" kill-session -t "$MX_UPGRADE_TMUX_SESSION" 2>/dev/null || true; fi; if [ -n "${MX_UPGRADE_TMUX_DIR:-}" ]; then rm -rf "$MX_UPGRADE_TMUX_DIR"; fi; if [ -n "${MX_UPGRADE_PRESENTATION_LOCK:-}" ]; then rm -f "$MX_UPGRADE_PRESENTATION_LOCK/pid"; rmdir "$MX_UPGRADE_PRESENTATION_LOCK" 2>/dev/null || true; fi; if [ -n "${MX_UPGRADE_PRIMARY_PID:-}" ]; then kill "$MX_UPGRADE_PRIMARY_PID" 2>/dev/null || true; wait "$MX_UPGRADE_PRIMARY_PID" 2>/dev/null || true; fi; mx_test_cleanup' EXIT
  export MX_UPGRADE_TMUX_DIR="$(mktemp -d /tmp/mx-upstop.XXXXXX)"
  export MX_UPGRADE_TMUX_SOCKET="$MX_UPGRADE_TMUX_DIR/socket"
  export MX_UPGRADE_TMUX_SESSION="$(jq -r .endpoint "$install/data/home/state/.spawn-actions/$standing.json" | cut -d: -f1)"
  case "$MX_UPGRADE_TMUX_SESSION" in primary|broker) ;; *) fail "unexpected fixture session" ;; esac
  export MX_UPGRADE_REAL_TMUX="$upgrade_real_tmux"
  python3 - "$standing_tmux_bin/tmux" <<'PY'
from pathlib import Path
import sys
p=Path(sys.argv[1])
s=p.read_text().replace('#!/usr/bin/env bash\n', '''#!/usr/bin/env bash
if [ -n "${MX_UPGRADE_TMUX_SOCKET:-}" ]; then
  printf '%s\\n' "$*" >> "$MX_UPGRADE_TMUX_DIR/probes"
  if [ "${1:-}" = display-message ]; then
    "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" "$@" | tee -a "$MX_UPGRADE_TMUX_DIR/probes"
    exit "${PIPESTATUS[0]}"
  fi
  if [ "${1:-}" = kill-window ] && [ -f "$MX_UPGRADE_TMUX_SOCKET.fail" ]; then exit 99; fi
  exec "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" "$@"
fi
''',1)
p.write_text(s)
PY
  "$MX_UPGRADE_REAL_TMUX" -f /dev/null -S "$MX_UPGRADE_TMUX_SOCKET" new-session -d -s "$MX_UPGRADE_TMUX_SESSION" -n fixture-sentinel 'sleep 300'
}
create_upgrade_real_window() {
  "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" new-window -d -t "$MX_UPGRADE_TMUX_SESSION" -n "mx-$standing" -c "$standing_worktree" 'sleep 300'
  "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" set-window-option -t "$MX_UPGRADE_TMUX_SESSION:mx-$standing" automatic-rename off
  "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" set-window-option -t "$MX_UPGRADE_TMUX_SESSION:mx-$standing" allow-rename off
}
upgrade_real_window_id() {
  "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" list-windows -t "=$MX_UPGRADE_TMUX_SESSION" -F '#{window_name} #{window_id}' |
    awk -v name="mx-$standing" '$1==name {print $2; found=1} END {if (!found) exit 1}'
}
finish_upgrade_real_tmux() {
  "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" kill-session -t "$MX_UPGRADE_TMUX_SESSION"
  rm -rf "$MX_UPGRADE_TMUX_DIR"
  unset MX_UPGRADE_TMUX_SOCKET MX_UPGRADE_REAL_TMUX MX_UPGRADE_TMUX_DIR MX_UPGRADE_TMUX_SESSION
}
run_upgrade_owned_stop_checks() {
  setup_upgrade_real_tmux
  create_upgrade_real_window
  local before output
  before=$(upgrade_real_window_id)
  for answer in n EOF; do
    if output=$(run_tty_confirmation "$answer" env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
        --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail "live stop accepted $answer"; fi
    assert_contains "$output" 'STOP task standing-upgrade' "live preview omitted exact task (recorded: $(sed -n 's/^window=//p' "$standing_meta"), cwd: $standing_worktree, probes: $(cat "$MX_UPGRADE_TMUX_DIR/probes" 2>/dev/null))"
    assert_contains "$output" 'interrupt active work' 'live preview omitted interruption warning'
    [ "$(upgrade_real_window_id)" = "$before" ] || fail 'negative consent changed live endpoint'
    cmp -s "$TMP_ROOT/standing-meta.before" "$standing_meta" || fail 'negative consent changed metadata'
  done
  if upgrade_standing --allow-stopped-agents </dev/null >"$TMP_ROOT/owned-stop-no-optin.out" 2>"$TMP_ROOT/owned-stop-no-optin.err"; then fail 'old stopped flag authorized a live stop'; fi
  assert_grep 'stop-managed-sessions' "$TMP_ROOT/owned-stop-no-optin.err" 'live noninteractive refusal omitted opt-in'
  [ "$(upgrade_real_window_id)" = "$before" ] || fail 'old flag stopped a live endpoint'

  # Recycle the managed name in the exact allocation after consent preview.
  # A different immutable ID/process lifetime must fail before any stop.
  cat >"$TMP_ROOT/recycle-owned-window" <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail
"$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" kill-window -t "$MX_UPGRADE_TMUX_SESSION:mx-standing-upgrade"
"$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" new-window -d -t "$MX_UPGRADE_TMUX_SESSION" -n mx-standing-upgrade -c "$MX_UPGRADE_TEST_CWD" 'sleep 300'
"$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" set-window-option -t "$MX_UPGRADE_TMUX_SESSION:mx-standing-upgrade" automatic-rename off
"$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" set-window-option -t "$MX_UPGRADE_TMUX_SESSION:mx-standing-upgrade" allow-rename off
HOOK
  chmod +x "$TMP_ROOT/recycle-owned-window"
  export MX_TEST_CONFIRM_HOOK="$TMP_ROOT/recycle-owned-window" MX_UPGRADE_TEST_CWD="$standing_worktree"
  if output=$(run_tty_confirmation y env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail 'upgrade stopped recycled endpoint'; fi
  unset MX_TEST_CONFIRM_HOOK MX_UPGRADE_TEST_CWD
  assert_contains "$output" 'changed after upgrade consent' 'recycled name lacked changed-identity diagnostic'
  [ "$(upgrade_real_window_id)" != "$before" ] || fail 'fixture did not recycle exact named endpoint'

  # A verified stop whose backend operation fails cannot publish runtime files.
  : >"$MX_UPGRADE_TMUX_SOCKET.fail"
  if upgrade_standing --stop-managed-sessions --allow-stopped-agents >"$TMP_ROOT/owned-stop-failed.out" 2>"$TMP_ROOT/owned-stop-failed.err"; then fail 'failed exact stop allowed upgrade'; fi
  assert_grep 'stop/reconciliation failed' "$TMP_ROOT/owned-stop-failed.err" 'failed stop omitted retry guidance'
  upgrade_real_window_id >/dev/null || fail 'failed stop fixture removed endpoint'
  rm "$MX_UPGRADE_TMUX_SOCKET.fail"
  # Combine live and stale users in a single positive confirmation.
  printf '{"schema":"mx-workspace-launch.v1","owner":{"pid":%s,"started":"retired-lifetime"}}\n' "$$" >"$install/data/home/state/workspace-launch.json"
  cp "$install/data/home/state/workspace-launch.json" "$standing_home/state/workspace-launch.json"
  if output=$(run_tty_confirmation y env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then :; else fail "verified real stop upgrade failed: $output"; fi
  [ "$(printf '%s' "$output" | python3 -c 'import sys; print(sys.stdin.read().count("[y/N]"))')" = 1 ] || fail 'live plus stale upgrade prompted more than once'
  if upgrade_real_window_id >/dev/null 2>&1; then fail 'positive consent retained live endpoint'; fi
  [ ! -e "$standing_home/state/workspace-launch.json" ] || fail 'owned child stale launch was not reconciled'
  "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" display-message -p -t "$MX_UPGRADE_TMUX_SESSION:fixture-sentinel" '#{window_id}' >/dev/null || fail 'upgrade touched unrelated fixture window'
  cmp -s "$TMP_ROOT/standing-meta.before" "$standing_meta" || fail 'stop changed task metadata/history'
  cmp -s "$TMP_ROOT/standing-lease.before" "$install/data/home/data/.home-allocation-$standing.json" || fail 'stop changed persistent home lease'
  [ "$(cat "$standing_worktree/retained-untracked")" = 'dirty standing work' ] || fail 'stop discarded dirty files'
  finish_upgrade_real_tmux
  pass 'owned live sessions confirm once; no and EOF preserve them; recycled identities and failed stops refuse replacement'
}
run_upgrade_retired_stop_check() {
  setup_upgrade_real_tmux
  create_upgrade_real_window
  upgrade_standing --stop-managed-sessions --allow-stopped-agents >/dev/null || fail 'verified retired receipt-only endpoint was not stopped'
  if upgrade_real_window_id >/dev/null 2>&1; then fail 'retired receipt-only endpoint remains live'; fi
  cmp -s "$TMP_ROOT/retired-action.before" "$install/data/home/state/.spawn-actions/$standing.json" || fail 'retired stop changed historical launch receipt'
  [ "$(cat "$retired_home/data/retained-private")" = 'private pending work' ] || fail 'retired stop lost archived private data'
  finish_upgrade_real_tmux
  pass 'live receipt-only retired execution stops without deleting its original ownership history'
}
run_upgrade_herdr_shape_check() {
  local action="$install/data/home/state/.spawn-actions/$standing.json" output
  cp "$action" "$TMP_ROOT/herdr-action.before"
  cp "$standing_meta" "$TMP_ROOT/herdr-meta.before"
  export MX_UPGRADE_HERDR_FIXTURE="$TMP_ROOT/upgrade-herdr" MX_UPGRADE_TEST_CWD="$standing_home" MX_UPGRADE_NATIVE_PID="$$"
  mkdir "$MX_UPGRADE_HERDR_FIXTURE"
  cat >"$MX_UPGRADE_HERDR_FIXTURE/herdr" <<'PY'
#!/usr/bin/env python3
import json, os, pathlib, sys
fixture=pathlib.Path(os.environ['MX_UPGRADE_HERDR_FIXTURE'])
args=sys.argv[1:]
cmd=tuple(args[:2])
pane={'pane_id':'w-test:p1','tab_id':'w-test:t1','workspace_id':'w-test','terminal_id':'term_fixture_stable','foreground_cwd':os.environ['MX_UPGRADE_TEST_CWD'],'cwd':os.environ['MX_UPGRADE_TEST_CWD'],'agent_status':'unknown','revision':6}
projected=(fixture/'projection').exists()
active=(fixture/'active').exists()
moved=(fixture/'focus-moved').exists()
if cmd==('pane','get'):
    result={'error':{'code':'pane_not_found'}} if (fixture/'gone').exists() or args[2]!='w-test:p1' else {'result':{'pane':pane}}
elif cmd==('pane','process-info'):
    result={'result':{'process_info':{'pane_id':'w-test:p1','shell_pid':int(os.environ['MX_UPGRADE_NATIVE_PID'])}}}
elif cmd==('workspace','list'):
    spaces=[{'workspace_id':'w-test','label':'agent-'+os.environ.get('MX_UPGRADE_TASK_LABEL','standing-upgrade')}]
    if projected:
        spaces=[{'workspace_id':'parent','label':'agent-standing-upgrade','focused':not active and not moved,'active_tab_id':'parent:t1'}, {'workspace_id':'w-test','label':'└ standing-upgrade · p:abcdefghijklmnopqrstuv','focused':active,'active_tab_id':'w-test:t1'}, {'workspace_id':'other','label':'unrelated','focused':moved,'active_tab_id':'other:t1'}]
    result={'result':{'workspaces':spaces}}
elif cmd==('tab','list'):
    workspace=args[args.index('--workspace')+1] if '--workspace' in args else 'w-test'
    result={'result':{'tabs':[{'tab_id':workspace+':t1','label':'mx-'+os.environ.get('MX_UPGRADE_TASK_LABEL','standing-upgrade') if workspace=='w-test' else 'unrelated','focused':active if workspace=='w-test' else (moved if workspace=='other' else not active and not moved)}]}}
elif cmd==('tab','get'):
    result={'result':{'tab':{'tab_id':'parent:t1','workspace_id':'parent'}}}
elif cmd==('tab','focus'):
    if args[2]!='parent:t1':sys.exit(98)
    (fixture/'focus-moved').unlink(missing_ok=True)
    (fixture/'focus-restored').write_text('parent:t1')
    result={'result':{}}
elif cmd==('session','list'):
    result={'sessions':[{'name':'default','running':True,'socket_path':str(fixture/'default.sock')}]}
elif cmd==('pane','list'):
    result={'result':{'panes':[] if (fixture/'gone').exists() else [pane]}}
elif cmd==('agent','get'):
    result={'error':{'code':'agent_not_found'}}
elif cmd==('pane','close'):
    (fixture/'close.log').write_text(' '.join(args))
    (fixture/'gone').touch()
    if projected:(fixture/'focus-moved').touch()
    result={'result':{}}
else:
    print(json.dumps({'error':{'code':'unsupported_fixture_call'}}))
    sys.exit(99)
print(json.dumps(result))
PY
  chmod +x "$MX_UPGRADE_HERDR_FIXTURE/herdr"
  export MX_HERDR_BIN="$MX_UPGRADE_HERDR_FIXTURE/herdr"
  python3 - "$standing_meta" "$action" "$standing_home" <<'PY'
import json, pathlib, sys
meta,action,home=map(pathlib.Path,sys.argv[1:])
rows=[]
for line in meta.read_text().splitlines():
    key,value=line.split('=',1)
    if key=='canonical_model':
        record=json.loads(value)
        record['runtime']['provider']='herdr'
        record['runtime']['endpoint']='default:w-test:p1'
        record['allocation']=None
        value=json.dumps(record,separators=(',',':'))
    elif key=='window':value='default:w-test:p1'
    elif key=='backend':value='herdr'
    rows.append(key+'='+value)
if not any(row.startswith('backend=') for row in rows):rows.append('backend=herdr')
meta.write_text('\n'.join(rows)+'\n')
a=json.loads(action.read_text());a['backend']='herdr';a['binding']['runtime']['provider']='herdr';a['binding']['allocation']=None;a['worktree']=str(home);a['endpoint']='default:w-test:p1';action.write_text(json.dumps(a)+'\n')
PY
  if output=$(run_tty_confirmation n env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail 'Herdr stop accepted default No'; fi
  assert_contains "$output" 'STOP task standing-upgrade: herdr endpoint default:w-test:p1' 'actual Herdr response shape/private-home route was not eligible'
  [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/close.log" ] || fail 'Herdr No sent close'
  run_upgrade_herdr_lock_checks
  upgrade_standing --stop-managed-sessions --allow-stopped-agents >/dev/null || fail 'actual Herdr shape with no pane-get shell_pid blocked safe stop'
  [ -e "$MX_UPGRADE_HERDR_FIXTURE/gone" ] || fail 'Herdr exact pane stop was not observed'
  run_upgrade_herdr_projection_checks
  run_upgrade_reused_endpoint_checks
  cp "$TMP_ROOT/herdr-action.before" "$action"
  cp "$TMP_ROOT/herdr-meta.before" "$standing_meta"
  unset MX_HERDR_BIN MX_UPGRADE_HERDR_FIXTURE MX_UPGRADE_TEST_CWD MX_UPGRADE_NATIVE_PID
  pass 'Herdr terminal/process-info proof follows private execution homes and allocation-null records'
}
upgrade_runtime_identity() {
  python3 - "$install/data/runtime/AGENTS.md" "$install/bin/multplx" <<'PY'
import pathlib,sys
for name in sys.argv[1:]:
 s=pathlib.Path(name).stat();print(s.st_dev,s.st_ino,s.st_mtime_ns,s.st_size)
PY
}
run_upgrade_herdr_lock_checks() {
  local output before native_pid
  before=$(upgrade_runtime_identity)
  mkdir -p "$TMP_ROOT/upgrade-herdr"
  cat >"$TMP_ROOT/upgrade-herdr/codex" <<'NATIVE'
#!/usr/bin/env bash
sleep 300 &
owned_child=$!
trap 'kill "$owned_child" 2>/dev/null || true; wait "$owned_child" 2>/dev/null || true; exit 0' TERM
wait "$owned_child"
NATIVE
  chmod +x "$TMP_ROOT/upgrade-herdr/codex"
  "$TMP_ROOT/upgrade-herdr/codex" &
  native_pid=$!
  export MX_UPGRADE_PRIMARY_PID="$native_pid" MX_UPGRADE_NATIVE_PID="$native_pid"
  if [ -e "$standing_home/state/.lock" ]; then cp "$standing_home/state/.lock" "$TMP_ROOT/herdr-home-lock.before"; fi
  printf '%s\n' "$native_pid" >"$standing_home/state/.lock"
  if output=$(run_tty_confirmation n env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail 'owned native home lock accepted No'; fi
  assert_contains "$output" 'STOP task standing-upgrade' 'exact task process did not account for owned home lock'
  [ "$(cat "$standing_home/state/.lock")" = "$native_pid" ] || fail 'No changed owned home lock'
  kill -0 "$native_pid" || fail 'No stopped owned native fixture'
  python3 - "$standing_home/state/workspace-launch.json" "$$" <<'PY'
import json,pathlib,subprocess,sys
p=pathlib.Path(sys.argv[1]);pid=int(sys.argv[2]);stat=pathlib.Path(f'/proc/{pid}/stat')
started='linux-starttime='+stat.read_text().rsplit(')',1)[1].split()[19] if stat.exists() else ' '.join(subprocess.check_output(['ps','-p',str(pid),'-o','lstart='],env={'LC_ALL':'C','PATH':'/bin:/usr/bin'}).decode().split())
p.write_text(json.dumps({'schema':'mx-workspace-launch.v1','owner':{'pid':pid,'started':started}})+'\n')
PY
  cp "$standing_home/state/workspace-launch.json" "$TMP_ROOT/independent-launch.before"
  if upgrade_standing --stop-managed-sessions --allow-stopped-agents >"$TMP_ROOT/herdr-independent.out" 2>"$TMP_ROOT/herdr-independent.err"; then fail 'owned task lock hid an independent workspace launch'; fi
  assert_grep 'independent workspace launch' "$TMP_ROOT/herdr-independent.err" 'independent home launcher refusal omitted ownership route'
  cmp -s "$TMP_ROOT/independent-launch.before" "$standing_home/state/workspace-launch.json" || fail 'refusal changed independent launch receipt'
  [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/close.log" ] || fail 'independent launch refusal sent close'
  [ "$(upgrade_runtime_identity)" = "$before" ] || fail 'home lock refusal replaced runtime'
  rm "$standing_home/state/workspace-launch.json" "$standing_home/state/.lock"
  if [ -e "$TMP_ROOT/herdr-home-lock.before" ]; then cp "$TMP_ROOT/herdr-home-lock.before" "$standing_home/state/.lock"; fi
  kill "$native_pid"
  wait "$native_pid" || true
  unset MX_UPGRADE_PRIMARY_PID
  export MX_UPGRADE_NATIVE_PID="$$"
  pass 'exact native task accounts for its home lock, while independent workspace launch refuses without mutation'
}
run_upgrade_herdr_projection_checks() {
  local journal="$install/data/home/state/$standing.herdr-presentation" lock_path before
  rm "$MX_UPGRADE_HERDR_FIXTURE/gone" "$MX_UPGRADE_HERDR_FIXTURE/close.log"
  : >"$MX_UPGRADE_HERDR_FIXTURE/projection"
  python3 - "$journal" "$standing_home" <<'PY'
import pathlib,sys
p,home=map(pathlib.Path,sys.argv[1:])
p.write_text('version=2\ntask_id=standing-upgrade\nprojection_id=abcdefghijklmnopqrstuv\nhome='+str(home)+'\nsession=default\nworkspace_id=w-test\ntab_id=w-test:t1\npane_id=w-test:p1\nparent_workspace_id=parent\nparent_label=agent-standing-upgrade\nworkspace_label=└ standing-upgrade · p:abcdefghijklmnopqrstuv\ntask_label=mx-standing-upgrade\n')
PY
  cp "$journal" "$TMP_ROOT/projected-journal.before"
  before=$(upgrade_runtime_identity)
  : >"$MX_UPGRADE_HERDR_FIXTURE/active"
  if upgrade_standing --stop-managed-sessions --allow-stopped-agents >"$TMP_ROOT/projected-active.out" 2>"$TMP_ROOT/projected-active.err"; then fail 'active projected task was stopped'; fi
  assert_grep 'active tab' "$TMP_ROOT/projected-active.err" 'active projected refusal omitted focus guidance'
  [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/close.log" ] || fail 'active projected refusal closed a pane'
  rm "$MX_UPGRADE_HERDR_FIXTURE/active"
  lock_path=$(python3 - "$MX_UPGRADE_HERDR_FIXTURE" <<'PY'
import hashlib,pathlib,sys
socket=pathlib.Path(sys.argv[1]).resolve()/'default.sock'
print('/tmp/broker-herdr-presentation/order-'+hashlib.sha256(b'default\0'+str(socket).encode()).hexdigest()[:32]+'.lock')
PY
)
  if [ ! -d /tmp/broker-herdr-presentation ]; then mkdir -m 700 /tmp/broker-herdr-presentation; fi
  mkdir "$lock_path"
  export MX_UPGRADE_PRESENTATION_LOCK="$lock_path"
  printf '%s\n' "$$" >"$lock_path/pid"
  if upgrade_standing --stop-managed-sessions --allow-stopped-agents >"$TMP_ROOT/projected-busy.out" 2>"$TMP_ROOT/projected-busy.err"; then fail 'busy projected session allowed stop'; fi
  rm "$lock_path/pid"
  rmdir "$lock_path"
  unset MX_UPGRADE_PRESENTATION_LOCK
  assert_grep 'presentation session is busy' "$TMP_ROOT/projected-busy.err" 'projected stop bypassed its shared session owner'
  [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/close.log" ] || fail 'busy projected refusal closed a pane'
  [ "$(upgrade_runtime_identity)" = "$before" ] || fail 'projected refusal replaced runtime'
  upgrade_standing --stop-managed-sessions --allow-stopped-agents >/dev/null || fail 'verified projected stop failed'
  cmp -s "$TMP_ROOT/projected-journal.before" "$journal" || fail 'projected stop deleted or rewrote presentation history'
  [ "$(cat "$MX_UPGRADE_HERDR_FIXTURE/focus-restored")" = parent:t1 ] || fail 'projected stop did not restore exact unrelated focus'
  [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/focus-moved" ] || fail 'projected stop left unrelated focus changed'
  # A successor journal cannot hide an independently owned old flat receipt.
  rm "$MX_UPGRADE_HERDR_FIXTURE/projection" "$MX_UPGRADE_HERDR_FIXTURE/gone" "$MX_UPGRADE_HERDR_FIXTURE/close.log"
  sed 's/pane_id=w-test:p1/pane_id=w-successor:p1/' "$TMP_ROOT/projected-journal.before" >"$journal"
  cp "$journal" "$TMP_ROOT/successor-journal.before"
  upgrade_standing --stop-managed-sessions --allow-stopped-agents >/dev/null || fail 'successor journal hid historical flat execution'
  cmp -s "$TMP_ROOT/successor-journal.before" "$journal" || fail 'historical flat stop changed successor journal'
  rm "$journal"
  pass 'projected Herdr stops honor active focus and shared locks; exact close restores focus and retains original/successor journals'
}
run_upgrade_primary_stop_check() {
  local pid output
  mkdir -p "$TMP_ROOT/upgrade-primary"
  cat >"$TMP_ROOT/upgrade-primary/codex" <<'PRIMARY'
#!/usr/bin/env bash
sleep 300 &
owned_child=$!
trap 'kill "$owned_child" 2>/dev/null || true; wait "$owned_child" 2>/dev/null || true; exit 0' TERM
wait "$owned_child"
PRIMARY
  chmod +x "$TMP_ROOT/upgrade-primary/codex"
  "$TMP_ROOT/upgrade-primary/codex" &
  pid=$!
  MX_UPGRADE_PRIMARY_PID=$pid
  export MX_UPGRADE_PRIMARY_PID
  python3 - "$install/data/home" "$pid" <<'PY'
import json, pathlib, subprocess, sys
home=pathlib.Path(sys.argv[1]).resolve();pid=int(sys.argv[2]);state=home/'state'
if pathlib.Path(f'/proc/{pid}/stat').exists():
    start=pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()[19]
    started='linux-starttime='+start
else:
    started=' '.join(subprocess.check_output(['ps','-p',str(pid),'-o','lstart='],env={'LC_ALL':'C','PATH':'/bin:/usr/bin'}).decode().split())
owner={'pid':pid,'started':started}
(state/'.lock').write_text(str(pid)+'\n')
(state/'workspace-connection.json').write_text(json.dumps({'schema':'mx-workspace-connection.v1','owner':owner,'harness':'codex','caller_cwd':str(home),'home':str(home),'state':str(state),'backend':None,'target':None,'pane':None,'tmux_socket':None}))
(state/'workspace-launch.json').write_text(json.dumps({'schema':'mx-workspace-launch.v1','owner':owner}))
PY
  if output=$(run_tty_confirmation n env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail 'primary stop accepted No'; fi
  assert_contains "$output" "STOP primary/workspace launch pid $pid" 'primary preview omitted exact PID/home'
  kill -0 "$pid" || fail 'No stopped fixture primary'
  if output=$(run_tty_confirmation y env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then :; else fail "verified primary stop failed: $output"; fi
  wait "$pid" || true
  unset MX_UPGRADE_PRIMARY_PID
  [ ! -e "$install/data/home/state/workspace-launch.json" ] || fail 'stopped primary launch reservation not reconciled'
  [ -f "$install/data/home/state/workspace-connection.json" ] || fail 'primary stop discarded connection history'
  pass 'registered primary stop previews exact lifetime and reconciles its reservation without discarding history'
}

run_upgrade_reused_endpoint_checks() {
  local output root_state="$install/data/home/state" owner=endpoint-owner nested=interrupted-child
  local owner_home="$TMP_ROOT/endpoint-owner-home"
  cp "$standing_meta" "$TMP_ROOT/reuse-standing.before"
  mkdir -p "$owner_home/state" "$owner_home/data" "$owner_home/config"
  printf 'endpoint-owner\n' >"$owner_home/.mx-agent-home"
  export MX_UPGRADE_TASK_LABEL="$owner" MX_UPGRADE_TEST_CWD="$owner_home"
  rm -f "$MX_UPGRADE_HERDR_FIXTURE/gone" "$MX_UPGRADE_HERDR_FIXTURE/close.log"
  python3 - "$standing_meta" "$owner_home" "$standing_home" <<'PY_REUSE'
import copy,json,pathlib,sys
meta=pathlib.Path(sys.argv[1]);owner_home=pathlib.Path(sys.argv[2]);child_home=pathlib.Path(sys.argv[3]);state=meta.parent
current=json.loads(next(l.split('=',1)[1] for l in meta.read_text().splitlines() if l.startswith('canonical_model=')))
def write(path,r):
 path.write_text('schema_version=2\ncanonical_model='+json.dumps(r,separators=(',',':'))+'\nbackend=herdr\n'+('window='+r['runtime']['endpoint']+'\n' if r['runtime']['endpoint'] else ''))
# Completed old record: its historical receipt still names the reused pane.
current['schedule']['state']='completed';write(meta,current)
owner=copy.deepcopy(current);owner.update(task_id='endpoint-owner',persistent=False,private_home=False,home_allocation=None,persistent_home=str(owner_home))
owner['attempt']['id']='attempt-endpoint-owner';owner['schedule']['state']='running';write(state/'endpoint-owner.meta',owner)
# A child in a different owner state claims the same recycled ID, but its exact
# released root admission proves the original execution is already gone.
nested=copy.deepcopy(current);nested.update(task_id='interrupted-child',persistent=False,private_home=False,home_allocation=None,persistent_home=str(child_home/'nested-cwd'),owner_home=str(child_home),owner_state=str(child_home/'state'),parent_id='standing-upgrade',parent_home=str(state.parent),parent_state=str(state))
pathlib.Path(nested['persistent_home']).mkdir()
nested['attempt']['id']='attempt-interrupted-child';nested['schedule']['state']='running'
nested['schedule']['decisions']=[{'id':'retained-choice','task_id':'interrupted-child','brief_revision':nested['accepted_brief_revision'],'workflow_revision':None,'question':'Retain this unresolved choice','answer':None}]
write(child_home/'state/interrupted-child.meta',nested)
(child_home/'state/interrupted-child.status').write_text('running: interrupted fixture\n')
a={'version':1,'request_id':'custom-child-request','task_id':'interrupted-child','binding':nested,'backend':'herdr','endpoint':nested['runtime']['endpoint'],'worktree':nested['persistent_home'],'stage':'running','detail':None}
(child_home/'state/.spawn-actions/custom-child-request.json').write_text(json.dumps(a))
admission=json.loads((state/'.admissions/standing-upgrade.json').read_text())
admission.update(request_id='custom-child-request',task_id='interrupted-child',owner_home=str(child_home),owner_state=str(child_home/'state'),attempt_id=nested['attempt']['id'],backend='herdr',endpoint=nested['runtime']['endpoint'],allocation_id=None,state='released',owner={'pid':999999999,'marker':'absent-original-creator'})
(state/'.admissions/custom-child-request.json').write_text(json.dumps(admission))
PY_REUSE
  cp "$standing_home/state/$nested.meta" "$TMP_ROOT/reuse-nested.before"
  cp "$root_state/.admissions/custom-child-request.json" "$TMP_ROOT/reuse-admission.before"
  for answer in n EOF; do
    if output=$(run_tty_confirmation "$answer" env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
        --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail 'reused endpoint accepted negative consent'; fi
    assert_contains "$output" 'historical endpoint default:w-test:p1 for completed task standing-upgrade' 'completed reused endpoint was not recognized'
    assert_contains "$output" 'INTERRUPT task interrupted-child' 'released nested execution did not reach consent'
    assert_contains "$output" 'STOP task endpoint-owner' 'current pane owner omitted from preview'
    if printf '%s' "$output" | grep -q 'STOP task standing-upgrade\|STOP task interrupted-child'; then fail 'stale task gained stop authority'; fi
    [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/close.log" ] || fail 'negative consent stopped reused pane'
    cmp -s "$TMP_ROOT/reuse-nested.before" "$standing_home/state/$nested.meta" || fail 'negative consent changed interrupted task'
  done
  cp "$root_state/$owner.meta" "$TMP_ROOT/reuse-owner.before"
  cat >"$TMP_ROOT/change-reused-claimant" <<'HOOK'
#!/usr/bin/env python3
import json,os,pathlib
p=pathlib.Path(os.environ['MX_UPGRADE_OWNER_META']);rows=p.read_text().splitlines()
for i,line in enumerate(rows):
 if line.startswith('canonical_model='):
  r=json.loads(line.split('=',1)[1]);r['schedule']['priority']+=1;rows[i]='canonical_model='+json.dumps(r,separators=(',',':'))
p.write_text('\n'.join(rows)+'\n')
HOOK
  chmod +x "$TMP_ROOT/change-reused-claimant"
  export MX_TEST_CONFIRM_HOOK="$TMP_ROOT/change-reused-claimant" MX_UPGRADE_OWNER_META="$root_state/$owner.meta"
  if output=$(run_tty_confirmation y env -u MX_HOME -u MX_ROOT_OVERRIDE -u MX_STATE_OVERRIDE -u MX_CONFIG_OVERRIDE "$package/bin/mx" launcher-install --upgrade --package "$package" \
      --bin-dir "$install/bin" --config-dir "$install/config" --data-dir "$install/data" 2>&1); then fail 'changed reused claimant accepted old consent'; fi
  unset MX_TEST_CONFIRM_HOOK MX_UPGRADE_OWNER_META
  assert_contains "$output" 'changed after upgrade consent' 'changed reused claimant lacked retry diagnostic'
  [ ! -e "$MX_UPGRADE_HERDR_FIXTURE/close.log" ] || fail 'changed claimant stopped reused pane'
  cmp -s "$TMP_ROOT/reuse-nested.before" "$standing_home/state/$nested.meta" || fail 'changed claimant consent altered nested task'
  cp "$TMP_ROOT/reuse-owner.before" "$root_state/$owner.meta"
  jq '.state="active"' "$TMP_ROOT/reuse-admission.before" >"$root_state/.admissions/custom-child-request.json"
  if upgrade_standing --allow-stopped-agents --stop-managed-sessions >/dev/null 2>&1; then fail 'unreleased reused execution accepted'; fi
  cp "$TMP_ROOT/reuse-admission.before" "$root_state/.admissions/custom-child-request.json"
  cp "$root_state/$owner.meta" "$TMP_ROOT/reuse-owner.before"
  printf 'corrupt claimant\n' >"$root_state/$owner.meta"
  if upgrade_standing --allow-stopped-agents --stop-managed-sessions >/dev/null 2>&1; then fail 'corrupt current claimant accepted'; fi
  cp "$TMP_ROOT/reuse-owner.before" "$root_state/$owner.meta"
  # The real exact proof must reject a different allocation directory.
  export MX_UPGRADE_TEST_CWD="$standing_home"
  if upgrade_standing --allow-stopped-agents --stop-managed-sessions >/dev/null 2>&1; then fail 'mismatched current claimant directory accepted'; fi
  export MX_UPGRADE_TEST_CWD="$owner_home"
  upgrade_standing --allow-stopped-agents --stop-managed-sessions >/dev/null || fail 'proven reused nested execution refused consent'
  python3 - "$standing_home/state/$nested.meta" "$TMP_ROOT/reuse-nested.before" <<'PY_VERIFY'
import json,pathlib,sys
def model(p):return json.loads(next(l.split('=',1)[1] for l in pathlib.Path(p).read_text().splitlines() if l.startswith('canonical_model=')))
a,b=map(model,sys.argv[1:]);assert a['runtime']['endpoint'] is None
assert a['schedule']['state']=='waiting-human' and a['schedule']['waiting_condition']=='retained-choice'
for field in ['attempt','briefs','allocation','delivery','prior_attempts']:assert a[field]==b[field],field
assert a['schedule']['decisions']==b['schedule']['decisions']
PY_VERIFY
  assert_grep 'failed: upgrade' "$standing_home/state/$nested.status" 'interrupted task lacks failed disposition'
  upgrade_standing --allow-stopped-agents >/dev/null || fail 'repeat upgrade rejected reconciled historical child receipt'
  [ -f "$standing_home/state/.spawn-actions/custom-child-request.json" ] || fail 'reconciliation discarded historical launch receipt'
  rm "$root_state/$owner.meta" "$root_state/.admissions/custom-child-request.json" "$standing_home/state/$nested.meta" "$standing_home/state/$nested.status" "$standing_home/state/.spawn-actions/custom-child-request.json"
  # Fixture-only cleanup retains the originals in TMP_ROOT until test completion.
  rm "$standing_home/state"/.upgrade-reconcile.interrupted-child.*.json "$standing_home/state/.transitions"/upgrade-interrupt-interrupted-child-*.json
  cp "$TMP_ROOT/reuse-standing.before" "$standing_meta"
  export MX_UPGRADE_TEST_CWD="$standing_home"
  unset MX_UPGRADE_TASK_LABEL
  pass 'reused endpoint uses exact current owner; released nested interruption confirms, retains decisions/history, and permits repeat upgrade'
}
