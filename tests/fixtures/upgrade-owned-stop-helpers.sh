#!/usr/bin/env bash
# Isolated upgrade acceptance: real tmux sessions belong only to this fixture.
setup_upgrade_real_tmux() {
  trap 'if [ -n "${MX_UPGRADE_TMUX_SOCKET:-}" ]; then "$MX_UPGRADE_REAL_TMUX" -S "$MX_UPGRADE_TMUX_SOCKET" kill-session -t "$MX_UPGRADE_TMUX_SESSION" 2>/dev/null || true; fi; if [ -n "${MX_UPGRADE_TMUX_DIR:-}" ]; then rm -rf "$MX_UPGRADE_TMUX_DIR"; fi; if [ -n "${MX_UPGRADE_PRIMARY_PID:-}" ]; then kill "$MX_UPGRADE_PRIMARY_PID" 2>/dev/null || true; wait "$MX_UPGRADE_PRIMARY_PID" 2>/dev/null || true; fi; mx_test_cleanup' EXIT
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
if cmd==('pane','get'):
    result={'error':{'code':'pane_not_found'}} if (fixture/'gone').exists() else {'result':{'pane':pane}}
elif cmd==('pane','process-info'):
    result={'result':{'process_info':{'pane_id':'w-test:p1','shell_pid':int(os.environ['MX_UPGRADE_NATIVE_PID'])}}}
elif cmd==('workspace','list'):
    result={'result':{'workspaces':[{'workspace_id':'w-test','label':'agent-standing-upgrade'}]}}
elif cmd==('tab','list'):
    result={'result':{'tabs':[{'tab_id':'w-test:t1','label':'mx-standing-upgrade'}]}}
elif cmd==('pane','list'):
    result={'result':{'panes':[] if (fixture/'gone').exists() else [pane]}}
elif cmd==('agent','get'):
    result={'error':{'code':'agent_not_found'}}
elif cmd==('pane','close'):
    (fixture/'close.log').write_text(' '.join(args))
    (fixture/'gone').touch()
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
  upgrade_standing --stop-managed-sessions --allow-stopped-agents >/dev/null || fail 'actual Herdr shape with no pane-get shell_pid blocked safe stop'
  [ -e "$MX_UPGRADE_HERDR_FIXTURE/gone" ] || fail 'Herdr exact pane stop was not observed'
  cp "$TMP_ROOT/herdr-action.before" "$action"
  cp "$TMP_ROOT/herdr-meta.before" "$standing_meta"
  unset MX_HERDR_BIN MX_UPGRADE_HERDR_FIXTURE MX_UPGRADE_TEST_CWD MX_UPGRADE_NATIVE_PID
  pass 'Herdr terminal/process-info proof follows private execution homes and allocation-null records'
}
run_upgrade_primary_stop_check() {
  local pid output
  cat >"$TMP_ROOT/upgrade-primary-codex" <<'PRIMARY'
#!/usr/bin/env bash
sleep 300 &
owned_child=$!
trap 'kill "$owned_child" 2>/dev/null || true; wait "$owned_child" 2>/dev/null || true; exit 0' TERM
wait "$owned_child"
PRIMARY
  chmod +x "$TMP_ROOT/upgrade-primary-codex"
  "$TMP_ROOT/upgrade-primary-codex" &
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
