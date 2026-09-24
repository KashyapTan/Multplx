#!/usr/bin/env bash
# Real tmux/PTY routing with a synthetic harness; no model provider is started.
set -eu
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

command -v tmux >/dev/null 2>&1 || { printf 'skip: tmux unavailable\n'; exit 0; }
command -v python3 >/dev/null 2>&1 || { printf 'skip: python3 unavailable\n'; exit 0; }
PYTHON=${MX_TEST_PYTHON:-$(command -v python3)}
if [ -z "${MX_TEST_PYTHON:-}" ] && [ -x /usr/bin/python3 ]; then PYTHON=/usr/bin/python3; fi
export MX_TEST_ROOT=$ROOT

mx_test_tmproot_into TMP_ROOT mx-launcher-connection
RUNTIME=$TMP_ROOT/runtime
HOME_DIR=$TMP_ROOT/home
FAKE=$TMP_ROOT/codex
BINARY=${MX_TEST_BINARY:-$ROOT/target/release/mx}
mkdir -p "$RUNTIME/bin" "$RUNTIME/.agents/skills" "$RUNTIME/share/shell/shims" \
  "$HOME_DIR/config" "$HOME_DIR/data" "$HOME_DIR/projects" "$HOME_DIR/state"
printf '# fixture\n' >"$RUNTIME/AGENTS.md"
cp "$ROOT/bin/mx-launcher.sh" "$RUNTIME/bin/mx-launcher.sh"
cp "$ROOT/bin/mx-lock.sh" "$RUNTIME/bin/mx-lock.sh"
chmod +x "$RUNTIME/bin/mx-launcher.sh" "$RUNTIME/bin/mx-lock.sh"
git -C "$RUNTIME" init -q
git -C "$RUNTIME" add -A
git -C "$RUNTIME" -c user.name=Fixture -c user.email=fixture@example.test commit -qm fixture
cat >"$FAKE" <<'SH'
#!/usr/bin/env bash
set -eu
printf '%s\n' "$$" >"$MX_FAKE_RECORD.pid"
printf 'start\n' >>"$MX_FAKE_RECORD.count"
sleep "${MX_FAKE_LOCK_DELAY:-0}"
printf '%s\n' "$$" >"$MX_HOME/state/.lock"
while [ ! -e "$MX_FAKE_RECORD.stop" ]; do sleep .05; done
SH
chmod +x "$FAKE"

run_env=(env MX_ROOT_OVERRIDE="$RUNTIME" MX_HOME="$HOME_DIR" MX_REAL_CODEX="$FAKE" MX_FAKE_RECORD="$TMP_ROOT/harness")
"${run_env[@]}" "$BINARY" project register "$RUNTIME" >/dev/null
socket=mx-connection-$$
cleanup() {
  tmux -L "$socket" kill-server 2>/dev/null || true
  if [ -s "$TMP_ROOT/harness.pid" ]; then kill "$(cat "$TMP_ROOT/harness.pid")" 2>/dev/null || true; fi
  [ -z "${launcher:-}" ] || kill "$launcher" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
printf -v launch_command '%q ' "${run_env[@]}" "$BINARY" launch-harness codex
tmux -L "$socket" new-session -d -s primary "$launch_command"
for _ in $(seq 1 100); do [ -s "$HOME_DIR/state/.lock" ] && break; sleep .05; done
[ -s "$HOME_DIR/state/.lock" ] || fail 'synthetic tmux harness did not publish its lock'
for _ in $(seq 1 100); do [ -s "$HOME_DIR/state/workspace-connection.json" ] && break; sleep .02; done
[ -s "$HOME_DIR/state/workspace-connection.json" ] || fail 'launcher did not publish tmux connection record'

"$PYTHON" - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import fcntl, os, pty, select, struct, sys, termios, time
sys.path.insert(0,os.path.join(os.environ["MX_TEST_ROOT"],"tests"))
from helpers.pty_screen import screen_text
binary, root, home, fake, record = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 12, 40, 0, 0))
captured=b""
def read_until_quiet(timeout=10, quiet=.25):
    global captured
    deadline=time.time()+timeout; last_output=None
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.05)
        if ready:
            try: data=os.read(fd,8192)
            except OSError: break
            if not data: break
            captured += data; last_output=time.time()
        elif last_output is not None and time.time()-last_output>=quiet:
            return
    if last_output is None: raise SystemExit("workspace TUI produced no terminal output")
read_until_quiet()
before_resize=len(captured)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
read_until_quiet(timeout=5)
resized=captured[before_resize:]
if b"Domains" not in resized or b"\x1b[" not in resized:
    raise SystemExit("workspace TUI did not repaint after resize without input: " + resized.decode(errors="replace")[-2000:])
