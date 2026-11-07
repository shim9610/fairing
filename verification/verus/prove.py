#!/usr/bin/env python3
"""Add a contract around the production function without replacing its body."""
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
source = (root / "crates/fairing/src/shell/bool_ack.rs").read_text()
signature = ") -> Decision {"
if source.count(signature) != 1 or source.count("fn ") != 1:
    raise SystemExit("The production kernel changed; review the contract generator")
contract = """
) -> (result: Decision)
    ensures
        match result {
            Decision::Send(next) => reported == Some(sent) && next == desired && next != sent,
            Decision::Keep(previous_failure) => reported != Some(sent) && (!failed || was_failed) && previous_failure == failed,
            Decision::Complete => (reported == Some(sent) && desired == sent)
                || (reported != Some(sent) && failed && !was_failed),
        },
{
"""
generated = root / "target/assurance/verus-bool-ack.rs"
generated.parent.mkdir(parents=True, exist_ok=True)
if source.count("pub(crate) ") != 2:
    raise SystemExit("Review visibility translation for the production kernel")
# Verus's generated enum accessors require public visibility in this standalone crate.
# This changes visibility only; the executable function body is preserved byte for byte.
contract = contract.rstrip("\n")
verified_source = source.replace("pub(crate) ", "pub ").replace(signature, contract, 1)
if verified_source.split(contract, 1)[1] != source.split(signature, 1)[1]:
    raise SystemExit("The proof adapter changed the production function body")
generated.write_text("use vstd::prelude::*;\nverus! {\n" + verified_source + "\n}\n")
subprocess.run([os.environ["VERUS"], "--crate-type=lib", str(generated)], check=True)
