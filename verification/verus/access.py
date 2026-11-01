#!/usr/bin/env python3
"""Prove contracts on the exact decision bodies called by the public access APIs."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, default=ROOT / "crates/fairing/src/access/rules.rs")
parser.add_argument("--output", type=Path, default=ROOT / "target/assurance/verus-access.rs")
args = parser.parse_args()
source = args.source.read_text()
contracts = {
    "level_allows": "result == (held >= required)",
    "grant_allowed": "result == (known && (candidate > held || (lock && candidate == held)))",
    "next_failure": "result >= current, result as int == if current < u32::MAX { current as int + 1 } else { current as int }",
    "lockout_reached": "result == match limit { Some(n) => failures >= n, None => false }",
}
if source.count("pub(crate) fn ") != len(contracts) or source.count("fn ") != len(contracts):
    raise SystemExit("Access decision functions changed; review the proof adapter")
header, generated = source.split("\n", 1)
if not header.startswith("//!"):
    raise SystemExit("Review the access module documentation boundary")
for name, contract in contracts.items():
    start = generated.index("pub(crate) fn " + name + "(")
    brace = generated.index("{", start)
    signature = generated[start:brace]
    if signature.count("->") != 1:
        raise SystemExit("Review the access function signature")
    before, return_type = signature.split("->")
    replacement = before.replace("pub(crate)", "pub") + "-> (result: " + return_type.strip() + ")\n    ensures " + contract + ",\n"
    generated = generated[:start] + generated[start:].replace(signature, replacement, 1)
    # Compare this function's complete body through its balanced closing brace.
    original_start = source.index("{", source.index("pub(crate) fn " + name + "("))
    changed_start = generated.index("{", generated.index("pub fn " + name + "(") + len(replacement))
    def body(text, at):
        depth = 0
        for end in range(at, len(text)):
            depth += (text[end] == "{") - (text[end] == "}")
            if depth == 0:
                return text[at:end + 1]
        raise SystemExit("Unbalanced production function")
    if body(source, original_start) != body(generated, changed_start):
        raise SystemExit("Access adapter changed a production function body")
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(header + "\nuse vstd::prelude::*;\nverus! {\n" + generated + "\n}\n")
subprocess.run([os.environ["VERUS"], "--crate-type=lib", str(args.output)], check=True)
