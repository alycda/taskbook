#!/usr/bin/env python3
"""Time the tb CLI against generated stores around the supported size.

Usage:
    cargo build --release -p taskbook-client
    python3 scripts/bench-envelope.py target/release/tb [ACTIVE/ARCHIVED ...]

Prints a markdown table of median board-view and create times. Stores are
synthetic: 80% tasks, notes with 200-1500 character bodies, about 385 bytes
per item. Each create is timed against a fresh copy of the store.
"""
import json
import os
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

DEFAULT_SIZES = ["256/4096", "512/4096", "1024/4096", "2048/8192"]
RUNS = 7
WORDS = (
    "alpha beta gamma review deploy invoice call email draft fix bug plan "
    "meeting doc report budget tax rust ffi nix ledger sync server client "
    "release notes"
).split()
BOARDS = ["my board", "work", "home", "finance", "rust"]


def text(rng, lo, hi):
    target = rng.randint(lo, hi)
    out = []
    while len(" ".join(out)) < target:
        out.append(rng.choice(WORDS))
    return " ".join(out)


def item(rng, i, ts):
    is_task = rng.random() < 0.8
    entry = {
        "_id": i,
        "_date": "Wed Oct 21 2026",
        "_timestamp": ts,
        "_isTask": is_task,
        "description": text(rng, 60, 120),
        "isStarred": rng.random() < 0.05,
        "boards": [rng.choice(BOARDS)],
    }
    if is_task:
        entry.update(
            isComplete=rng.random() < 0.5,
            inProgress=False,
            priority=rng.choice([1, 1, 1, 2, 3]),
        )
    elif rng.random() < 0.7:
        entry["body"] = text(rng, 200, 1500)
    return entry


def write_store(path, count, start_ts, rng):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    data = {str(i): item(rng, i, start_ts + i * 1000) for i in range(1, count + 1)}
    with open(path, "w") as f:
        json.dump(data, f, indent=2)


def median_ms(cmd, env, prepare=None):
    times = []
    for _ in range(RUNS):
        if prepare:
            prepare()
        started = time.perf_counter()
        subprocess.run(
            cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True
        )
        times.append(time.perf_counter() - started)
    return statistics.median(times) * 1000


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    tb = os.path.abspath(sys.argv[1])
    sizes = sys.argv[2:] or DEFAULT_SIZES

    print("| Active / archived | Files | Board view | Create |")
    print("|---|---|---|---|")
    with tempfile.TemporaryDirectory(prefix="tb-bench-") as root:
        for spec in sizes:
            active, archived = (int(x) for x in spec.split("/"))
            rng = random.Random(42)
            golden = os.path.join(root, f"golden-{spec.replace('/', '-')}")
            write_store(f"{golden}/.taskbook/storage/storage.json", active, 1_700_000_000_000, rng)
            write_store(f"{golden}/.taskbook/archive/archive.json", archived, 1_600_000_000_000, rng)
            size = sum(
                os.path.getsize(f"{golden}/.taskbook/{p}")
                for p in ("storage/storage.json", "archive/archive.json")
            )

            work = os.path.join(root, "work")

            def fresh_copy():
                shutil.rmtree(work, ignore_errors=True)
                shutil.copytree(golden, work)

            env = dict(os.environ, TASKBOOK_DIR=work)
            fresh_copy()
            show = median_ms([tb, "--cli"], env)
            create = median_ms([tb, "--cli", "--task", "bench task"], env, fresh_copy)
            print(f"| {active} / {archived} | {size / 1e6:.1f} MB | {show:.0f} ms | {create:.0f} ms |")


if __name__ == "__main__":
    main()
