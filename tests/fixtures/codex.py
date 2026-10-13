#!/usr/bin/env python3
"""Run one test command beneath a real, live Codex-named owner process."""

import os
import pathlib
import subprocess
import sys
import time


def main() -> int:
    if len(sys.argv) < 4 or sys.argv[2] not in {"handle", "retain"}:
        return 2
    state = pathlib.Path(sys.argv[1])
    mode = sys.argv[2]
    state.mkdir(parents=True, exist_ok=True)
    lock = state / ".lock"
    fixture_lock = state / ".test-wake-owner.lock"
    deadline = time.monotonic() + 10
    while True:
        try:
            fixture_lock.mkdir()
            break
        except FileExistsError:
            if time.monotonic() >= deadline:
                return 3
            time.sleep(0.01)
    owner = f"{os.getpid()}\n"
    try:
        with lock.open("x", encoding="utf-8") as stream:
            stream.write(owner)
    except FileExistsError:
        fixture_lock.rmdir()
        return 4
    environment = os.environ.copy()
    environment["MX_STATE_OVERRIDE"] = str(state)
    # Session-start fixtures may provide a fake `ps`; point that probe at this
    # same live owner so nested lock and wake commands share one identity.
    environment["MX_FAKE_HARNESS_PID"] = str(os.getpid())
    try:
        result = subprocess.run(sys.argv[3:], env=environment, check=False)
        if result.returncode == 0 and mode == "handle":
            configured = environment.get("MX_RUST_BIN")
            command_path = pathlib.Path(sys.argv[3])
            if configured:
                mx = pathlib.Path(configured)
            elif command_path.name == "mx":
                mx = command_path
            elif command_path.parent.name == "bin":
                mx = command_path.parent.parent / "target" / "release" / "mx"
            else:
                mx = command_path.parent / "mx"
            listing = subprocess.run(
                [str(mx), "wake", "list", "--unfinished"],
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )
            if listing.returncode != 0:
                return listing.returncode
            import json

            for line in listing.stdout.splitlines():
                if not line.startswith("{"):
                    continue
                item = json.loads(line)
                claim = item.get("claim")
                if not claim or claim.get("owner", {}).get("pid") != os.getpid():
                    continue
                event_id = item["event_id"]
                disposition = subprocess.run(
                    [
                        str(mx),
                        "wake",
                        "disposition",
                        event_id,
                        "handled",
                        "--detail",
                        "handled by isolated queue test",
                    ],
                    env=environment,
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                if disposition.returncode != 0:
                    return disposition.returncode
                acknowledgement = subprocess.run(
                    [str(mx), "wake", "ack", event_id],
                    env=environment,
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                if acknowledgement.returncode != 0:
                    return acknowledgement.returncode
        return result.returncode
    finally:
        try:
            if lock.read_text(encoding="utf-8") == owner:
                lock.unlink()
        except FileNotFoundError:
            pass
        try:
            fixture_lock.rmdir()
        except FileNotFoundError:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
