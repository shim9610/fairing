#!/usr/bin/env python3
"""Measure which production code the API contract suite actually executes."""
import json
import os
from pathlib import Path
import subprocess
import tomllib

import install

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/assurance/coverage"
OUT.mkdir(parents=True, exist_ok=True)
channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
environment = dict(os.environ, RUSTUP_TOOLCHAIN=channel, CARGO_INCREMENTAL="0",
                   CARGO_TARGET_DIR=str(ROOT / "target/assurance-coverage"),
                   PROPTEST_CASES="512", PROPTEST_RNG_SEED="20261101")
binary = install.tool("cargo_llvm_cov")
command = [str(binary), "llvm-cov", "--locked", "--manifest-path", str(ROOT / "verification/Cargo.toml"),
           "--lib", "--target", "x86_64-unknown-linux-gnu",
           "--dep-coverage", "fairing,fairing-widgets", "--no-default-ignore-filename-regex",
           "--json", "--summary-only", "--output-path", str(OUT / "raw.json")]
with (OUT / "tests.log").open("w") as log:
    result = subprocess.run(command, cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT)
if result.returncode:
    print((OUT / "tests.log").read_text()[-12000:])
result.check_returncode()
export = json.loads((OUT / "raw.json").read_text())["data"][0]
product = {str(Path(item["filename"]).resolve().relative_to(ROOT)): item["summary"] for item in export["files"]
           if str(Path(item["filename"]).resolve()).startswith(str(ROOT / "crates") + os.sep)}
required = ["fairing/src/access/auth.rs", "fairing/src/access/mod.rs", "fairing/src/access/rules.rs",
            "fairing/src/shell/mod.rs", "fairing/src/shell/input_route.rs", "fairing/src/shell/back_route.rs", "fairing/src/services/mod.rs", "fairing/src/notify/center.rs",
            "fairing/src/time.rs", "fairing/src/inbox.rs", "fairing/src/inbox/bounded.rs",
            "fairing-widgets/src/memory.rs", "fairing-widgets/src/memory/display.rs", "fairing-widgets/src/unit.rs"]
for path in required:
    summary = product.get("crates/" + path)
    if summary is None or summary["lines"]["covered"] == 0:
        raise SystemExit(f"Contract tests did not execute the claimed production module: {path}")
totals = {}
for metric in ["lines", "functions", "regions"]:
    count = sum(summary[metric]["count"] for summary in product.values())
    covered = sum(summary[metric]["covered"] for summary in product.values())
    totals[metric] = {"count": count, "covered": covered, "percent": covered * 100 / count if count else 0}
summary = {"scope": "Product code executed by public API contracts, distinct from the full product unit-test coverage",
           "toolchain": channel, "tool": install.PINS["cargo_llvm_cov"], "command": command,
           "product_lock_sha256": install.digest(ROOT / "Cargo.lock"),
           "verification_lock_sha256": install.digest(ROOT / "verification/Cargo.lock"),
           "totals": totals, "files": product}
(OUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(totals, indent=2))
