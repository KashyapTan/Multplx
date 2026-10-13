#!/usr/bin/env python3
"""Deterministic Herdr transport for real Rust launch/allocation contracts."""
import json
import os
import signal
import subprocess
import sys
from pathlib import Path

args = sys.argv[1:]
if args[-2:-1] == ["--session"]:
    args = args[:-2]
if args == ["--version"]:
    print("herdr 0.7.4")
    sys.exit(0)
store = Path(os.environ["MX_HERDR_FIXTURE"])
state = json.loads(store.read_text())

def option(name):
    return args[args.index(name) + 1]

def result(kind, key, value):
    return {"result": {"type": kind, key: value}}

def process_start(pid):
    out = subprocess.run(["ps", "-p", str(pid), "-o", "lstart="],
                         capture_output=True, text=True, check=False)
    return out.stdout.strip() if out.returncode == 0 else None

operation = tuple(args[:2])
if args[0] == "status":
    value = {"client": {"version": "0.7.4", "protocol": 14},
             "server": {"running": True}}
elif operation == ("session", "list"):
    value = {"sessions": [{"name": os.environ["HERDR_SESSION"], "running": True,
                           "socket_path": str(store.parent / "herdr.sock")} ]}
elif operation == ("workspace", "list"):
    value = result("workspace_list", "workspaces", state["workspaces"])
elif operation == ("workspace", "create"):
    assert "--no-focus" in args
    state["next"] += 1
    workspace = {"workspace_id": f'w{state["next"]}', "label": option("--label"),
                 "focused": False}
    tab = {"workspace_id": workspace["workspace_id"],
           "tab_id": f'{workspace["workspace_id"]}:seed', "label": "1", "focused": False}
    pane = {"workspace_id": workspace["workspace_id"], "tab_id": tab["tab_id"],
            "pane_id": f'{workspace["workspace_id"]}:seed-pane',
            "foreground_cwd": option("--cwd")}
    state["workspaces"].append(workspace)
    state["tabs"].append(tab)
    state["panes"].append(pane)
    value = {"result": {"workspace": workspace, "tab": tab, "root_pane": pane}}
elif operation == ("workspace", "rename"):
    next(space for space in state["workspaces"] if space["workspace_id"] == args[2])["label"] = args[3]
    value = {}
elif operation == ("tab", "list"):
    value = result("tab_list", "tabs", [tab for tab in state["tabs"]
                                         if tab["workspace_id"] == option("--workspace")])
elif operation == ("tab", "get"):
    value = result("tab_info", "tab", next(tab for tab in state["tabs"] if tab["tab_id"] == args[2]))
elif operation == ("tab", "create"):
    assert "--no-focus" in args
    state["next"] += 1
    workspace = option("--workspace")
    tab = {"workspace_id": workspace, "tab_id": f'{workspace}:t{state["next"]}',
           "label": option("--label"), "focused": False}
    pane = {"workspace_id": workspace, "tab_id": tab["tab_id"],
            "pane_id": f'{workspace}:p{state["next"]}', "foreground_cwd": option("--cwd")}
    state["tabs"].append(tab)
    state["panes"].append(pane)
    value = {"result": {"tab": tab, "root_pane": pane}}
elif operation == ("pane", "list"):
    value = result("pane_list", "panes", [pane for pane in state["panes"]
                                           if pane["workspace_id"] == option("--workspace")])
elif operation == ("pane", "get"):
    pane = next((pane for pane in state["panes"] if pane["pane_id"] == args[2]), None)
    value = result("pane_info", "pane", pane) if pane else {"error": {"code": "pane_not_found"}}
elif operation == ("agent", "get"):
    value = {"error": {"code": "agent_not_found"}}
elif operation == ("pane", "send-text"):
    pane = next(pane for pane in state["panes"] if pane["pane_id"] == args[2])
    pane["input"] = args[3]
    value = {}
elif operation == ("pane", "read"):
    pane = next(pane for pane in state["panes"] if pane["pane_id"] == args[2])
    print("› " + pane.get("input", ""))
    sys.exit(0)
elif operation in (("pane", "run"), ("pane", "send-keys")):
    pane = next(pane for pane in state["panes"] if pane["pane_id"] == args[2])
    if operation == ("pane", "send-keys"):
        assert args[3].lower() == "enter", args
        script = pane.pop("input", "")
    else:
        script = args[3]
    subprocess.run(["bash", os.environ["MX_INERT_START"], script], check=True)
    pid = int(Path(os.environ["MX_TEST_INERT_PID_LOG"]).read_text().splitlines()[-1])
    pane["pid"] = pid
    pane["started"] = process_start(pid)
    value = {}
elif operation == ("pane", "close"):
    pane = next((pane for pane in state["panes"] if pane["pane_id"] == args[2]), None)
    if pane:
        pid = pane.get("pid")
        if pid and pane.get("started") and process_start(pid) == pane["started"]:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        state["panes"].remove(pane)
        if not any(item["tab_id"] == pane["tab_id"] for item in state["panes"]):
            state["tabs"] = [item for item in state["tabs"] if item["tab_id"] != pane["tab_id"]]
        if not any(item["workspace_id"] == pane["workspace_id"] for item in state["tabs"]):
            state["workspaces"] = [item for item in state["workspaces"] if item["workspace_id"] != pane["workspace_id"]]
    value = {}
else:
    raise AssertionError(f"unexpected Herdr fixture call: {args}")
store.write_text(json.dumps(state))
print(json.dumps(value))
