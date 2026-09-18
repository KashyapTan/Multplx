#!/usr/bin/env bash
# Real tmux/PTY routing with a synthetic harness; no model provider is started.
set -eu
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

command -v tmux >/dev/null 2>&1 || { printf 'skip: tmux unavailable\n'; exit 0; }
command -v python3 >/dev/null 2>&1 || { printf 'skip: python3 unavailable\n'; exit 0; }

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

python3 - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import fcntl, os, pty, select, struct, sys, termios, time
binary, root, home, fake, record = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 12, 40, 0, 0))
time.sleep(1.5)
captured=b""
while True:
    ready,_,_=select.select([fd],[],[],0)
    if not ready: break
    try: captured += os.read(fd,8192)
    except OSError: break
before_resize=len(captured)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
time.sleep(.4)
while True:
    ready,_,_=select.select([fd],[],[],0)
    if not ready: break
    try: captured += os.read(fd,8192)
    except OSError: break
if b"[Domains 0]" not in captured[before_resize:]:
    raise SystemExit("workspace TUI did not repaint after resize without input")
for key in (b"\t", b"\t", b"\t"):
    os.write(fd, key); time.sleep(.1)
os.write(fd, b"\t"); time.sleep(.3)
deadline=time.time()+5
while time.time()<deadline:
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
    if b"No active domains" in captured and b"Tab view" in captured: break
else: os.kill(pid,9); raise SystemExit("workspace TUI did not render views: " + captured[-4000:].decode(errors="replace"))
if b"No active domains" not in captured or b"Tab view" not in captured:
    raise SystemExit("workspace TUI did not render navigable detail views/footer")
os.write(fd, b"q")
deadline=time.time()+5
while time.time()<deadline:
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("workspace TUI did not close cleanly")
        break
    time.sleep(.05)
else: os.kill(pid,9); raise SystemExit("workspace TUI ignored clean close")
PY
kill -0 "$(cat "$TMP_ROOT/harness.pid")" || fail 'closing workspace TUI ended independent harness work'

python3 - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import os, pty, select, sys, time
binary, root, home, fake, record = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
captured=b""
def wait_for(marker, timeout=15):
    global captured
    deadline=time.time()+timeout
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.1)
        if ready:
            try: captured += os.read(fd,8192)
            except OSError: pass
        if marker in captured: return
        done,status=os.waitpid(pid,os.WNOHANG)
        if done: raise SystemExit("TUI exited before "+repr(marker)+": "+captured[-4000:].decode(errors="replace"))
    raise SystemExit("TUI did not render "+repr(marker)+": "+captured[-4000:].decode(errors="replace"))
wait_for(b"Tab view")
os.write(fd,b"/runtime"); wait_for(b"Filter: runtime")
os.write(fd,b"\rj"); wait_for(b"Suggested project: runtime")
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

python3 - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" <<'PY'
import os, pty, select, sys, time
binary, root, home, fake, record = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
captured=b""
def wait_for(marker, timeout=15):
    global captured
    start=len(captured); deadline=time.time()+timeout
    while time.time()<deadline:
        ready,_,_=select.select([fd],[],[],.1)
        if ready:
            try: captured += os.read(fd,8192)
            except OSError: pass
        if marker in captured[start:]: return
    raise SystemExit("navigation TUI missed "+repr(marker)+": "+captured[-3000:].decode(errors="replace"))
wait_for(b"Tab view")
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
os.write(fd,b"\x1b"); wait_for(b"Tab view")
os.write(fd,b"q")
deadline=time.time()+5
while time.time()<deadline:
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("navigation TUI did not close cleanly")
        break
    time.sleep(.05)
else: os.kill(pid,9); raise SystemExit("navigation TUI ignored clean close")
PY

python3 - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" "$socket" <<'PY'
import os, pty, select, subprocess, sys, time
binary, root, home, fake, record, socket = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.chdir("/")
    os.execve(binary, [binary, "launcher"], dict(os.environ, TERM="xterm-256color",
        MX_ROOT_OVERRIDE=root, MX_HOME=home, MX_REAL_CODEX=fake, MX_FAKE_RECORD=record))
captured=b""; deadline=time.time()+15
while time.time()<deadline and b"Tab view" not in captured:
    ready,_,_=select.select([fd],[],[],.1)
    if ready:
        try: captured += os.read(fd,8192)
        except OSError: pass
if b"Tab view" not in captured: raise SystemExit("context chat TUI did not render")
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
subprocess.run(["tmux","-L",socket,"detach-client","-s","primary"],check=True)
deadline=time.time()+5
while time.time()<deadline:
    done,status=os.waitpid(pid,os.WNOHANG)
    if done:
        if os.waitstatus_to_exitcode(status): raise SystemExit("selected TUI chat failed")
        break
    time.sleep(.05)
else: os.kill(pid,9); raise SystemExit("selected TUI chat did not return")
PY
find "$HOME_DIR/state/workspace-contexts" -type f -name '*.json' -print -quit | grep -q . \
  || fail 'selected TUI chat did not publish immutable next-request context'

python3 - "$BINARY" "$RUNTIME" "$HOME_DIR" "$FAKE" "$TMP_ROOT/harness" "$socket" <<'PY'
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
