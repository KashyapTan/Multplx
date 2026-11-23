#!/usr/bin/env python3
"""Bounded, read-only launch observation. Writes only a new private output directory."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import time


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def regular(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        meta = os.fstat(stream.fileno())
        if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.getuid():
            raise ValueError('not an owned regular file')
        value = stream.read(limit + 1)
        if len(value) > limit:
            raise ValueError(f'over {limit} byte bound; preserve separately')
        return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--task', required=True)
    parser.add_argument('--request-id', required=True, help='the exact chosen stable spawn request ID')
    parser.add_argument('--mx', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=30)
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,100}', args.task):
        parser.error('use a simple run-scoped task ID')
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,100}', args.request_id):
        parser.error('use a simple run-scoped request ID')
    if not 0 < args.seconds <= 60:
        parser.error('--seconds must be >0 and <=60')
    if not all(p.is_absolute() for p in (args.state, args.mx, args.output)):
        parser.error('paths must be absolute')
    os.umask(0o077)
    args.output.mkdir(parents=False, exist_ok=False, mode=0o700)
    env = dict(os.environ, MX_STATE_OVERRIDE=str(args.state))
    seen = {}
    started = time.monotonic()
    events = (args.output / 'events.jsonl').open('x')

    def event(kind, **values):
        events.write(json.dumps(dict(at=utc(), elapsed=round(time.monotonic()-started, 3),
                                     kind=kind, **values)) + '\n')
        events.flush()

    def save(label, path, limit=8*1024*1024):
        try:
            value = regular(path, limit)
            digest = hashlib.sha256(value).hexdigest()
            if seen.get(label) != digest:
                name = f'{len(list(args.output.iterdir())):05d}-{label}'
                (args.output / name).write_bytes(value)
                seen[label] = digest
                event('copy', source=str(path), saved=name, sha256=digest, bytes=len(value))
            return value
        except (OSError, ValueError) as error:
            # Deduplicate absent/racy files without hiding a later successful read.
            error_key = str(error)
            if seen.get('error:'+label) != error_key:
                event('unavailable', source=str(path), reason=error_key)
                seen['error:'+label] = error_key
            return None

    def command(label, argv, timeout=1):
        remaining = args.seconds - (time.monotonic() - started)
        if remaining <= 0:
            event('not-run', label=label, reason='observation deadline reached')
            return None
        timeout = min(timeout, remaining)
        try:
            result = subprocess.run(argv, env=env, capture_output=True, timeout=timeout)
            event('command', label=label, argv=[str(x) for x in argv], rc=result.returncode,
                  stdout=result.stdout.decode(errors='replace'), stderr=result.stderr.decode(errors='replace'))
            return result.stdout if result.returncode == 0 else None
        except (OSError, subprocess.TimeoutExpired) as error:
            event('command-unavailable', label=label, reason=str(error))
            return None

    event('start', task=args.task, request_id=args.request_id, owner_state=str(args.state), runtime=str(args.mx),
          seconds=args.seconds, pid=os.getpid())
    next_slow = 0.0
    sampled = set()
    while time.monotonic()-started < args.seconds:
        save('spawn-action.json', args.state / '.spawn-actions' / f'{args.request_id}.json')
        meta = save('task.meta', args.state / f'{args.task}.meta')
        fields = {}
        if meta:
            fields = dict(line.split('=', 1) for line in meta.decode(errors='replace').splitlines() if '=' in line)
        # Exact per-task paths only; never scan provider stores or other tasks.
        for suffix in ('status',):
            save('task.'+suffix, args.state / f'{args.task}.{suffix}')
        for suffix in ('intent', 'operation'):
            save('spawn.'+suffix, args.state / f'.spawn-{args.task}.{suffix}')
        tmp = fields.get('tasktmp')
        pid = None
        if tmp and Path(tmp).is_absolute():
            receipt = save('launch-started', Path(tmp) / 'launch-started', 16384)
            save('launch.sh', Path(tmp) / 'launch.sh')
            if receipt:
                lines = receipt.decode(errors='replace').splitlines()
                if len(lines) == 2 and lines[1].isdigit() and int(lines[1]) > 1:
                    pid = int(lines[1])
        elapsed = time.monotonic()-started
        if elapsed >= next_slow:
            next_slow = elapsed + 1
            if pid:
                # No argv/env listing; this PID is launcher evidence, not provider identity.
                command('launcher-process', ['ps', '-p', str(pid), '-o', 'pid=,ppid=,lstart=,etime=,comm='])
                if os.uname().sysname == 'Darwin' and pid not in sampled:
                    sampled.add(pid)
                    command('launcher-sample', ['/usr/bin/sample', str(pid), '1', '10',
                            '-file', str(args.output / f'sample-{pid}.txt')], timeout=2)
            command('task-model', [str(args.mx), 'task-model', 'inspect', args.task])
            session = command('task-session', [str(args.mx), 'task-session', 'inspect', args.task])
            if session:
                try:
                    value = json.loads(session)
                    provider = value.get('provider_session') or {}
                    path = provider.get('transcript_path')
                    if value.get('transcript_available') is True and path:
                        save('provider-transcript.jsonl', Path(path), 64*1024*1024)
                except (ValueError, TypeError) as error:
                    event('session-parse-error', reason=str(error))
            if meta:
                # peek resolves this exact task's recorded backend/endpoint.
                command('endpoint-tail', [str(args.mx), 'peek', args.task, '400'])
        time.sleep(min(.2, max(0, args.seconds-(time.monotonic()-started))))
    event('end', reason='bounded observation completed; no process stopped')
    events.close()


if __name__ == '__main__':
    main()
