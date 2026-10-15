#!/usr/bin/env bash
# Phase 11 configured discovery, local reuse and durable task intake.
set -u

# shellcheck source=tests/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TMP_ROOT=$(mx_test_tmproot workspace-discovery)
TMP_ROOT=$(cd "$TMP_ROOT" && pwd -P)
HOME_FIXTURE="$TMP_ROOT/home"
DEV_ROOT="$TMP_ROOT/dev root"
REPO="$DEV_ROOT/team/app"
PLAIN="$DEV_ROOT/plain"
mkdir -p "$HOME_FIXTURE/state" "$HOME_FIXTURE/data" "$HOME_FIXTURE/config" "$PLAIN"
printf '{}\n' >"$PLAIN/package.json"
mx_git_init_commit "$REPO"
printf 'local and untouched\n' >"$REPO/working-file"

mx_cli() {
  MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$HOME_FIXTURE" \
    MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
    MX_CONFIG_OVERRIDE="$HOME_FIXTURE/config" \
    "${MX_RUST_BIN:-$ROOT/target/release/mx}" "$@"
}

mx_cli project roots add "$DEV_ROOT" --depth 5 >"$TMP_ROOT/root.json" || fail 'root add failed'
jq -e --arg root "$DEV_ROOT" '.roots == [{path:$root,max_depth:5}] and .follow_symlinks == false' \
  "$TMP_ROOT/root.json" >/dev/null || fail 'configured root contract differs'

mx_cli project discover --refresh >"$TMP_ROOT/discovery.json" || fail 'refresh failed'
jq -e --arg repo "$REPO" --arg plain "$PLAIN" '
  .status == "complete" and .completed_roots == 1 and
  any(.candidates[]; .canonical_path == $repo and .kind == "git" and .task_ready == true) and
  any(.candidates[]; .canonical_path == $plain and .kind == "unversioned" and .task_ready == false)
' "$TMP_ROOT/discovery.json" >/dev/null || fail 'nested Git and unversioned discovery differs'

mx_cli task --project "$REPO" --request-id login-request --batch-id release-batch \
  --task-id fix-login 'Fix login' >"$TMP_ROOT/receipt.json" || fail 'task intake failed'
jq -e '
  .schema == "mx-task-intake-receipt.v1" and .newly_accepted == true and
  .implementation_started == false and .working_changes == "present-and-excluded" and
  .route.kind == "main-orchestrator" and .request.task_id == "fix-login" and
  .request.batch_id == "release-batch" and .request.notification_event_id != null
' "$TMP_ROOT/receipt.json" >/dev/null || fail 'durable task receipt differs'
[ "$(cat "$REPO/working-file")" = 'local and untouched' ] || fail 'task intake changed working files'

printf 'next\n' >>"$REPO/file"
git -C "$REPO" add file
git -C "$REPO" -c user.name='Multplx Tests' -c user.email='tests@example.invalid' commit -qm next
mx_cli task --project "$REPO" --request-id login-request --batch-id release-batch \
  --task-id fix-login 'Fix login' >"$TMP_ROOT/retry.json" || fail 'identical retry failed after HEAD moved'
jq -e --arg start "$(jq -r '.request.starting_revision' "$TMP_ROOT/receipt.json")" '
  .newly_accepted == false and .request.starting_revision == $start
' "$TMP_ROOT/retry.json" >/dev/null || fail 'retry did not retain first binding'

if mx_cli task --project "$REPO" --request-id login-request --batch-id release-batch \
  --task-id fix-login 'Different request' >"$TMP_ROOT/conflict.out" 2>"$TMP_ROOT/conflict.err"; then
  fail 'changed retry was accepted'
fi
grep -F 'conflicts with durable submission intent' "$TMP_ROOT/conflict.err" >/dev/null \
  || fail 'changed retry did not explain conflict'

MX_ROOT_OVERRIDE="$ROOT" MX_HOME="$HOME_FIXTURE" \
  MX_STATE_OVERRIDE="$HOME_FIXTURE/state" MX_DATA_OVERRIDE="$HOME_FIXTURE/data" \
  MX_CONFIG_OVERRIDE="$HOME_FIXTURE/config" \
  "$ROOT/bin/mx-system-snapshot.sh" --json >"$TMP_ROOT/snapshot.json" \
  || fail 'canonical snapshot failed'
jq -e '
  any(.portfolio.tasks[]; .id == "fix-login" and .state == "queued-request" and
    .intake.request_id == "login-request" and .intake.implementation_started == false)
' "$TMP_ROOT/snapshot.json" >/dev/null || fail 'accepted intake missing from canonical portfolio'

if mx_cli task --project "$PLAIN" 'Cannot dispatch without Git' >"$TMP_ROOT/plain.out" 2>"$TMP_ROOT/plain.err"; then
  fail 'unversioned folder produced a task-ready binding'
fi
grep -E 'Git rev-parse|not a git repository' "$TMP_ROOT/plain.err" >/dev/null \
  || fail 'unversioned refusal did not retain its capability limit'

pass 'configured nested discovery, local reuse, immutable retry and canonical intake projection'

# Consent is an explicit human terminal action, never a nested model session.
mx_cli project register "$REPO" --alias consent-app >"$TMP_ROOT/registered.json" 2>"$TMP_ROOT/registered.err" \
  || fail 'consent fixture registration failed'
