//! Instrumented runtime proof with isolated owner and queue/watch fixtures.
use std::fs;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// Linux exec refuses an inode still open for writing in another process.
// Keep executable-fixture writes separate from the other fixture's fork/exec:
// a child can inherit the writable descriptor before close-on-exec runs.
static EXECUTABLE_FIXTURE: Mutex<()> = Mutex::new(());

#[test]
fn installed_idle_wrapper_uses_canonical_binary_without_release_tree() {
    let _fixture = EXECUTABLE_FIXTURE.lock().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let layout = tempfile::tempdir().unwrap();
    let wrapper = layout.path().join("mx-codex-idle.sh");
    fs::write(&wrapper, include_str!("../../../bin/mx-codex-idle.sh")).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(wrapper)
        .arg("--help")
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_RUST_SOURCE_ROOT", layout.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("Usage:")
    );
    assert!(!layout.path().join("target").exists());
}

#[test]
fn codex_idle_runtime_two_cycles_identity_retry_and_cleanup() {
    let _fixture = EXECUTABLE_FIXTURE.lock().unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let script = fixture.path().join("codex_fixture.py");
    fs::write(&script, r#"
import atexit, json, os, pathlib, subprocess, sys, time
root = pathlib.Path(sys.argv[2]); mx = sys.argv[1]
for name in ['state', 'bin', 'home']: (root/name).mkdir()
(root/'AGENTS.md').write_text('# isolated fixture\n')
(root/'.mx-daemon-home').write_text('codex-idle-fixture\n')
state = root/'state'; (state/'.lock').write_text(str(os.getpid()))
watch = root/'bin/mx-watch.sh'
watch.write_text('#!/bin/sh\nwhile [ ! -f "$MX_STATE_OVERRIDE/trigger" ]; do sleep .05; done\nrm -f "$MX_STATE_OVERRIDE/trigger"\n'); watch.chmod(0o755)
codex = root/'bin/codex'
def transport(mode='good'):
    text = '#!/bin/sh\n'
    if mode == 'unsupported': text += 'exit 1\n'
    else:
        text += 'if [ "$2" = --help ]; then printf "%s\\n" "--thread --message"; exit 0; fi\n'
        if mode == 'failed': text += 'echo rejected >&2; exit 1\n'
        else: text += 'printf "%s\\n" "$CODEX_HOME|$*" >> "$MX_STATE_OVERRIDE/sends"\n'
    codex.write_text(text); codex.chmod(0o755)
transport()
env = dict(os.environ, MX_ROOT_OVERRIDE=str(root), MX_HOME=str(root), MX_STATE_OVERRIDE=str(state), MX_RUST_SOURCE_ROOT=str(root), MX_CODEX_IDLE_CLI='1', CODEX_HOME=str(root/'home'), MX_REAL_CODEX=str(codex))
for name in ['MX_TASK_ID','MX_CURRENT_ADMISSION_ID','DEEP_REVIEW_GATE']: env.pop(name,None)
thread = '123e4567-e89b-12d3-a456-426614174000'
def call(args=[], payload=None, extra=None):
    config = dict(env); config.update(extra or {})
    result = subprocess.run([mx,'supervision','mx-codex-idle.sh']+args, input=json.dumps({'session_id':thread} if payload is None else payload) if not args or args == ['--register'] else '', text=True, capture_output=True, env=config, timeout=8)
    assert result.returncode == 0, result.stderr
    return result.stdout
def wait(test):
    until=time.monotonic()+6
    while time.monotonic()<until:
        if test(): return
        time.sleep(.03)
    raise AssertionError('condition timed out')
def sends():
    return (state/'sends').read_text().splitlines() if (state/'sends').exists() else []
def append(n):
    with (state/'.wake-queue').open('a') as f: f.write(f'1\t{n}\tsignal\tchild-{n}\tdone\n')
atexit.register(lambda: call(['--end'], extra={'CODEX_THREAD_ID':thread}))
append(1)
assert 'readiness is missing' in call()
assert len(sends())==0
(state/'.lock').unlink()
assert call(['--register']).strip()=='{}'
assert json.loads((state/'.codex-idle-hook-ready.json').read_text())['thread']==thread
assert json.loads((state/'.codex-idle-hook-ready.json').read_text())['owner']['pid']==os.getpid(), (state/'.codex-idle-hook-ready.json').read_text()
(state/'.lock').write_text('99999999')
assert call(['--register']).strip()=='{}'
assert (state/'.lock').read_text()=='99999999', 'registration mutated stale lock'
(state/'.lock').write_text('malformed')
assert call(['--register']).strip()==''
assert (state/'.lock').read_text()=='malformed'
(state/'.lock').write_text(str(os.getpid()))
# Registration restores the missing-readiness diagnostic, not queue failures.
call(['--retry'],extra={'CODEX_THREAD_ID':thread})
first_output = call(); assert first_output.strip() == '{}', first_output
wait(lambda:len(sends())==1 and not (state/'.codex-idle-failure').exists())
first=json.loads((state/'.codex-idle-process.json').read_text())
assert call().strip() == '{}'
assert json.loads((state/'.codex-idle-process.json').read_text())==first
assert len(sends())==1
assert str(root/'home') in sends()[0] and '--thread '+thread in sends()[0]
assert 'MULTPLX_OP:' in sends()[0] and (state/'.wake-queue').exists()
assert 'already bound' in call(payload={'session_id':'123e4567-e89b-12d3-a456-426614174001'})
assert not (state/'.codex-idle-failure').exists(), 'other thread poisoned owner'
# A new report wakes after watcher completion; hook owns the subsequent cycle.
append(2)
(state/'trigger').touch()
wait(lambda:len(sends())==2)
assert json.loads((state/'.codex-idle-process.json').read_text())==first
assert call().strip() == '{}'
time.sleep(.15); assert len(sends())==2
assert 'already bound' in call(['--end'],extra={'CODEX_THREAD_ID':'123e4567-e89b-12d3-a456-426614174001'})
assert call(['--end'],extra={'CODEX_THREAD_ID':thread}).strip() == '{}'
wait(lambda:not (state/'.codex-idle.lock').exists())
assert call(['--end'],extra={'CODEX_THREAD_ID':thread}).strip() == '{}'
transport('unsupported'); assert 'does not support' in call()
assert (state/'.codex-idle-failure').exists(); assert len(sends())==2
transport(); assert call(['--retry'],extra={'CODEX_THREAD_ID':thread}).strip() == '{}'
time.sleep(.15); assert len(sends())==2
call(['--end'],extra={'CODEX_THREAD_ID':thread})
transport('failed'); append(3); call()
wait(lambda:(state/'.codex-idle-failure').exists() and not (state/'.codex-idle.lock').exists())
assert 'uncertain' in call(); assert len(sends())==2
transport(); call(['--retry'],extra={'CODEX_THREAD_ID':thread}); wait(lambda:len(sends())==3)
call(['--end'],extra={'CODEX_THREAD_ID':thread})
# A crash after receipt publication but before failure publication is visible.
append(4)
receipts=json.loads((state/'.codex-idle-receipts.json').read_text())
receipts.append(thread+':4:0')
(state/'.codex-idle-receipts.json').write_text(json.dumps(receipts))
(state/'.codex-idle-uncertain.json').write_text(json.dumps(['4:0']))
(state/'.codex-idle-failure').unlink(missing_ok=True)
assert 'Codex idle supervision failed' in call()
assert len(sends())==3
call(['--retry'],extra={'CODEX_THREAD_ID':thread}); wait(lambda:len(sends())==4)
call(['--end'],extra={'CODEX_THREAD_ID':thread})
# Watcher failure wakes the ended turn through a retained canonical Check event.
watch.write_text('#!/bin/sh\nexit 7\n')
call(); wait(lambda:(state/'.codex-idle-failure').exists() and not (state/'.codex-idle.lock').exists())
assert len(sends())==5 and 'codex-idle-failure-' in (state/'.wake-queue').read_text()
watch.write_text('#!/bin/sh\nwhile [ ! -f "$MX_STATE_OVERRIDE/trigger" ]; do sleep .05; done\nrm -f "$MX_STATE_OVERRIDE/trigger"\n')
call(['--retry'],extra={'CODEX_THREAD_ID':thread})
call(['--end'],extra={'CODEX_THREAD_ID':thread})
# Missing thread identity cannot make a manual recovery retarget the bridge.
env.pop('CODEX_THREAD_ID', None)
assert 'CODEX_THREAD_ID' in call(['--retry'])
assert 'exact UUID' in call(payload={'session_id':'not-a-thread'})
assert 'session_id' in call(payload={})
assert call(['--help']).startswith('Usage:')
assert call(extra={'MX_CODEX_IDLE_CLI':'0'}).strip()==''
# A lost system-lock identity retires the detached worker and watcher group.
watch.write_text('#!/bin/sh\nsleep 60\n'); call()
wait(lambda:(state/'.codex-idle.lock').exists())
(state/'.lock').write_text('99999999')
wait(lambda:not (state/'.codex-idle.lock').exists())
assert call().strip()==''
print('two wake cycles, duplicate Stop, exact thread/home, unsupported/failure recovery, owner-loss cleanup passed')
"#).unwrap();
    let mut child = Command::new("python3")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_mx"))
        .arg(fixture.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("isolated Codex bridge fixture exceeded 45 seconds");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