deadline=time.time()+5
while time.time()<deadline and "s ago" not in screen_text(captured):
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: data=os.read(fd,8192)
        except OSError: break
        if not data: break
        captured += data
if "s ago" not in screen_text(captured):
    raise SystemExit("workspace snapshot freshness did not become available")
last_output=time.time(); deadline=time.time()+5
while time.time()<deadline and time.time()-last_output<.25:
    ready,_,_=select.select([fd],[],[],.05)
    if ready:
        try: data=os.read(fd,8192)
        except OSError: break
        if not data: break
        captured += data; last_output=time.time()
idle_start=len(captured)
idle_deadline=time.time()+.4
while time.time()<idle_deadline:
    ready,_,_=select.select([fd],[],[],.05)
    if ready:
        try: data=os.read(fd,8192)
        except OSError: break
        if not data: break
        captured += data
if len(captured)-idle_start > 1000:
    raise SystemExit("workspace TUI emitted full-frame output while idle (" + str(len(captured)-idle_start) + " bytes): " + captured[idle_start:][-2000:].decode(errors="replace"))
def active_heading():
    for line in screen_text(captured).splitlines():
        for heading in ("╭ Projects ", "╭ Tasks ", "╭ Pending decisions ", "╭ Active domains "):
            if heading in line: return heading
    return None
for _ in range(4):
    prior=active_heading(); os.write(fd,b"\t"); deadline=time.time()+3
    while time.time()<deadline and active_heading()==prior:
        ready,_,_=select.select([fd],[],[],.1)
        if ready:
            try: data=os.read(fd,8192)
            except OSError: break
            if not data: break
            captured += data
    if active_heading()=="╭ Active domains ": break
if active_heading()!="╭ Active domains ":
    raise SystemExit("workspace tab navigation did not reach Domains: "+screen_text(captured))
deadline=time.time()+5
while time.time()<deadline:
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: data=os.read(fd,8192)
        except OSError: break
        if not data: break
        captured += data
    visible=screen_text(captured)
    if "No active domains." in visible and "q quit" in visible: break
else: os.kill(pid,9); raise SystemExit("workspace TUI did not render views: " + screen_text(captured))
visible=screen_text(captured)
if "No active domains." not in visible or "q quit" not in visible:
    raise SystemExit("workspace TUI did not render navigable detail views/footer: "+visible)
os.write(fd, b"q")
deadline=time.time()+5
while time.time()<deadline:
    ready,_,_=select.select([fd],[],[],.05)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("workspace TUI did not close cleanly")
        break
    time.sleep(.05)
else:
    os.kill(pid,9)
    raise SystemExit("workspace TUI ignored clean close; screen="+screen_text(captured)+"; tail="+captured[-500:].decode(errors="replace"))
while True:
    ready,_,_=select.select([fd],[],[],0)
    if not ready: break
    try: data=os.read(fd,8192)
    except OSError: break
    if not data: break
    captured += data
if b"\x1b[?1049h" not in captured or b"\x1b[?1049l" not in captured:
    raise SystemExit("workspace TUI did not enter and leave the alternate screen")
if b"\r\n" in captured:
    raise SystemExit("workspace TUI emitted newline-based screen redraws")
PY
kill -0 "$(cat "$TMP_ROOT/harness.pid")" || fail 'closing workspace TUI ended independent harness work'

"$PYTHON" - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import fcntl, os, pty, select, signal, struct, subprocess, sys, termios, time
sys.path.insert(0,os.path.join(os.environ["MX_TEST_ROOT"],"tests"))
from helpers.pty_screen import screen_text
binary, root, home, fake, record = sys.argv[1:]
def launch():
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir("/")
        os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
            MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    return pid, fd
def wait_for(pid, fd):
    captured=b""; deadline=time.time()+15
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.1)
        if ready:
            try: data=os.read(fd,8192)
            except OSError: break
            if not data: break
            captured += data
        if "Tab " in screen_text(captured): return captured
        done,status=os.waitpid(pid,os.WNOHANG)
        if done: raise SystemExit("TUI exited before signal test: "+captured[-2000:].decode(errors="replace"))
    raise SystemExit("TUI did not render before signal test")
