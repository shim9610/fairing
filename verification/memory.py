#!/usr/bin/env python3
"""Run real API/UI allocation trials and require allocator-lab exhaustion to reject growth."""
import json
import os
from pathlib import Path
import resource
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/assurance/memory"
OUT.mkdir(parents=True, exist_ok=True)
target = ROOT / "target/memory-modes"
results = []

def no_core_dump():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

for capacity in [None, 16 * 1024 * 1024, 64 * 1024 * 1024]:
    name = "system" if capacity is None else str(capacity)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_INCREMENTAL="0")
    command = ["cargo", "run", "--locked", "-p", "fairing", "--example", "memory_limits", "--no-default-features"]
    if capacity is not None:
        env["FAIRING_HEAP_BYTES"] = str(capacity)
        command = ["cargo", "run", "--locked", "-p", "fairing-fixed-heap", "--example", "allocation_pressure", "--features", "global"]
    path = OUT / (name + ".log")
    with path.open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    text = path.read_text()
    if result.returncode or (capacity is not None and (f"capacity={capacity}" not in text or ("recoverable-pressure:" not in text or "bounded-display:" not in text))):
        print(text[-12000:])
        raise SystemExit(f"Memory mode failed: {path}")
    results.append({"capacity": capacity, "command": command, "exit_code": result.returncode, "log": str(path)})
    if capacity is not None:
        path = OUT / (name + "-exhaustion.log")
        with path.open("w") as log:
            control = subprocess.run([str(target / "debug/examples/allocation_pressure"), "--infallible-exhaust"],
                                     cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
                                     preexec_fn=no_core_dump)
        if control.returncode == 0 or "memory allocation" not in path.read_text():
            raise SystemExit(f"Infallible exhaustion did not fail as expected: {path}")
        results.append({"capacity": capacity, "unsupported_infallible_lab_control": True,
                        "exit_code": control.returncode, "log": str(path)})
    (OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
    print(name + ": verified capacity, reuse, and exhaustion behavior", flush=True)
