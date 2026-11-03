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
results.append(execute("verus-memory", [sys.executable, ROOT / "verification/verus/memory.py"], environment))
results.append(execute("kani-memory", [kani, ROOT / "verification/kani_memory.rs"], kani_env))
results.append(execute("verus-display", [sys.executable, ROOT / "verification/verus/display.py"], environment))
results.append(execute("verus-input", [sys.executable, ROOT / "verification/verus/input.py"], environment))
results.append(execute("kani-input", [kani, ROOT / "verification/kani_input.rs"], kani_env))

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
    ("stale-response", "active == Some(response)", "active.is_some()"),
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
memory_source = (ROOT / "crates/fairing-widgets/src/memory.rs").read_text()
needle = "current.checked_add(1)"
if memory_source.count(needle) != 1:
    raise SystemExit("Review the generation negative control")
mutant = memory_source.replace(needle, "Some(current.wrapping_add(1))", 1)
# Generated inline modules keep their production child-module source path.
display_source = ROOT / "crates/fairing-widgets/src/memory/display.rs"
mutant = mutant.replace('#[path = "memory/display.rs"]\nmod display;', '#[path = ' + json.dumps(str(display_source)) + ']\nmod display;')
source_path = OUT / "memory-wrap-source.rs"
source_path.write_text(mutant)
results.append(execute("verus-memory-wrap", [sys.executable, ROOT / "verification/verus/memory.py",
                       "--source", source_path, "--output", OUT / "memory-wrap-verus.rs"], environment,
                       expected_failure=True, marker="postcondition not satisfied"))
harness = (ROOT / "verification/kani_memory.rs").read_text()
declaration = '#[path = "../crates/fairing-widgets/src/memory.rs"]\nmod memory;'
if harness.count(declaration) != 1:
    raise SystemExit("Review the generation Kani mutation adapter")
harness_path = OUT / "memory-wrap-kani.rs"
harness_path.write_text(harness.replace(declaration, "mod memory {\n" + mutant + "\n}"))
results.append(execute("kani-memory-wrap", [kani, harness_path], kani_env,
                       expected_failure=True, marker="VERIFICATION:- FAILED"))
# A viewport must select the newest bounded suffix, not the older full window.
display_text = display_source.read_text()
needle = "retained - limit"
if display_text.count(needle) != 1: raise SystemExit("Review display-clipping mutation")
display_mutant = OUT / "display-clip-source.rs"
display_mutant.write_text(display_text.replace(needle, "0", 1))
results.append(execute("verus-display-clip", [sys.executable, ROOT / "verification/verus/display.py",
                       "--source", display_mutant, "--output", OUT / "display-clip-verus.rs"], environment,
                       expected_failure=True, marker="postcondition not satisfied"))
inline = memory_source.replace('#[path = "memory/display.rs"]\nmod display;',
                               '#[path = ' + json.dumps(str(display_mutant)) + ']\nmod display;')
clip_harness = OUT / "display-clip-kani.rs"
clip_harness.write_text(harness.replace(declaration, "mod memory {\n" + inline + "\n}"))
results.append(execute("kani-display-clip", [kani, clip_harness], kani_env,
                       expected_failure=True, marker="VERIFICATION:- FAILED"))
# Reject over-deferral and modal capture in both source-bound input verifiers.
input_source = (ROOT / "crates/fairing/src/shell/input_route.rs").read_text()
input_harness = (ROOT / "verification/kani_input.rs").read_text()
declaration = '#[path = "../crates/fairing/src/shell/input_route.rs"]\nmod input_route;'
if input_harness.count(declaration) != 1:
    raise SystemExit("Review input Kani source binding")
for name, old, new in [
    ("input-route", "geometry_changed && fresh_primary_press && region_claimed", "geometry_changed || fresh_primary_press || region_claimed"),
    ("modal-capture", "overlay_closed && !prompt_open && !overview_open", "overlay_closed && !overview_open"),
]:
    if input_source.count(old) != 1:
        raise SystemExit("Review input mutation: " + name)
    mutant = input_source.replace(old, new, 1)
    input_mutant = OUT / (name + "-source.rs")
    input_mutant.write_text(mutant)
    results.append(execute("verus-" + name + "-negative", [sys.executable, ROOT / "verification/verus/input.py",
                           "--source", input_mutant, "--output", OUT / (name + "-verus.rs")], environment,
                           expected_failure=True, marker="postcondition not satisfied"))
    input_control = OUT / (name + "-kani.rs")
    input_control.write_text(input_harness.replace(declaration, "mod input_route {\n" + mutant + "\n}"))
    results.append(execute("kani-" + name + "-negative", [kani, input_control], kani_env,
                           expected_failure=True, marker="VERIFICATION:- FAILED"))
(OUT / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
