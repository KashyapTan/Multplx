#!/usr/bin/env python3
"""Reproducible local Phase03 lifecycle cost observations, not throughput claims.

Run after a release build:
  python3 tests/fixtures/measure-worktrees.py --binary target/release/mx \
    --output plans/lean_redesign/phase03-worktree-costs.json \
    --overlap 'Describe concurrent validation or host workloads'

Every Git mutation and runtime home is confined to a temporary fixture.
Setup means exact-cwd Git readiness observation, not endpoint/model startup.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time
from datetime import datetime, timezone


def checked(argv, env, cwd, expected=0):
    started = time.perf_counter()
    result = subprocess.run(
        [str(value) for value in argv], cwd=cwd, env=env,
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=90,
    )
    elapsed = (time.perf_counter() - started) * 1000
    if result.returncode != expected:
        raise RuntimeError(f"{argv[0:3]}: exit {result.returncode}: {result.stderr}")
    return result, elapsed


def disk(path):
    logical = allocated = files = 0
    for directory, _, names in os.walk(path, followlinks=False):
        for name in names:
            item = Path(directory) / name
            if item.is_symlink():
                continue
            stat = item.stat()
            if item.is_file():
                logical += stat.st_size
                allocated += stat.st_blocks * 512
                files += 1
    return {"logical_bytes": logical, "allocated_bytes": allocated, "files": files}


def summary(values):
    return {
        "observations": len(values), "total_ms": round(sum(values), 3),
        "median_ms": round(statistics.median(values), 3),
        "min_ms": round(min(values), 3), "max_ms": round(max(values), 3),
    }


def trial(count, root, binary, environment):
    base_dir = root / f"tasks-{count}"
    base_dir.mkdir()
    project = base_dir / "borrowed-project"
    homes = [base_dir / "home-a", base_dir / "home-b"]
    for home in homes:
        home.mkdir()
    project.mkdir()
    env = {**environment, "HOME": str(homes[0]), "MX_HOME": str(homes[0])}
    git = lambda *args, cwd=project: checked(["git", "-C", cwd, *args], env, base_dir)[0].stdout.strip()
    git("init", "-q", "-b", "main")
    git("config", "user.name", "Phase03 Measurement")
    git("config", "user.email", "phase03@example.invalid")
    (project / "tracked.bin").write_bytes(bytes(range(256)) * 4096)
    (project / ".gitignore").write_text("cache/\n")
    git("add", ".")
    git("commit", "-qm", "measurement fixture")
    base = git("rev-parse", "HEAD")
    original = (git("rev-parse", "HEAD"), git("status", "--porcelain"))
    timings = {name: [] for name in ["acquire", "same_request_retry", "setup_readiness", "release", "prune_preview", "prune_apply"]}
    allocations = []
    row = {"task_count": count, "homes": 2, "execution": "serial, alternating owner homes", "disk_before": disk(base_dir)}

    def mx(home, *args, expected=0):
        return checked([binary, "worktree", *args], {**env, "HOME": str(home), "MX_HOME": str(home)}, base_dir, expected)

    def acquire(home, request):
        args = ["acquire", project, "--request", request, "--task", request, "--attempt", f"attempt-{request}", "--base", base]
        result, elapsed = mx(home, *args)
        return json.loads(result.stdout), elapsed, args

    def disposition(home, allocation, operation, apply=False, expected=0):
        token = allocation["binding"]
        return mx(home, operation, project, token["allocation_id"], "--lease", token["lease_id"], "--generation", str(token["generation"]), "--attempt", token["attempt_id"], *(["--apply"] if apply else []), expected=expected)

    for index in range(count):
        home = homes[index % 2]
        allocation, elapsed, args = acquire(home, f"task-{index}")
        timings["acquire"].append(elapsed)
        result, elapsed = mx(home, *args)
        assert json.loads(result.stdout) == allocation, "retry changed ownership"
        timings["same_request_retry"].append(elapsed)
        path = Path(allocation["binding"]["path"])
        started = time.perf_counter()
        assert git("rev-parse", "--show-toplevel", cwd=path) == str(path)
        assert git("rev-parse", "HEAD", cwd=path) == base
        assert not git("status", "--porcelain", "--untracked-files=all", cwd=path)
        timings["setup_readiness"].append((time.perf_counter() - started) * 1000)
        allocations.append((home, allocation))
    assert len({a["binding"]["path"] for _, a in allocations}) == count
    assert len({a["binding"]["lease_id"] for _, a in allocations}) == count
    row["disk_after_acquire"] = disk(base_dir)
    for home, allocation in allocations:
        _, elapsed = disposition(home, allocation, "release")
        timings["release"].append(elapsed)
    row["disk_after_release"] = disk(base_dir)
    for home, allocation in allocations:
        _, elapsed = disposition(home, allocation, "prune")
        timings["prune_preview"].append(elapsed)
        assert Path(allocation["binding"]["path"]).is_dir()
        _, elapsed = disposition(home, allocation, "prune", apply=True)
        timings["prune_apply"].append(elapsed)
        assert not Path(allocation["binding"]["path"]).exists()
    row["disk_after_prune"] = disk(base_dir)
    cache_allocation, _, _ = acquire(homes[0], "cache-retained")
    cached_path = Path(cache_allocation["binding"]["path"])
    cache = cached_path / "cache" / "unclassified.bin"
    cache.parent.mkdir()
    cache.write_bytes(b"cache" * 52429)
    result, retained_ms = disposition(homes[0], cache_allocation, "release", expected=1)
    assert "ignored" in result.stderr and cache.is_file()
    result, _ = mx(homes[0], "inspect", project, cache_allocation["binding"]["allocation_id"])
    assert json.loads(result.stdout)["state"] == "retained"
    fallback, fallback_ms, _ = acquire(homes[1], "fresh-after-cache")
    fallback_path = Path(fallback["binding"]["path"])
    assert fallback_path != cached_path and not (fallback_path / "cache").exists()
    disposition(homes[1], fallback, "release")
    disposition(homes[1], fallback, "prune", apply=True)
    assert cache.read_bytes() == b"cache" * 52429
    assert original == (git("rev-parse", "HEAD"), git("status", "--porcelain"))
    row["cache_behavior"] = {
        "unclassified_ignored_bytes": cache.stat().st_size,
        "release_refused_retained_ms": round(retained_ms, 3),
        "fresh_fallback_acquire_ms": round(fallback_ms, 3),
        "cache_preserved": True, "fresh_path_distinct": True,
        "cache_copied_to_fresh_path": False,
        "disk_after_retained_cache_and_fallback_prune": disk(base_dir),
    }
    row["borrowed_checkout_unchanged"] = True
    row["timings"] = {name: summary(values) for name, values in timings.items()}
    row["samples_ms"] = {name: [round(value, 3) for value in values] for name, values in timings.items()}
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/mx"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--overlap", default="Concurrent host workloads were not controlled.")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if not shutil.which("git") or not shutil.which("lsof"):
        raise RuntimeError("real Git and lsof are required")
    environment = {key: value for key, value in os.environ.items() if not key.startswith(("MX_", "GIT_"))}
    environment.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0", "LC_ALL": "C"})
    report = {
        "schema_version": 1, "started_utc": datetime.now(timezone.utc).isoformat(),
        "host": {"platform": platform.platform(), "machine": platform.machine(), "logical_cpus": os.cpu_count(), "load_average_start": os.getloadavg()},
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "git_version": subprocess.check_output(["git", "--version"], text=True).strip(),
        "overlap": args.overlap,
        "method": {
            "trials": [1, 5, 10, 20], "repetitions_per_count": 1,
            "repository": "local-only temporary Git repository; 1 MiB tracked file, cache/ ignored",
            "timing": "wall clock per serial CLI operation; includes process startup, registry validation and Git/lsof",
            "setup": "three Git readiness observations at exact allocated cwd: top-level, exact HEAD, clean status; no endpoint or harness startup",
            "same_request_retry": "idempotent active allocation reconciliation, not idle warm worktree reuse",
            "idle_warm_reuse": "not applicable: implementation creates fresh paths instead of reusing disposed paths",
            "disk": "regular-file logical bytes and st_blocks*512 allocated bytes, includes local Git records and both homes; no symlink following",
            "cleanup": "manager prunes only clean disposed allocations; retained cache stays until temporary test fixture destruction",
            "limitations": "one small local fixture per task count, serial and workstation-loaded; no model throughput, concurrent acquisition scaling, endpoint latency or production repository extrapolation",
        }, "trials": [],
    }
    with tempfile.TemporaryDirectory(prefix="mx-phase03-costs-") as temporary:
        root = Path(temporary).resolve()
        fakebin = root / "sentinels"
        fakebin.mkdir()
        sentinel_log = root / "unexpected-provider-or-download"
        for tool in ["treehouse", "curl", "wget"]:
            path = fakebin / tool
            path.write_text(f"#!/bin/sh\nprintf invoked >> '{sentinel_log}'\nexit 99\n")
            path.chmod(0o755)
        environment["PATH"] = f"{fakebin}{os.pathsep}{environment['PATH']}"
        for count in report["method"]["trials"]:
            print(f"measuring {count} task allocations", flush=True)
            report["trials"].append(trial(count, root, binary, environment))
        assert not sentinel_log.exists(), "provider or downloader invoked"
        report["provider_and_download_sentinels_untouched"] = True
    report["finished_utc"] = datetime.now(timezone.utc).isoformat()
    report["host"]["load_average_end"] = os.getloadavg()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    staging = args.output.with_suffix(".json.tmp")
    staging.write_text(json.dumps(report, indent=2) + "\n")
    staging.replace(args.output)
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
