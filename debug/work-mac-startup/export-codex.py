#!/usr/bin/env python3
"""Back up one operator-identified Codex session; never choose newest/by cwd."""
import argparse
import datetime
import hashlib
import json
import os
import sys
from pathlib import Path
import uuid
from importlib.util import module_from_spec, spec_from_file_location

sys.dont_write_bytecode = True
spec = spec_from_file_location('startup_observer', Path(__file__).with_name('observe.py'))
observer = module_from_spec(spec)
spec.loader.exec_module(observer)
regular = observer.regular


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--session', required=True, help='exact native UUID of this conversation')
    parser.add_argument('--cwd', type=Path, required=True, help='session metadata cwd (not current shell cwd)')
    parser.add_argument('--store', type=Path, required=True, help='exact selected CODEX_HOME/sessions or archived_sessions')
    parser.add_argument('--output', type=Path, required=True, help='new private directory')
    args = parser.parse_args()
    try:
        canonical = str(uuid.UUID(args.session))
    except ValueError:
        parser.error('use the exact canonical native session UUID')
    if canonical != args.session:
        parser.error('use the exact canonical native session UUID')
    if not all(p.is_absolute() for p in (args.cwd, args.store, args.output)):
        parser.error('paths must be absolute')
    matches = []
    for path in args.store.rglob(f'*{args.session}*.jsonl'):
        try:
            value = regular(path, 256*1024*1024)
            first = json.loads(value.splitlines()[0])
            if (first.get('type') == 'session_meta' and first['payload']['id'] == args.session
                    and Path(first['payload']['cwd']).resolve() == args.cwd.resolve()):
                matches.append((path, value))
        except (OSError, ValueError, KeyError, IndexError):
            continue
    if len(matches) != 1:
        parser.error(f'expected exactly one header-verified session, found {len(matches)}; do not guess')
    os.umask(0o077)
    args.output.mkdir(parents=False, exist_ok=False, mode=0o700)
    path, value = matches[0]
    (args.output / 'transcript.jsonl').write_bytes(value)
    (args.output / 'manifest.json').write_text(json.dumps(dict(
        at=datetime.datetime.now(datetime.timezone.utc).isoformat(), source=str(path),
        session=args.session, cwd=str(args.cwd), bytes=len(value),
        sha256=hashlib.sha256(value).hexdigest(),
        limitation='Point-in-time copy of current file; repeat after final provider flush. '
                   'Includes any earlier conversation content; private until reviewed.'), indent=2)+'\n')
    print(args.output / 'manifest.json')


if __name__ == '__main__':
    main()
