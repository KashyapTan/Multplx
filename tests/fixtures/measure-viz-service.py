#!/usr/bin/env python3
"""Measure the bounded MX Viz cache against deterministic portfolio fixtures."""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import platform
import resource
import shutil
import socket
import statistics
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

TRANSIENT_POLL_ERRORS = 0


def percentile(samples: list[float], fraction: float) -> float:
    ordered = sorted(samples)
    return ordered[max(0, int(len(ordered) * fraction + 0.999999) - 1)]


def summary(samples: list[float]) -> dict[str, object]:
    return {
        "count": len(samples),
        "p50_ms": round(statistics.median(samples), 3),
        "p95_ms": round(percentile(samples, 0.95), 3),
        "max_ms": round(max(samples), 3),
        "samples_ms": [round(value, 3) for value in samples],
    }


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def fetch(url: str, etag: str | None = None) -> tuple[int, dict[str, str], bytes, float]:
    request = urllib.request.Request(url, headers={"If-None-Match": etag} if etag else {})
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=3) as response:
            return response.status, dict(response.headers.items()), response.read(), (time.perf_counter() - started) * 1000
    except urllib.error.HTTPError as error:
        return error.code, dict(error.headers.items()), error.read(), (time.perf_counter() - started) * 1000


def wait_meta(url: str, predicate, timeout: float = 8) -> dict[str, object]:
    global TRANSIENT_POLL_ERRORS
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            status, _, body, _ = fetch(f"{url}api/meta")
        except OSError:
            TRANSIENT_POLL_ERRORS += 1
            time.sleep(0.01)
            continue
        if status == 200:
            value = json.loads(body)
            if predicate(value):
                return value
        time.sleep(0.01)
    raise RuntimeError("timed out waiting for viz metadata")


def wait_file(path: Path, timeout: float = 3) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.005)
    raise RuntimeError(f"timed out waiting for {path}")


def wait_probe_entry(path: Path, expected: int, timeout: float = 3) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists() and int((path.read_text() or "0").split()[0]) >= expected:
            return
        time.sleep(0.005)
    raise RuntimeError(f"timed out waiting for probe {expected} in {path}")


def trigger_refresh(url: str, etag: str, expected_successes: int) -> dict[str, object]:
    global TRANSIENT_POLL_ERRORS
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        try:
            fetch(f"{url}api/state", etag)
            status, _, body, _ = fetch(f"{url}api/meta")
        except OSError:
            TRANSIENT_POLL_ERRORS += 1
            time.sleep(0.01)
            continue
        if status == 200:
            value = json.loads(body)
            if value["metrics"]["refresh_successes"] >= expected_successes:
                return value
        time.sleep(0.01)
    raise RuntimeError("timed out triggering viz refresh")