def finish(pid, fd, captured, expected):
    deadline=time.time()+5
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.05)
        if ready:
            try: data=os.read(fd,8192)
            except OSError: data=b""
            if not data: pass
            else: captured += data
        done,status=os.waitpid(pid,os.WNOHANG)
        if done:
            while True:
                ready,_,_=select.select([fd],[],[],0)
                if not ready: break
                try: data=os.read(fd,8192)
                except OSError: break
                if not data: break
                captured += data
            code=os.waitstatus_to_exitcode(status)
            if code != expected: raise SystemExit("unexpected TUI signal exit status: "+str(code))
            if b"\x1b[?1049l" not in captured: raise SystemExit("TUI did not leave alternate screen after signal")
            try:
                flags=termios.tcgetattr(fd)[3]
                if flags & (termios.ICANON | termios.ECHO) != (termios.ICANON | termios.ECHO):
                    raise SystemExit("TUI did not restore canonical echo mode")
            except OSError: pass
            return
        time.sleep(.05)
    try:
        ready,_,_=select.select([fd],[],[],0)
        if ready: captured += os.read(fd,8192)
    except OSError: pass
    process=subprocess.run(["ps","-o","pid,ppid,stat,etime,command","-p",str(pid)],capture_output=True,text=True).stdout
    os.kill(pid,9); raise SystemExit("TUI did not exit after signal "+str(expected)+"; ps="+process+"; "+captured[-1000:].decode(errors="replace"))
pid,fd=launch(); captured=wait_for(pid,fd)
os.kill(pid,signal.SIGINT); finish(pid,fd,captured,130)
pid,fd=launch(); captured=wait_for(pid,fd)
os.kill(pid,signal.SIGTERM); finish(pid,fd,captured,143)
PY

"$PYTHON" - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import fcntl, os, pty, select, struct, sys, termios, time
sys.path.insert(0,os.path.join(os.environ["MX_TEST_ROOT"],"tests"))
from helpers.pty_screen import screen_text
binary, root, home, fake, record = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
captured=b""
def wait_for(marker, timeout=15):
    global captured
    deadline=time.time()+timeout
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.1)
        if ready:
            try: data=os.read(fd,8192)
            except OSError: break
            if not data: break
            captured += data
        if marker.decode(errors="replace") in screen_text(captured): return
        done,status=os.waitpid(pid,os.WNOHANG)
        if done: raise SystemExit("TUI exited before "+repr(marker)+": "+captured[-4000:].decode(errors="replace"))
    raise SystemExit("TUI did not render "+repr(marker)+": "+captured[-4000:].decode(errors="replace"))
wait_for(b"Tab ")
os.write(fd,b"/runtime"); wait_for(b"Filter: runtime")
os.write(fd,b"\r")
deadline=time.time()+5
while time.time()<deadline and "Filter:" in screen_text(captured):
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: break
if "Filter:" in screen_text(captured): raise SystemExit("TUI did not leave filter mode after Enter")
os.write(fd,b"\x1b[B"); wait_for(b"Project  runtime")
os.write(fd,b"t"); wait_for(b"New task:")
os.write(fd,"PTY café 界 task".encode()); wait_for("New task: PTY café 界 task".encode())
os.write(fd,b"\r")
deadline=time.time()+30
while time.time()<deadline:
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("TUI task intake failed: "+captured.decode(errors="replace"))
        break
else: os.kill(pid,9); raise SystemExit("TUI task intake did not return")
if b"mx-operational-input" not in captured and b"request_id" not in captured:
    raise SystemExit("TUI task intake returned no durable receipt")
PY
grep -R -F 'PTY café 界 task' "$HOME_DIR/state" >/dev/null \
  || fail 'TUI task intake corrupted non-ASCII request text'

"$PYTHON" - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import fcntl, os, pty, select, struct, sys, termios, time
sys.path.insert(0,os.path.join(os.environ["MX_TEST_ROOT"],"tests"))
from helpers.pty_screen import screen_text
binary, root, home, fake, record = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
captured=b""
def wait_for(marker, timeout=15):
    global captured
    start=len(captured); deadline=time.time()+timeout
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.1)
        if ready:
            try: data=os.read(fd,8192)
            except OSError: break
            if not data: break
            captured += data
        if marker.decode(errors="replace") in screen_text(captured): return
    raise SystemExit("navigation TUI missed "+repr(marker)+": "+captured[-3000:].decode(errors="replace"))
wait_for(b"Tab ")
os.write(fd,b"/runtimx"); wait_for(b"Filter: runtimx")
os.write(fd,b"\x7f"); time.sleep(.2)
os.write(fd,b"\x1b"); time.sleep(.2)
os.write(fd,b"\x1b[B\x1b[A"); time.sleep(.2)
os.write(fd,b"\t"); wait_for("PTY café 界 task".encode())
os.write(fd,b"r"); time.sleep(.4)
for key in (b"\t", b"\t", b"\t"):
    os.write(fd,key); time.sleep(.2)
os.write(fd,b"t"); wait_for(b"New task:")
os.write(fd,"draft café".encode()); wait_for("New task: draft café".encode())
os.write(fd,b"\x7f"); wait_for(b"New task: draft caf")
os.write(fd,b"\x1b"); wait_for(b"Tab ")
os.write(fd,b"q")
deadline=time.time()+5
while time.time()<deadline:
    ready,_,_=select.select([fd],[],[],.05)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("navigation TUI did not close cleanly")
        break
    time.sleep(.05)
