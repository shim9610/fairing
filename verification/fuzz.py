#!/usr/bin/env python3
"""Run finite, seeded ASan fuzz campaigns and retain the command and logs."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import install

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target/assurance/fuzz"
OUT.mkdir(parents=True, exist_ok=True)
binary = install.tool("cargo_fuzz")
environment = dict(os.environ, RUSTUP_TOOLCHAIN=install.PINS["nightly"])
seeds = json.loads((ROOT / "verification/seeds.json").read_text())
lock = ROOT / "verification/Cargo.lock"
lock_before = install.digest(lock)
results = []
for target, runs, max_length in [("bool_protocol", 20000, 128), ("config", 10000, 4096)]:
    corpus = OUT / "corpus" / target
    corpus.mkdir(parents=True, exist_ok=True)
    for index, seed in enumerate(seeds[target]):
        data = seed.encode() if isinstance(seed, str) else bytes(seed)
        (corpus / f"seed-{index}").write_bytes(data)
    command = [str(binary), "fuzz", "run", "--fuzz-dir", str(ROOT / "verification"),
               "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "target/assurance-fuzz"),
               "--sanitizer", "address", "--codegen-units", "16", "--features", "fuzz"]
    if target == "bool_protocol":
        command.append("--no-default-features")
    command += [target, str(corpus), "--", f"-runs={runs}", f"-max_len={max_length}", "-seed=20261101", "-rss_limit_mb=2048"]
    print(f"Fuzzing {target}: {runs} runs", flush=True)
    with (OUT / (target + ".log")).open("w") as output:
        completed = subprocess.run(command, cwd=ROOT, env=environment, stdout=output, stderr=subprocess.STDOUT)
    if completed.returncode:
        print((OUT / (target + ".log")).read_text()[-16000:], file=sys.stderr)
    results.append({"target": target, "runs": runs, "max_length": max_length, "command": command, "exit_code": completed.returncode})
    (OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
    completed.check_returncode()
    if install.digest(lock) != lock_before:
        raise SystemExit("Fuzzing changed the approved verification lockfile")