def cpu_seconds(pid: int) -> float | None:
    output = subprocess.run(
        ["ps", "-o", "time=", "-p", str(pid)],
        check=False,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if not output:
        return None
    days = 0
    if "-" in output:
        day, output = output.split("-", 1)
        days = int(day)
    fields = [float(value) for value in output.split(":")]
    seconds = fields[-1] + (fields[-2] * 60 if len(fields) >= 2 else 0)
    if len(fields) >= 3:
        seconds += fields[-3] * 3600
    return days * 86400 + seconds


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def reader_source() -> str:
    return """#!/usr/bin/env python3
import os
import time
from pathlib import Path

count = Path(os.environ["MX_VIZ_COUNT_FILE"])
count.write_text(str(int(count.read_text() or "0") + 1) if count.exists() else "1")
delay = Path(os.environ["MX_VIZ_DELAY_FILE"])
if delay.exists():
    time.sleep(float(delay.read_text()))
failure = Path(os.environ["MX_VIZ_FAIL_FILE"])
if failure.exists():
    raise SystemExit("fixture refresh failed")
print(Path(os.environ["MX_VIZ_FIXTURE"]).read_text(), end="")
"""


def measure_count(root: Path, scratch: Path, task_count: int) -> dict[str, object]:
    case = scratch / f"tasks-{task_count}"
    for name in ["state", "data", "config", "projects", "readers"]:
        (case / name).mkdir(parents=True, exist_ok=True)
    fixture = case / "snapshot.json"
    fixture.write_bytes(
        subprocess.check_output(["node", str(root / "tests/fixtures/viz/portfolio.mjs"), str(task_count)])
    )
    reader = case / "readers/snapshot.py"
    reader.write_text(reader_source())
    reader.chmod(0o755)
    doctor = case / "readers/doctor.sh"
    doctor.write_text("#!/bin/sh\nprintf '%s\\n' '{\"exit_code\":0,\"findings\":[]}'\n")
    doctor.chmod(0o755)
    timeline = case / "readers/timeline.sh"
    timeline.write_text("#!/bin/sh\nexit 0\n")
    timeline.chmod(0o755)
    count_file = case / "snapshot.count"
    delay_file = case / "snapshot.delay"
    fail_file = case / "snapshot.fail"
    environment = os.environ.copy()
    for inherited in [
        "MX_VIZ_COMMAND_TIMEOUT_MS",
        "MX_SNAPSHOT_TASK_TIMEOUT",
        "MX_SNAPSHOT_TASK_CONCURRENCY",
    ]:
        environment.pop(inherited, None)
    environment.update(
        {
            "MX_HOME": str(case),
            "MX_VIZ_PORT": str(free_port()),
            "MX_VIZ_IDLE_SECS": "60",
            "MX_VIZ_REFRESH_SECS": "0.1",
            "MX_VIZ_COMMAND_TIMEOUT_MS": "2000",
            "MX_VIZ_SNAPSHOT_BIN": str(reader),
            "MX_VIZ_DOCTOR_BIN": str(doctor),
            "MX_VIZ_TIMELINE_BIN": str(timeline),
            "MX_VIZ_FIXTURE": str(fixture),
            "MX_VIZ_COUNT_FILE": str(count_file),
            "MX_VIZ_DELAY_FILE": str(delay_file),
            "MX_VIZ_FAIL_FILE": str(fail_file),
        }
    )
    url = subprocess.check_output([str(root / "bin/mx-viz.sh"), "serve"], env=environment, text=True).strip()
    try:
        status, headers, _, initial_ms = fetch(f"{url}api/state")
        if status != 200:
            raise RuntimeError(f"initial state returned {status}")
        etag = headers["ETag"]
        meta = json.loads(fetch(f"{url}api/meta")[2])
        pid = int(meta["pid"])
        cpu_before = cpu_seconds(pid)
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            cached = list(executor.map(lambda _: fetch(f"{url}api/state", etag)[3], range(50)))
        refresh_samples = []
        successes = int(meta["metrics"]["refresh_successes"])
        for _ in range(10):
            time.sleep(0.105)
            successes += 1
            meta = trigger_refresh(url, etag, successes)
            refresh_samples.append(float(meta["metrics"]["last_refresh_ms"]))
        result: dict[str, object] = {
            "task_count": task_count,
            "initial_refresh_ms": round(initial_ms, 3),
            "cached_conditional_api": summary(cached),
            "healthy_refresh": summary(refresh_samples),
        }
        if task_count == 20:
            delay_file.write_text("4")
            time.sleep(0.105)
            with concurrent.futures.ThreadPoolExecutor(max_workers=12) as executor:
                stalled = list(executor.map(lambda _: fetch(f"{url}api/state", etag), range(12)))
            if any(status != 304 for status, _, _, _ in stalled):
                raise RuntimeError("stalled-provider callers did not receive cached 304 responses")
            failed = wait_meta(url, lambda value: value["metrics"]["refresh_failures"] >= 1)
            result["stalled_provider_cached_api"] = summary([row[3] for row in stalled])
            result["stalled_provider_refresh_ms"] = failed["metrics"]["last_refresh_ms"]
            delay_file.unlink()
        final = json.loads(fetch(f"{url}api/meta")[2])
        cpu_after = cpu_seconds(pid)
        result["probe_count"] = int(count_file.read_text())
        result["metrics"] = final["metrics"]
        result["service_cpu_seconds_delta"] = (
            None if cpu_before is None or cpu_after is None else round(cpu_after - cpu_before, 3)
        )
        return result
    finally:
        subprocess.run(
            [str(root / "bin/mx-viz.sh"), "stop"],
            env=environment,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def measure_real_projection(root: Path, scratch: Path) -> dict[str, object]:
    case = scratch / "real-projection-20"
    subprocess.run(
        [str(root / "tests/fixtures/viz/real-portfolio-home.sh"), str(case)], check=True
    )
    (case / "readers").mkdir()
    count_file = case / "snapshot.count"
    reader = case / "readers/snapshot.sh"
    reader.write_text(
        "#!/bin/sh\n"
        "count=0\n"
        "[ ! -f \"$MX_VIZ_COUNT_FILE\" ] || count=$(cat \"$MX_VIZ_COUNT_FILE\")\n"
        "printf '%s' $((count + 1)) >\"$MX_VIZ_COUNT_FILE\"\n"
        f"exec '{root / 'target/release/mx'}' session mx-system-snapshot.sh \"$@\"\n"
    )
    reader.chmod(0o755)
    environment = os.environ.copy()
    environment.update(
        {
            "MX_HOME": str(case),
            "MX_VIZ_PORT": str(free_port()),
            "MX_VIZ_IDLE_SECS": "60",
            "MX_VIZ_REFRESH_SECS": "0.1",
            "MX_VIZ_COMMAND_TIMEOUT_MS": "2000",
            "MX_VIZ_SNAPSHOT_BIN": str(reader),
            "MX_VIZ_COUNT_FILE": str(count_file),
        }
    )
    url = subprocess.check_output([str(root / "bin/mx-viz.sh"), "serve"], env=environment, text=True).strip()
    try:
        status, headers, body, initial_ms = fetch(f"{url}api/state")
        if status != 200:
            raise RuntimeError(f"real projection initial state returned {status}: {body.decode(errors='replace')}")
        envelope = json.loads(body)
        portfolio = envelope["snapshot"]["portfolio"]
        if portfolio["counts"]["tasks"] != 20 or len(portfolio["tasks"]) != 20:
            raise RuntimeError("real projection did not collect 20 tasks")
        if portfolio["counts"]["sessions"] != 0:
            raise RuntimeError("legacy fixture invented execution sessions")
        etag = headers["ETag"]
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
            cached = list(executor.map(lambda _: fetch(f"{url}api/state", etag)[3], range(50)))
        meta = json.loads(fetch(f"{url}api/meta")[2])
        successes = int(meta["metrics"]["refresh_successes"])
        refresh_samples = []
        for _ in range(10):
            time.sleep(0.105)
            successes += 1
            meta = trigger_refresh(url, etag, successes)
            refresh_samples.append(float(meta["metrics"]["last_refresh_ms"]))
        return {
            "task_count": 20,
            "fixture_kind": "isolated real filesystem projection collection",
            "execution_visibility": "20 canonical legacy_unknown tasks; zero live provider sessions",
            "initial_refresh_ms": round(initial_ms, 3),
            "cached_conditional_api": summary(cached),
            "healthy_refresh": summary(refresh_samples),
            "probe_count": int(count_file.read_text()),
            "metrics": meta["metrics"],
        }
    finally:
        subprocess.run(
            [str(root / "bin/mx-viz.sh"), "stop"],
            env=environment,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def measure_collector(
    root: Path,
    source_root: Path,
    binary: Path,
    scratch: Path,
    label: str,
    collection_model: str,
) -> dict[str, object]:
    """Measure the shipping collector with a fixed-cost actor observation.

    The 50 ms provider shim makes serial versus bounded scheduling observable
    without claiming that the fixture is a live model/backend session.
    """
    case = scratch / f"collector-{label}"
    home = case / "home"
    subprocess.run(
        [str(root / "tests/fixtures/viz/real-portfolio-home.sh"), str(home)], check=True
    )
    shim_root = case / "source"
    (shim_root / "bin").mkdir(parents=True)
    actor = shim_root / "bin/mx-actor-state.sh"
    actor.write_text(
        "#!/bin/sh\n"
        "sleep 0.05\n"
        f"exec '{binary}' actor-state \"$@\"\n"
    )
    actor.chmod(0o755)
    environment = os.environ.copy()
    for inherited in ["MX_SNAPSHOT_TASK_TIMEOUT", "MX_SNAPSHOT_TASK_CONCURRENCY"]:
        environment.pop(inherited, None)
    environment.update(
        {
            "MX_HOME": str(home),
            "MX_ROOT_OVERRIDE": str(shim_root),
            "MX_RUST_SOURCE_ROOT": str(shim_root),
            "MX_RUST_BIN": str(binary),
            "MX_SNAPSHOT_NOW": "2026-09-18T02:00:00Z",
        }
    )
    wall_samples: list[float] = []
    cpu_samples: list[float] = []
    for _ in range(10):
        usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
        started = time.perf_counter()
        completed = subprocess.run(
            [str(binary), "session", "mx-system-snapshot.sh", "--json"],
            env=environment,
            check=True,
            capture_output=True,
            timeout=20,
        )
        wall_samples.append((time.perf_counter() - started) * 1000)
        usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
        cpu_samples.append(
            (usage_after.ru_utime + usage_after.ru_stime
             - usage_before.ru_utime - usage_before.ru_stime)
            * 1000
        )
        snapshot = json.loads(completed.stdout)
        if len(snapshot.get("tasks", [])) != 20:
            raise RuntimeError(f"{label} collector did not observe 20 task records")
    return {
        "label": label,
        "collection_model": collection_model,
        "source_revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=source_root, text=True
        ).strip(),
        "source_worktree_dirty": bool(
            subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=source_root, text=True
            ).strip()
        ),
        "source_sha256": sha256(source_root / "crates/multplx-cli/src/system_snapshot.rs"),
        "binary_sha256": sha256(binary),
        "task_count": 20,
        "samples": 10,
        "provider_fixture": "synthetic 50 ms actor observation over a real 20-record filesystem home",
        "actor_timeout_ms": 2000,
        "task_concurrency": 1 if collection_model.startswith("serial") else 4,
        "wall_latency": summary(wall_samples),
        "child_cpu_ms": summary(cpu_samples),
        "collector_invocations": 10,
    }


def measure_real_stalled_projection(root: Path, binary: Path, scratch: Path) -> dict[str, object]:
    """Exercise one timed-out actor while a real sibling status event advances."""
    case = scratch / "real-stalled-projection"
    home = case / "home"
    subprocess.run(
        [str(root / "tests/fixtures/viz/real-portfolio-home.sh"), str(home)], check=True
    )
    shim_root = case / "source"
    (shim_root / "bin").mkdir(parents=True)
    stall = case / "stall"
    entered = case / "stalled-provider-entered"
    actor = shim_root / "bin/mx-actor-state.sh"
    actor.write_text(
        "#!/bin/sh\n"
        f"if [ \"$1\" = task-1 ] && [ -f '{stall}' ]; then : >'{entered}'; sleep 4; fi\n"
        f"exec '{binary}' actor-state \"$@\"\n"
    )
    actor.chmod(0o755)
    readers = case / "readers"
    readers.mkdir()
    count_file = case / "snapshot.count"
    reader = readers / "snapshot.sh"
    reader.write_text(
        "#!/bin/sh\n"
        "count=0\n"
        "[ ! -f \"$MX_VIZ_COUNT_FILE\" ] || count=$(cat \"$MX_VIZ_COUNT_FILE\")\n"
        "printf '%s' $((count + 1)) >\"$MX_VIZ_COUNT_FILE\"\n"
        f"MX_ROOT_OVERRIDE='{shim_root}' MX_RUST_SOURCE_ROOT='{shim_root}' exec '{binary}' session mx-system-snapshot.sh \"$@\"\n"
    )
    reader.chmod(0o755)
    environment = os.environ.copy()
    for inherited in [
        "MX_VIZ_COMMAND_TIMEOUT_MS",
        "MX_SNAPSHOT_TASK_TIMEOUT",
        "MX_SNAPSHOT_TASK_CONCURRENCY",
    ]:
        environment.pop(inherited, None)
    environment.update(
        {
            "MX_HOME": str(home),
            "MX_ROOT_OVERRIDE": str(shim_root),
            "MX_VIZ_PORT": str(free_port()),
            "MX_VIZ_IDLE_SECS": "60",
            "MX_VIZ_REFRESH_SECS": "0.1",
            "MX_VIZ_SNAPSHOT_BIN": str(reader),
            "MX_VIZ_COUNT_FILE": str(count_file),
        }
    )
    url = subprocess.check_output(
        [str(root / "bin/mx-viz.sh"), "serve"], env=environment, text=True
    ).strip()
    try:
        status, headers, _, _ = fetch(f"{url}api/state")
        if status != 200:
            raise RuntimeError(f"stalled projection initial state returned {status}")
        etag = headers["ETag"]
        meta = json.loads(fetch(f"{url}api/meta")[2])
        pid = int(meta["pid"])
        cpu_before = cpu_seconds(pid)
        successes = int(meta["metrics"]["refresh_successes"])
        stall.write_text("1")
        event_id = "healthy-event-during-stall"
        event_text = f"working [key={event_id}]: healthy sibling advanced"
        wake_epoch = int(time.time())
        (home / "state/.wake-queue").write_text(
            f"{wake_epoch}\tsignal\tbenchmark pending wake\t"
            f"{home / 'state/task-2.status'}\tbenchmark-wake\n"
        )
        event_written = time.perf_counter()
        (home / "state/task-2.status").write_text(event_text + "\n")
        time.sleep(0.105)
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as executor:
            callers = list(executor.map(lambda _: fetch(f"{url}api/state", etag), range(12)))
        wait_file(entered)
        if any(row[0] != 304 for row in callers):
            raise RuntimeError(
                "multi-viewer stalled collector did not serve cached 304 responses: "
                + repr([row[0] for row in callers])
            )
        deadline = time.monotonic() + 8
        visible = None
        while time.monotonic() < deadline:
            try:
                status, response_headers, body, _ = fetch(f"{url}api/state", etag)
            except OSError:
                global TRANSIENT_POLL_ERRORS
                TRANSIENT_POLL_ERRORS += 1
                time.sleep(0.01)
                continue
            if status == 200:
                candidate = json.loads(body)
                healthy_rows = [
                    task
                    for task in candidate["snapshot"]["portfolio"]["tasks"]
                    if task.get("id") == "task-2"
                ]
                if len(healthy_rows) == 1 and healthy_rows[0]["latest_change"]["raw"] == event_text:
                    visible = (response_headers, candidate)
                    break
            time.sleep(0.01)
        if visible is None:
            raise RuntimeError("healthy sibling event was not exposed through stalled collection")
        exposed_ms = (time.perf_counter() - event_written) * 1000
        wake = visible[1]["snapshot"]["wake_queue"]
        final = wait_meta(
            url, lambda value: value["metrics"]["refresh_successes"] >= successes + 1, timeout=3
        )
        stalled_rows = [
            task
            for task in visible[1]["snapshot"]["tasks"]
            if task.get("id") == "task-1"
        ]
        if len(stalled_rows) != 1 or stalled_rows[0]["current_state"]["source"] != "timeout":
            raise RuntimeError("stalled actor did not remain an explicit timeout observation")
        probes = int(count_file.read_text())
        if probes != 2 or final["metrics"]["refresh_attempts"] != 2:
            raise RuntimeError(
                f"12 viewers caused extra collector work: probes={probes}, "
                f"attempts={final['metrics']['refresh_attempts']}"
            )
        cpu_after = cpu_seconds(pid)
        return {
            "fixture_kind": "real filesystem collector with one synthetic stalled actor provider",
            "task_count": 20,
            "viewer_count": 12,
            "service_command_timeout_ms": 10000,
            "service_timeout_source": "shipping default; MX_VIZ_COMMAND_TIMEOUT_MS unset",
            "actor_timeout_ms": 2000,
            "task_concurrency": 4,
            "event_id": event_id,
            "event_to_fresh_view_ms": round(exposed_ms, 3),
            "pending_wake_depth_at_exposure": wake["depth"],
            "oldest_pending_wake_age_seconds_at_exposure": wake["oldest_age_secs"],
            "pending_wake_scope": "isolated queue-age observation only; no consumer or disposition trial",
            "stalled_provider_cached_api": summary([row[3] for row in callers]),
            "snapshot_refresh_ms": final["metrics"]["last_refresh_ms"],
            "probe_count": probes,
            "cache_metrics": final["metrics"],
            "service_cpu_seconds_delta": (
                None if cpu_before is None or cpu_after is None else round(cpu_after - cpu_before, 3)
            ),
            "partial_snapshot": visible[1]["snapshot"]["portfolio"]["freshness"]["partial"],
            "binary_sha256": sha256(binary),
        }
    finally:
        subprocess.run(
            [str(root / "bin/mx-viz.sh"), "stop"],
            env=environment,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def measure_baseline_mutex(
    root: Path, baseline_root: Path, baseline_binary: Path, scratch: Path
) -> dict[str, object]:
    case = scratch / "baseline-mutex"
    for name in ["state", "data", "config", "projects", "readers"]:
        (case / name).mkdir(parents=True, exist_ok=True)
    fixture = case / "snapshot.json"
    fixture.write_bytes(
        subprocess.check_output(["node", str(root / "tests/fixtures/viz/portfolio.mjs"), "20"])
    )
    reader = case / "readers/snapshot.py"
    reader.write_text(
        """#!/usr/bin/env python3
import os
import time
from pathlib import Path

count_file = Path(os.environ["MX_VIZ_COUNT_FILE"])
count = int(count_file.read_text() or "0") + 1 if count_file.exists() else 1
count_file.write_text(str(count))
if count > 1:
    Path(os.environ["MX_VIZ_ENTERED_FILE"]).write_text(f"{count} {time.time_ns()}")
    time.sleep(0.5)
    Path(os.environ["MX_VIZ_COMPLETED_FILE"]).write_text(f"{count} {time.time_ns()}")
print(Path(os.environ["MX_VIZ_FIXTURE"]).read_text(), end="")
"""
    )
    reader.chmod(0o755)
    count_file = case / "snapshot.count"
    entered_file = case / "snapshot.entered"
    completed_file = case / "snapshot.completed"
    environment = os.environ.copy()
    environment.update(
        {
            "MX_HOME": str(case),
            "MX_RUST_BIN": str(baseline_binary),
            "MX_VIZ_PORT": str(free_port()),
            "MX_VIZ_IDLE_SECS": "60",
            "MX_VIZ_REFRESH_SECS": "1",
            "MX_VIZ_SNAPSHOT_BIN": str(reader),
            "MX_VIZ_DOCTOR_BIN": str(reader),
            "MX_VIZ_TIMELINE_BIN": str(reader),
            "MX_VIZ_FIXTURE": str(fixture),
            "MX_VIZ_COUNT_FILE": str(count_file),
            "MX_VIZ_ENTERED_FILE": str(entered_file),
            "MX_VIZ_COMPLETED_FILE": str(completed_file),
        }
    )
    url = subprocess.check_output(
        [str(baseline_root / "bin/mx-viz.sh"), "serve"], env=environment, text=True
    ).strip()
    try:
        status, _, _, _ = fetch(f"{url}api/state")
        if status != 200:
            raise RuntimeError(f"baseline initial state returned {status}")
        baseline_meta = json.loads(fetch(f"{url}api/meta")[2])
        if "metrics" in baseline_meta or "refresh" in baseline_meta:
            raise RuntimeError("baseline service exposes Phase 10 cache fields; binary is contaminated")
        initial_probe_count = int(count_file.read_text())
        entered_file.unlink(missing_ok=True)
        completed_file.unlink(missing_ok=True)
        time.sleep(1.05)
        batch_started = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=13) as executor:
            state_futures = [executor.submit(fetch, f"{url}api/state")]
            wait_probe_entry(entered_file, initial_probe_count + 1)
            if completed_file.exists():
                raise RuntimeError("baseline reader completed before concurrent requests began")
            state_futures.extend(
                executor.submit(fetch, f"{url}api/state") for _ in range(11)
            )
            meta_future = executor.submit(fetch, f"{url}api/meta")
            states = [future.result() for future in state_futures]
            meta = meta_future.result()
        batch_ms = (time.perf_counter() - batch_started) * 1000
        wait_file(completed_file)
        entered_fields = entered_file.read_text().split()
        completed_fields = completed_file.read_text().split()
        reader_wall_ms = (int(completed_fields[1]) - int(entered_fields[1])) / 1_000_000
        if states[0][3] < 450 or batch_ms < 450 or reader_wall_ms < 450:
            raise RuntimeError(
                "baseline request did not span the synchronized 500 ms reader stall: "
                f"trigger={states[0][3]:.3f}, batch={batch_ms:.3f}, reader={reader_wall_ms:.3f}"
            )
        return {
            "revision": "fa60d1ca46c3834f4b582adf7a5c7c7fb3dbabc3",
            "service_source_sha256": sha256(
                baseline_root / "crates/multplx-services/src/local_services/viz.rs"
            ),
            "binary_sha256": sha256(baseline_binary),
            "behavior": "expired refresh invokes the collector while holding the service runtime mutex",
            "stalled_reader_seconds": 0.5,
            "effective_refresh_seconds": 1,
            "synchronization": "the reader wrote snapshot.entered before concurrent state/meta calls",
            "concurrent_state_callers": 12,
            "state_api": summary([row[3] for row in states]),
            "meta_api_ms": round(meta[3], 3),
            "batch_wall_ms": round(batch_ms, 3),
            "observed_reader_wall_ms": round(reader_wall_ms, 3),
            "probe_count": int(count_file.read_text()),
            "limitation": "The baseline had a fixed 60 second snapshot deadline; this 0.5 second reader completed normally.",
        }
    finally:
        subprocess.run(
            [str(baseline_root / "bin/mx-viz.sh"), "stop"],
            env=environment,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    parser.add_argument("--baseline-root", type=Path)
    parser.add_argument("--baseline-binary", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    current_binary = root / "target/release/mx"
    if not current_binary.is_file():
        raise RuntimeError("build target/release/mx before measuring")
    scratch = Path(tempfile.mkdtemp(prefix="mx-viz-measure."))
    try:
        results = [measure_count(root, scratch, count) for count in [1, 5, 10, 20]]
        real_projection = measure_real_projection(root, scratch)
        stalled_projection = measure_real_stalled_projection(root, current_binary, scratch)
        source = root / "crates/multplx-services/src/local_services/viz.rs"
        document = {
            "schema": "mx-viz-service-performance.v1",
            "measured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
            "working_tree_dirty": bool(
                subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True).strip()
            ),
            "service_source_sha256": sha256(source),
            "release_binary_sha256": sha256(current_binary),
            "machine": {
                "platform": platform.platform(),
                "architecture": platform.machine(),
                "logical_cpus": os.cpu_count(),
                "load_average_after": [round(value, 3) for value in os.getloadavg()],
                "python": platform.python_version(),
                "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
            },
            "workload": {
                "fixture": "tests/fixtures/viz/portfolio.mjs",
                "fixture_sha256": sha256(root / "tests/fixtures/viz/portfolio.mjs"),
                "real_home_fixture": "tests/fixtures/viz/real-portfolio-home.sh",
                "real_home_fixture_sha256": sha256(
                    root / "tests/fixtures/viz/real-portfolio-home.sh"
                ),
                "fixture_kind": "deterministic mocked service input",
                "cached_samples_per_scale": 50,
                "healthy_refresh_samples_per_scale": 10,
                "cached_callers": 8,
                "stalled_provider_callers": 12,
                "refresh_seconds": 0.1,
                "command_timeout_ms": 2000,
                "run_condition": "manual local run after aggregate validation; no intentional competing benchmark workload",
            },
            "measurement_script_sha256": sha256(Path(__file__)),
            "targets_ms": {"cached_api_p95": 250, "healthy_20_task_refresh_p95": 2000},
            "measurement_diagnostics": {
                "transient_condition_poll_errors": TRANSIENT_POLL_ERRORS,
                "sampled_request_errors": 0,
                "note": "Condition-poll retries are counted; any failed sampled API call aborts the run.",
            },
            "results": results,
            "real_projection_20_task": real_projection,
            "real_stalled_projection_20_task": stalled_projection,
            "limitations": [
                "These service-isolation fixtures do not claim live provider, model, browser-interactivity, or end-to-end projection performance.",
                "The real projection workload exercises the shipping filesystem collector over 20 task records, but intentionally has no live provider sessions or backend probes.",
                "The stalled real projection uses the shipping collector and real status files with one deterministic sleeping actor shim; it does not claim a live provider outage.",
                "Process CPU time has operating-system reporting granularity and may remain zero for this short fixture run.",
            ],
        }
        if args.baseline_root and args.baseline_binary:
            document["baseline_mutex_behavior"] = measure_baseline_mutex(
                root, args.baseline_root, args.baseline_binary, scratch
            )
            document["collector_comparison_20_task"] = {
                "method": "same real 20-record home shape and deterministic 50 ms actor observation; ten full system snapshots per implementation",
                "baseline": measure_collector(
                    root,
                    args.baseline_root,
                    args.baseline_binary,
                    scratch,
                    "historical-serial",
                    "serial per-task subprocess collection",
                ),
                "current": measure_collector(
                    root,
                    root,
                    current_binary,
                    scratch,
                    "current-bounded",
                    "bounded four-worker task collection",
                ),
            }
        document["measurement_diagnostics"]["transient_condition_poll_errors"] = (
            TRANSIENT_POLL_ERRORS
        )
        encoded = json.dumps(document, indent=2) + "\n"
        if args.output:
            args.output.write_text(encoded)
        print(encoded, end="")
    finally:
        shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
