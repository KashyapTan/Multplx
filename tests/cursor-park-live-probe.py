#!/usr/bin/env python3
"""Isolated real Cursor stop-hook acceptance probe; no global hook changes."""
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[1]
agent = shutil.which("agent") or shutil.which("cursor-agent")
if os.environ.get("MX_CURSOR_PARK_LIVE") != "1":
    print("skip: set MX_CURSOR_PARK_LIVE=1 for the authenticated interactive park probe")
    raise SystemExit(0)
assert agent and shutil.which("tmux"), "Cursor and tmux are required"
version = subprocess.check_output([agent, "--version"], text=True).strip()
subprocess.run([agent, "status"], check=True, stdout=subprocess.DEVNULL)
lab = Path(tempfile.mkdtemp(prefix="mx-cursor-park-live-"))
state = lab / "state"
state.mkdir()
shutil.copytree(root / "bin", lab / "bin")
(lab / ".cursor").mkdir()
(lab / "AGENTS.md").write_text("# Isolated transport probe\n")
(lab / ".mx-daemon-home").write_text("cursor-live-probe\n")
(state / "probe.meta").write_text("id=probe\n")
start_hook = lab / "session-start.py"
start_hook.write_text('''import json, os, pathlib, subprocess, sys
payload = json.load(sys.stdin)
state = pathlib.Path(os.environ["MX_STATE_OVERRIDE"])
pid = os.getppid()
for _ in range(12):
    row = subprocess.check_output(["ps", "-p", str(pid), "-o", "ppid=", "-o", "comm=", "-o", "args="], text=True).strip()
    parent, command, *arguments = row.split()
    if "cursor-agent" in command or "cursor-agent" in " ".join(arguments):
        (state / ".lock").write_text(str(pid) + "\\n")
        break
    pid = int(parent)
else: raise RuntimeError("cannot locate actual Cursor owner")
print(json.dumps({"additional_context":"Transport probe: never use tools. On an operational watcher followup reply exactly CURSOR_WAKE_HANDLED. Follow exact human reply requests otherwise."}))
''')
arm = lab / "bin" / "mx-watch-arm.sh"
arm.write_text('''#!/usr/bin/env python3
import os, pathlib, time
state = pathlib.Path(os.environ["MX_STATE_OVERRIDE"])
marker = state / ("live-arm." + str(os.getpid()))
marker.touch()
while not (state / "release-event").exists(): time.sleep(.05)
(state / "release-event").unlink()
print("signal: live-cursor-event", flush=True)
marker.unlink()
''')
arm.chmod(0o700)
hooks = json.loads((root / ".cursor/hooks.json").read_text())
hooks["hooks"] = {
    "sessionStart": [{"command": f"python3 {shlex.quote(str(start_hook))}"}],
    "stop": hooks["hooks"]["stop"],
}
(lab / ".cursor" / "hooks.json").write_text(json.dumps(hooks))
socket = f"mx-cursor-park-{os.getpid()}"
def tmux(*args):
    return subprocess.check_output(["tmux", "-L", socket, *args], text=True)
def pane():
    return tmux("capture-pane", "-p", "-S", "-1000", "-t", "primary")
def wait(predicate, what, timeout=90):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate(): return
        time.sleep(.3)
    raise RuntimeError(f"{what} timed out; fixture retained at {lab}\n{pane()}")
def submit(message):
    tmux("send-keys", "-t", "primary", "-l", message)
    tmux("send-keys", "-t", "primary", "Enter")
environment = {
    "MX_ROOT_OVERRIDE":str(lab), "MX_HOME":str(lab), "MX_STATE_OVERRIDE":str(state),
    "MX_RUST_BIN":str(root / "target/release/mx"),
}
launch = "env " + " ".join(f"{name}={shlex.quote(value)}" for name,value in environment.items())
launch += " " + shlex.join([agent, "--sandbox", "enabled", "--trust", "--workspace", str(lab), "--model", os.environ.get("MX_CURSOR_LIVE_MODEL", "auto")])
try:
    tmux("new-session", "-d", "-s", "primary", "-x", "180", "-y", "70", "-c", str(lab), launch)
    wait(lambda: (state / ".lock").exists(), "sessionStart owner")
    submit("Reply exactly CURSOR_IDLE_READY. Never use tools during this transport probe. On a watcher follow-up reply exactly CURSOR_WAKE_HANDLED.")
    wait(lambda: bool(list(state.glob(".cursor-park-output.*"))), "first owned park")
    first_baton = (state / ".cursor-park-owner").read_text()
    submit("Reply exactly CURSOR_HUMAN_READY. Never use tools.")
    wait(lambda: (state / ".cursor-park-owner").read_text() != first_baton, "human turn and next stop claim")
    wait(lambda: len(list(state.glob(".cursor-park-output.*"))) == 1, "older park retirement")
    assert "CURSOR_HUMAN_READY" in pane(), "human response absent"
    for event in (1, 2):
        replies_before = pane().count("CURSOR_WAKE_HANDLED")
        prior_baton = (state / ".cursor-park-owner").read_text()
        (state / "release-event").touch()
        wait(lambda: pane().count("CURSOR_WAKE_HANDLED") > replies_before, f"watcher follow-up {event}")
        wait(lambda: (state / ".cursor-park-owner").read_text() != prior_baton, f"successor park {event}")
        wait(lambda: len(list(state.glob(".cursor-park-output.*"))) == 1, f"single successor {event}")
    (state / ".afk").touch()
    wait(lambda: not list(state.glob(".cursor-park-output.*")), "away-mode child cleanup")
    (lab / "transcript.txt").write_text(pane())
    (lab / "result.json").write_text(json.dumps({"version":version,"model":os.environ.get("MX_CURSOR_LIVE_MODEL","auto"),"result":"passed","human_input_while_parked":True,"event_followups":2,"away_cleanup":True},indent=2))
    print(f"ok - Cursor {version}: real stop-hook park, human input, supersession, two event follow-ups and AFK cleanup; evidence {lab}")
finally:
    subprocess.run(["tmux", "-L", socket, "kill-server"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
