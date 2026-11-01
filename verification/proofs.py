#!/usr/bin/env python3
"""Verify the production kernel, then require both verifiers to reject a broken acknowledgement check."""
import json
import os
from pathlib import Path
import subprocess
import sys

import install

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target/assurance/proofs"
OUT.mkdir(parents=True, exist_ok=True)


def execute(name, command, env, expected_failure=False, marker=""):
    log = OUT / (name + ".log")
    with log.open("w") as output:
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    content = log.read_text()
    if expected_failure:
        if result.returncode == 0 or marker not in content:
            raise SystemExit(f"Negative control did not fail semantically: {log}")
    elif result.returncode:
        print(content[-12000:], file=sys.stderr)
        raise SystemExit(f"Proof failed: {log}")
    print(f"{name}: {'rejected contract violation' if expected_failure else 'verified'}", flush=True)
    return {"name": name, "exit_code": result.returncode, "expected_failure": expected_failure, "log": str(log)}


verus = install.tool("verus")
kani = install.tool("kani")
environment = dict(os.environ, VERUS=str(verus))
results = [execute("verus", [sys.executable, ROOT / "verification/verus/prove.py"], environment)]
sysroot = subprocess.check_output(["rustup", "run", install.PINS["kani_toolchain"], "rustc", "--print", "sysroot"], text=True).strip()
kani_env = dict(os.environ, RUSTUP_TOOLCHAIN=install.PINS["kani_toolchain"])
kani_env["PATH"] = str(kani.parent) + os.pathsep + kani_env.get("PATH", "")
kani_env["LD_LIBRARY_PATH"] = os.pathsep.join(path for path in [str(Path(sysroot) / "lib"), kani_env.get("LD_LIBRARY_PATH", "")] if path)
results.append(execute("kani", [kani, ROOT / "verification/kani.rs"], kani_env))
results.append(execute("verus-access", [sys.executable, ROOT / "verification/verus/access.py"], environment))
results.append(execute("kani-access", [kani, ROOT / "verification/kani_access.rs"], kani_env))

source = (ROOT / "crates/fairing/src/shell/bool_ack.rs").read_text()
needle = "if reported == Some(sent)"
if source.count(needle) != 1:
    raise SystemExit("Review the production acknowledgement-check mutation")
mutant = source.replace(needle, "if reported != Some(sent)", 1)
generated = (ROOT / "target/assurance/verus-bool-ack.rs").read_text()
if generated.count(needle) != 1:
    raise SystemExit("Review the Verus negative control")
verus_mutant = OUT / "verus-mutant.rs"
verus_mutant.write_text(generated.replace(needle, "if reported != Some(sent)", 1))
results.append(execute("verus-negative", [verus, "--crate-type=lib", verus_mutant], environment,
                       expected_failure=True, marker="postcondition not satisfied"))
kani_source = (ROOT / "verification/kani.rs").read_text()
declaration = '#[path = "../crates/fairing/src/shell/bool_ack.rs"]\nmod bool_ack;'
if kani_source.count(declaration) != 1:
    raise SystemExit("Review the Kani negative control")
kani_mutant = OUT / "kani-mutant.rs"
kani_mutant.write_text(kani_source.replace(declaration, "mod bool_ack {\n" + mutant + "\n}"))
results.append(execute("kani-negative", [kani, kani_mutant], kani_env,
                       expected_failure=True, marker="VERIFICATION:- FAILED"))
access_source = (ROOT / "crates/fairing/src/access/rules.rs").read_text()
access_harness = (ROOT / "verification/kani_access.rs").read_text()
declaration = '#[path = "../crates/fairing/src/access/rules.rs"]\nmod rules;'
if access_harness.count(declaration) != 1:
    raise SystemExit("Review the access mutation harness")
for name, old, new in [
    ("level-order", "held >= required", "held <= required"),
    ("unknown-grant", "known\n        &&", "true\n        &&"),
    ("counter-wrap", "current.saturating_add(1)", "current.wrapping_add(1)"),
    ("lockout-boundary", "failures >= limit", "failures > limit"),
]:
    if access_source.count(old) != 1:
        raise SystemExit(f"Review the access mutation: {name}")
    mutant = access_source.replace(old, new, 1)
    source_path = OUT / (name + "-source.rs")
    source_path.write_text(mutant)
    results.append(execute("verus-" + name, [sys.executable, ROOT / "verification/verus/access.py",
                           "--source", source_path, "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    harness_path = OUT / (name + "-kani.rs")
    harness_path.write_text(access_harness.replace(declaration, "mod rules {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name, [kani, harness_path], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))
(OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
