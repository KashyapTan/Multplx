#!/usr/bin/env python3
"""Run inert, isolated checks of capture, failures, bounds and exact-session export."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

KIT = Path(__file__).resolve().parent


def run(script, *args, ok=True):
    if script == 'observe.py':
        args = (*args, '--request-id', 'kit-request')
    result = subprocess.run([sys.executable, str(KIT/script), *map(str, args)], capture_output=True, text=True)
    assert (result.returncode == 0) == ok, (result.returncode, result.stdout, result.stderr)
    return result


with tempfile.TemporaryDirectory(prefix='mx-work-mac-kit-') as tmp:
    root = Path(tmp)
    state = root/'state'
    state.mkdir()
    (state/'.spawn-actions').mkdir()
    (state/'.spawn-actions/kit-request.json').write_text('{"stage":"submitted","endpoint":"@fake"}\n')
    launch = root/'launch'
    launch.mkdir()
    store = root/'sessions'
    store.mkdir()
    session = '12345678-1234-1234-1234-123456789abc'
    transcript = store/f'rollout-{session}.jsonl'
    transcript.write_text(json.dumps(dict(type='session_meta', payload=dict(id=session, cwd=str(root))))+'\n'
                          + json.dumps(dict(type='tool_result', payload='synthetic-only'))+'\n')
    (launch/'launch.sh').write_text('# synthetic launch, never execute\n')
    # No live PID: tests never sample a real process.
    (launch/'launch-started').write_text('synthetic-identity\n1\n')
    (state/'kit-worker.meta').write_text(f'tasktmp={launch}\nwindow=@fake\nharness=codex\n')
    (state/'kit-worker.status').write_text('working: synthetic\n')
    (state/'.spawn-kit-worker.intent').write_text('synthetic retained intent\n')
    fake = root/'mx'
    fake.write_text('#!'+sys.executable+'\n'+f'''
import json, sys
from pathlib import Path
assert __import__('os').environ['MX_STATE_OVERRIDE'] == {str(state)!r}
args = sys.argv[1:]
if args == ['task-session', 'inspect', 'kit-worker']:
    print(json.dumps(dict(transcript_available=True, provider_session=dict(transcript_path={str(transcript)!r}, session_id={session!r}))))
elif args == ['task-model', 'inspect', 'kit-worker']:
    print('{{"task_id":"kit-worker","attempt":{{"id":"synthetic-attempt"}}}}')
elif args == ['peek', 'kit-worker', '400']:
    print('synthetic endpoint text')
else:
    print('fixture command unavailable', file=sys.stderr)
    sys.exit(7)
''')
    fake.chmod(0o700)
    before = {p: p.read_bytes() for p in state.rglob('*') if p.is_file()}
    output = root/'observed'
    run('observe.py', '--state', state, '--task', 'kit-worker', '--mx', fake, '--output', output, '--seconds', '.5')
    events = [json.loads(line) for line in (output/'events.jsonl').read_text().splitlines()]
    assert events[0]['kind'] == 'start' and events[-1]['kind'] == 'end'
    copies = [e for e in events if e['kind'] == 'copy']
    assert any(e['source'] == str(launch/'launch-started') for e in copies)
    assert any(e['source'] == str(state/'.spawn-actions/kit-request.json') for e in copies)
    assert any(e['source'] == str(transcript) for e in copies)
    assert any(e.get('label') == 'endpoint-tail' and e['stdout'] == 'synthetic endpoint text\n' for e in events)
    for e in copies:
        value = (output/e['saved']).read_bytes()
        assert hashlib.sha256(value).hexdigest() == e['sha256']
        assert stat.S_IMODE((output/e['saved']).stat().st_mode) == 0o600
    assert stat.S_IMODE(output.stat().st_mode) == 0o700
    assert before == {p: p.read_bytes() for p in state.rglob('*') if p.is_file()}
    run('observe.py', '--state', state, '--task', 'kit-worker', '--mx', fake, '--output', output, '--seconds', '.1', ok=False)
    run('observe.py', '--state', state, '--task', '../escape', '--mx', fake, '--output', root/'bad-task', ok=False)
    run('observe.py', '--state', state, '--task', 'kit-worker', '--mx', fake, '--output', root/'bad-bound', '--seconds', '61', ok=False)
    missing = root/'missing'
    run('observe.py', '--state', state, '--task', 'absent', '--mx', fake, '--output', missing, '--seconds', '.5')
    absent_events = [json.loads(line) for line in (missing/'events.jsonl').read_text().splitlines()]
    assert any(e['kind'] == 'command' and e['rc'] == 7 for e in absent_events)
    assert any(e['kind'] == 'unavailable' for e in absent_events)
    backup = root/'root-backup'
    run('export-codex.py', '--session', session, '--cwd', root, '--store', store, '--output', backup)
    assert (backup/'transcript.jsonl').read_bytes() == transcript.read_bytes()
    assert json.loads((backup/'manifest.json').read_text())['session'] == session
    run('export-codex.py', '--session', session, '--cwd', state, '--store', store, '--output', root/'wrong-cwd', ok=False)
    second = store/f'duplicate-{session}.jsonl'
    second.write_bytes(transcript.read_bytes())
    run('export-codex.py', '--session', session, '--cwd', root, '--store', store, '--output', root/'ambiguous', ok=False)
    second.unlink()
    transcript.rename(root/'original')
    transcript.symlink_to(root/'original')
    run('export-codex.py', '--session', session, '--cwd', root, '--store', store, '--output', root/'linked', ok=False)
print('PASS: private capture/hashes, exact-task routing, unchanged fake state, missing evidence, bounds, exact-session export and refusal cases; no live models/backends sampled')