CHECKOUT=$(jq -r .checkout_id "$TMP_ROOT/registered.json")
grep -F "projects consent $CHECKOUT --harness codex" "$TMP_ROOT/registered.err" >/dev/null \
  || fail 'registration omitted exact consent command'
if mx_cli project consent consent-app --harness codex >"$TMP_ROOT/nonterminal.out" 2>"$TMP_ROOT/nonterminal.err"; then
  fail 'consent launched outside a foreground terminal'
fi
grep -F 'requires your foreground terminal' "$TMP_ROOT/nonterminal.err" >/dev/null \
  || fail 'nonterminal consent refusal was unclear'

export MX_CONSENT_BINARY="${MX_RUST_BIN:-$ROOT/target/release/mx}"
export MX_CONSENT_ROOT="$ROOT" MX_CONSENT_HOME="$HOME_FIXTURE" MX_CONSENT_REPO="$REPO" MX_CONSENT_TMP="$TMP_ROOT"
python3 - <<'PY' || exit 1
import errno, json, os, pathlib, pty, select, signal, time
base=pathlib.Path(os.environ['MX_CONSENT_TMP'])
repo=pathlib.Path(os.environ['MX_CONSENT_REPO'])
fake=base/'native codex'
record=base/'native-consent.json'
fake.write_text('''#!/usr/bin/env python3
import json,os,sys
with open(os.environ['MX_CONSENT_RECORD'],'w') as f:
 json.dump({'args':sys.argv[1:],'cwd':os.getcwd(),'idle':os.getenv('MX_CODEX_IDLE_CLI'),'thread':os.getenv('CODEX_THREAD_ID')},f)
sys.exit(int(os.getenv('MX_CONSENT_EXIT','0')))
''')
fake.chmod(0o700)
provider=base/'isolated provider'; provider.mkdir()
config=provider/'config.toml'; config.write_text('# unchanged provider configuration\n')
original=config.read_bytes()
env=dict(os.environ,MX_ROOT_OVERRIDE=os.environ['MX_CONSENT_ROOT'],MX_HOME=os.environ['MX_CONSENT_HOME'],
 MX_STATE_OVERRIDE=os.environ['MX_CONSENT_HOME']+'/state',MX_DATA_OVERRIDE=os.environ['MX_CONSENT_HOME']+'/data',
 MX_CONFIG_OVERRIDE=os.environ['MX_CONSENT_HOME']+'/config',MX_REAL_CODEX=str(fake),CODEX_HOME=str(provider),
 MX_CONSENT_RECORD=str(record),MX_CODEX_IDLE_CLI='1',CODEX_THREAD_ID='must-not-inherit')
def run(args, **overrides):
 record.unlink(missing_ok=True)
 pid,fd=pty.fork()
 if pid==0:
  binary=os.environ['MX_CONSENT_BINARY']
  os.execve(binary,[binary,'project',*args],dict(env,**overrides))
 output=b''; deadline=time.monotonic()+10; status=None
 try:
  while time.monotonic()<deadline:
   ready,_,_=select.select([fd],[],[],.05)
   if ready:
    try: chunk=os.read(fd,8192)
    except OSError as error:
     if error.errno!=errno.EIO: raise
     break
    if not chunk: break
    output+=chunk
  else:
   os.kill(pid,signal.SIGKILL)
   raise AssertionError('consent fixture did not exit')
 finally:
  os.close(fd); _,status=os.waitpid(pid,0)
 return os.waitstatus_to_exitcode(status),output.decode(errors='replace')
args=['consent','consent-app','--harness','codex']
code,output=run(args)
assert code==0,(code,output)
native=json.loads(record.read_text())
assert native=={'args':['--cd',str(repo)],'cwd':str(repo),'idle':None,'thread':None},native
assert 'exit status does not confirm a saved trust decision' in output,output
assert config.read_bytes()==original
code,output=run(args,MX_CONSENT_EXIT='23')
assert code==23,(code,output)
assert 'Canceling grants no consent' in output
for selector in [str(base/'foreign checkout'),'missing-project']:
 code,output=run(['consent',selector,'--harness','codex'])
 assert code!=0 and not record.exists(),(code,output)
for invalid in [['consent','consent-app'],['consent','consent-app','--harness','claude'],args+['--yes']]:
 code,output=run(invalid)
 assert code!=0 and not record.exists() and 'usage:' in output,(code,output)
code,output=run(args,MX_REAL_CODEX='relative-codex')
assert code!=0 and not record.exists() and 'absolute executable path' in output,(code,output)
code,output=run(args,MX_REAL_CODEX=str(base/'absent-codex'))
assert code!=0 and not record.exists() and 'cannot resolve Codex executable' in output,(code,output)
code,output=run(args,MX_REAL_CODEX=os.environ['MX_CONSENT_BINARY'])
assert code!=0 and not record.exists() and 'refuses a Multplx shim' in output,(code,output)
fake.chmod(0o600)
code,output=run(args)
assert code!=0 and not record.exists() and 'could not open native Codex folder review' in output,(code,output)
fake.chmod(0o700)
# A replaced registered location must fail before any native UI is opened.
repo.rename(repo.with_name('retained original'))
repo.mkdir()
code,output=run(args)
assert code!=0 and not record.exists(),(code,output)
assert config.read_bytes()==original
PY
pass 'explicit native consent retains exact paths, cancellation, provider config and registration identity'
