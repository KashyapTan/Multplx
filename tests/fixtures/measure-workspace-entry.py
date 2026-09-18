#!/usr/bin/env python3
"""Measure Phase 11 local discovery, intake, projection, and allocation startup.

Run after a release build:
  python3 tests/fixtures/measure-workspace-entry.py \
    --binary target/release/mx \
    --output plans/lean_redesign/phase11-workspace-entry-performance.json

This fixture uses only temporary local Git repositories. It does not start a model
provider or measure model throughput, execution time, or human response latency.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import time


def run(argv, env, cwd):
    started = time.perf_counter()
    result = subprocess.run(
        [str(value) for value in argv], cwd=cwd, env=env,
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120,
    )
    elapsed = (time.perf_counter() - started) * 1000
    if result.returncode:
        raise RuntimeError(f"{argv[:3]}: exit {result.returncode}: {result.stderr}")
    return result, round(elapsed, 3)


def source_fingerprint(source, excluded):
    listed = subprocess.check_output(
        ["git", "-C", source, "ls-files", "--cached", "--others", "--exclude-standard", "-z"]
    ).split(b"\0")
    digest = hashlib.sha256()
    files = 0
    for raw in sorted(item for item in listed if item):
        relative = Path(os.fsdecode(raw))
        if not (
            relative.name in {"Cargo.toml", "Cargo.lock"}
            or relative.parts[:1] in {("crates",), ("bin",), ("share",)}
            or relative == Path("tests/fixtures/measure-workspace-entry.py")
        ):
            continue
        path = source / relative
        if path.resolve() == excluded or not path.is_file() or path.is_symlink():
            continue
        digest.update(raw + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
        files += 1
    return digest.hexdigest(), files


def stats(samples):
    return {
        "observations": len(samples), "total_ms": round(sum(samples), 3),
        "median_ms": round(statistics.median(samples), 3),
        "min_ms": min(samples), "max_ms": max(samples), "samples_ms": samples,
    }


def trial(count, temporary, binary, source, base_env):
    root = temporary / f"tasks-{count}"
    home = root / "home"
    projects = root / "projects"
    for path in [home / "state", home / "data", home / "config", projects]:
        path.mkdir(parents=True, exist_ok=True)
    env = {
        **base_env, "MX_HOME": str(home), "MX_ROOT_OVERRIDE": str(source),
        "MX_STATE_OVERRIDE": str(home / "state"),
        "MX_DATA_OVERRIDE": str(home / "data"),
        "MX_CONFIG_OVERRIDE": str(home / "config"),
    }

    def command(*args):
        return run([binary, *args], env, root)

    repos = []
    starts = []
    for index in range(count):
        repo = projects / f"repo-{index:02d}"
        repo.mkdir()
        run(["git", "init", "-q", "-b", "main", repo], base_env, root)
        (repo / "base.txt").write_text(f"repository {index}\n")
        run(["git", "-C", repo, "add", "base.txt"], base_env, root)
        run([
            "git", "-C", repo, "-c", "user.name=Phase11 Measurement",
            "-c", "user.email=phase11@example.invalid", "commit", "-qm", "base",
        ], base_env, root)
        start, _ = run(["git", "-C", repo, "rev-parse", "HEAD"], base_env, root)
        repos.append(repo)
        starts.append(start.stdout.strip())

    command("project", "roots", "add", projects, "--depth", "1")
    discovery, discovery_ms = command("project", "discover", "--refresh")
    discovered = json.loads(discovery.stdout)
    intake_samples = []
    identities = []
    for index, repo in enumerate(repos):
        request = f"request-{count}-{index:02d}"
        task = f"task-{count}-{index:02d}"
        receipt, elapsed = command(
            "task", "--project", repo, "--request-id", request,
            "--batch-id", f"batch-{count}", "--task-id", task,
            f"Measure local intake for repository {index}",
        )
        value = json.loads(receipt.stdout)
        assert value["request"]["starting_revision"] == starts[index]
        intake_samples.append(elapsed)
        identities.append({
            "request_id": request, "task_id": task,
            "project_id": value["request"]["project_id"],
            "checkout_id": value["request"]["checkout_id"],
            "starting_revision": value["request"]["starting_revision"],
        })

    snapshot, snapshot_ms = command("session", "mx-system-snapshot.sh", "--json")
    projected = json.loads(snapshot.stdout)
    intake_rows = [
        row for row in projected["portfolio"]["tasks"]
        if row.get("intake", {}).get("batch_id") == f"batch-{count}"
    ]
    allocation_samples = []
    allocations = []
    for index, repo in enumerate(repos):
        allocated, elapsed = command(
            "worktree", "acquire", repo, "--request", identities[index]["request_id"],
            "--task", identities[index]["task_id"], "--attempt", f"attempt-{count}-{index:02d}",
            "--base", starts[index],
        )
        binding = json.loads(allocated.stdout)["binding"]
        assert Path(binding["path"]).is_dir()
        allocations.append({
            "allocation_id": binding["allocation_id"], "task_id": identities[index]["task_id"],
            "starting_revision": binding["base_revision"],
        })
        allocation_samples.append(elapsed)

    inbox_count = len(list((home / "state" / "request-inbox").glob("*.json")))
    assert len(discovered["candidates"]) == count
    assert len(intake_rows) == count and inbox_count == count and len(allocations) == count
    return {
        "task_count": count,
        "discovery": {"elapsed_ms": discovery_ms, "candidate_count": len(discovered["candidates"]),
                      "scanned_directories": discovered["scanned_directories"], "status": discovered["status"]},
        "intake": stats(intake_samples), "portfolio_projection_ms": snapshot_ms,
        "portfolio_intake_rows": len(intake_rows), "request_inbox_count": inbox_count,
        "wake_queue_depth": projected["wake_queue"]["depth"],
        "allocation_startup": stats(allocation_samples),
        "identities": identities, "allocations": allocations,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/mx"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--counts", default="1,5,10,20")
    parser.add_argument("--overlap", default="Other workstation workloads were not controlled.")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    source = Path(__file__).resolve().parents[2]
    if not shutil.which("git"):
        raise RuntimeError("real local Git is required")
    environment = {key: value for key, value in os.environ.items() if not key.startswith(("MX_", "GIT_"))}
    environment.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
                        "GIT_TERMINAL_PROMPT": "0", "LC_ALL": "C"})
    source_hash, source_files = source_fingerprint(source, args.output.resolve())
    report = {
        "schema": "mx-phase11-workspace-entry-measurement.v1",
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "machine": {"platform": platform.platform(), "architecture": platform.machine(),
                    "logical_cpus": os.cpu_count(), "load_average_start": os.getloadavg()},
        "binary": {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()},
        "source": {"revision": subprocess.check_output(["git", "-C", source, "rev-parse", "HEAD"], text=True).strip(),
                   "tree_sha256": source_hash, "files": source_files},
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "git_version": subprocess.check_output(["git", "--version"], text=True).strip(),
        "overlap": args.overlap,
        "method": {
            "counts": [int(value) for value in args.counts.split(",")], "samples_per_count": 1,
            "execution": "serial CLI processes over temporary independent local Git repositories",
            "timing": "wall clock including process startup and local filesystem/Git work",
            "allocation_startup": "local worktree creation and validated binding return; no harness or model started",
            "limitations": "small local repositories on one non-isolated workstation; no model usage, model throughput, implementation duration, concurrent scaling, network, or human latency claims",
        }, "trials": [],
    }
    with tempfile.TemporaryDirectory(prefix="mx-phase11-workspace-") as name:
        temporary = Path(name).resolve()
        for count in report["method"]["counts"]:
            print(f"measuring {count} repositories", flush=True)
            report["trials"].append(trial(count, temporary, binary, source, environment))
    report["finished_utc"] = datetime.now(timezone.utc).isoformat()
    report["machine"]["load_average_end"] = os.getloadavg()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    staging = args.output.with_suffix(".json.tmp")
    staging.write_text(json.dumps(report, indent=2) + "\n")
    staging.replace(args.output)
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
