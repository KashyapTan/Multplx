#!/usr/bin/env python3
"""Serve synthetic attention fixtures and current UI on an isolated ephemeral port.

Browser evidence only: never reads an operational home, launches mx, or writes state.
"""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
snapshot = json.loads(subprocess.check_output(["node", str(Path(__file__).with_name("attention.mjs"))]))
payload = json.dumps({"snapshot": snapshot}).encode()
assets = {
    "/": ("text/html", (ROOT / "share/viz/index.html").read_bytes().replace(b"__MX_VIZ_POLL_MS__", b"2500")),
    "/api/state": ("application/json", payload),
    **{f"/assets/{name}": (kind, (ROOT / "share/viz" / name).read_bytes()) for name, kind in [
        ("app.css", "text/css"), ("app.js", "text/javascript"), ("agents-graph.js", "text/javascript")
    ]},
}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        kind, body = assets.get(self.path.split("?")[0], ("text/plain", b"Fixture route unavailable"))
        self.send_response(200 if self.path.split("?")[0] in assets else 404)
        self.send_header("Content-Type", f"{kind}; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("X-Multplx-Cache", "fresh")
        self.send_header("X-Multplx-Observation-Age-Ms", "0")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
print(f"http://127.0.0.1:{server.server_port}/", flush=True)
try:
    server.serve_forever()
except KeyboardInterrupt:
    pass
finally:
    server.server_close()