else: os.kill(pid,9); raise SystemExit("navigation TUI ignored clean close")
PY

"$PYTHON" - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" "$socket" <<'PY'
import fcntl, os, pty, select, struct, subprocess, sys, termios, time
sys.path.insert(0,os.path.join(os.environ["MX_TEST_ROOT"],"tests"))
from helpers.pty_screen import screen_text
binary, root, home, fake, record, socket = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
captured=b""; deadline=time.time()+15
while time.time()<deadline and "Tab " not in screen_text(captured):
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: data=os.read(fd,8192)
        except OSError: break
        if not data: break
        captured += data
if "Tab " not in screen_text(captured): raise SystemExit("context chat TUI did not render")
os.write(fd,b"jc")
deadline=time.time()+8
while time.time()<deadline:
    clients=subprocess.run(["tmux","-L",socket,"list-clients"],capture_output=True)
    if clients.returncode == 0 and clients.stdout.strip(): break
    ready,_,_=select.select([fd],[],[],.05)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
    done,status=os.waitpid(pid,os.WNOHANG)
    if done: raise SystemExit("selected TUI chat exited before attach: "+captured[-3000:].decode(errors="replace"))
    time.sleep(.05)
else: os.kill(pid,9); raise SystemExit("selected TUI chat did not attach: "+captured[-3000:].decode(errors="replace"))
if b"\x1b[?1049l" not in captured:
    raise SystemExit("workspace TUI did not restore terminal before chat handoff")
subprocess.run(["tmux","-L",socket,"detach-client","-s","primary"],check=True)
deadline=time.time()+5
while time.time()<deadline:
    ready,_,_=select.select([fd],[],[],.05)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("selected TUI chat failed")
        break
    time.sleep(.05)
else: os.kill(pid,9); raise SystemExit("selected TUI chat did not return")
PY
find "$HOME_DIR/state/workspace-contexts" -type f -name '*.json' -print -quit | grep -q . \
  || fail 'selected TUI chat did not publish immutable next-request context'

"$PYTHON" - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" "$socket" <<'PY'
import os, pty, select, subprocess, sys, time
binary, root, home, fake, record, socket = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.execve(binary, [binary, "launch-harness", "codex"], dict(os.environ,
        TERM="xterm-256color", MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
deadline = time.time() + 5
captured = b""
while time.time() < deadline:
    clients = subprocess.run(["tmux", "-L", socket, "list-clients"], capture_output=True)
    if clients.returncode == 0 and clients.stdout.strip(): break
    ready, _, _ = select.select([fd], [], [], .05)
    if ready:
        try: captured += os.read(fd, 4096)
        except OSError: pass
    done, status = os.waitpid(pid, os.WNOHANG)
    if done: raise SystemExit("attach launcher exited before attachment: " + captured.decode(errors="replace"))
else: raise SystemExit("attach launcher never created a tmux client: " + captured.decode(errors="replace"))
subprocess.run(["tmux", "-L", socket, "detach-client", "-s", "primary"], check=True)
deadline = time.time() + 5
while time.time() < deadline:
    done, status = os.waitpid(pid, os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status) != 0: raise SystemExit("attach launcher failed")
        break
    time.sleep(.05)
else:
    os.kill(pid, 9); raise SystemExit("attach launcher did not return after detach")
PY
[ "$(wc -l <"$TMP_ROOT/harness.count" | tr -d ' ')" = 1 ] || fail 'attach launched a second harness'
touch "$TMP_ROOT/harness.stop"
tmux -L "$socket" kill-server 2>/dev/null || true
rm -f "$HOME_DIR/state/.lock" "$HOME_DIR/state/workspace-launch.json" "$TMP_ROOT/harness.stop"
rm -f "$TMP_ROOT/harness.pid"

MX_FAKE_LOCK_DELAY=2 "${run_env[@]}" "$BINARY" launch-harness codex & launcher=$!
for _ in $(seq 1 100); do [ -s "$HOME_DIR/state/workspace-launch.json" ] && [ -s "$TMP_ROOT/harness.pid" ] && break; sleep .02; done
kill -9 "$launcher" 2>/dev/null || true
wait "$launcher" 2>/dev/null || true
if "${run_env[@]}" "$BINARY" launch-harness codex >/dev/null 2>&1; then
  fail 'second launch succeeded while crash-surviving child held reservation'
fi
[ "$(wc -l <"$TMP_ROOT/harness.count" | tr -d ' ')" = 2 ] || fail 'crash reservation allowed a second child'
kill "$(cat "$TMP_ROOT/harness.pid")" 2>/dev/null || true
trap - EXIT HUP INT TERM
cleanup
pass 'custom-socket tmux attach and crash-safe launch reservation keep one synthetic harness'
