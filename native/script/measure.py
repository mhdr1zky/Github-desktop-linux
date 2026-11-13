#!/usr/bin/env python3
"""Measure a release native window on Linux without touching its preferences."""

import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time


def process_values(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    ticks = int(stat[11]) + int(stat[12])  # utime + stime, all process threads
    memory = {}
    for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
        if line.startswith(("Rss:", "Pss:")):
            key, value, _unit = line.split()
            memory[key.rstrip(":").lower() + "_kib"] = int(value)
    return ticks, memory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("repository", type=Path)
    parser.add_argument("--settle", type=int, default=30)
    parser.add_argument("--sample", type=int, default=60)
    args = parser.parse_args()
    if args.settle < 1 or args.sample < 1:
        parser.error("settle and sample must be positive seconds")
    with tempfile.TemporaryFile(mode="w+") as log:
        child = subprocess.Popen(
            [str(args.binary.resolve()), "--no-persist", str(args.repository.resolve())],
            stdout=log, stderr=log,
        )
        try:
            for _ in range(args.settle):
                if child.poll() is not None:
                    raise RuntimeError(f"Native window exited with code {child.returncode}")
                time.sleep(1)
            start_ticks, _ = process_values(child.pid)
            start = time.monotonic()
            samples = []
            for _ in range(args.sample):
                time.sleep(1)
                if child.poll() is not None:
                    raise RuntimeError(f"Native window exited with code {child.returncode}")
                end_ticks, memory = process_values(child.pid)
                samples.append(memory)
            elapsed = time.monotonic() - start
            result = {
                "platform": platform.platform(),
                "machine": platform.machine(),
                "session": "wayland" if os.environ.get("WAYLAND_DISPLAY") else "x11",
                "binary_bytes": args.binary.stat().st_size,
                "settle_seconds": args.settle,
                "sample_seconds": elapsed,
                "idle_cpu_percent_one_core": (end_ticks - start_ticks) / os.sysconf("SC_CLK_TCK") / elapsed * 100,
                "rss_kib_max_sampled": max(s["rss_kib"] for s in samples),
                "pss_kib_max_sampled": max(s["pss_kib"] for s in samples),
                "note": "Main process incl. worker threads; excludes GPU driver allocations outside this process. No Electron comparison.",
            }
            log.flush()
            log.seek(0)
            result["startup_log"] = log.read()[-8000:]
            print(json.dumps(result, indent=2))
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


if __name__ == "__main__":
    main()
