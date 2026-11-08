#!/usr/bin/env python3
"""Verify the exact typed callback admission predicate; Kani verifies generic invocation."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=ROOT / 'crates/fairing/src/access/protected_dispatch.rs')
p.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-protected.rs')
a = p.parse_args()
original = a.source.read_text()
start = original.index('pub(crate) fn admitted(')
end = original.index('\n}', start) + 2
source = original[start:end]
signature = 'pub(crate) fn admitted(token_mode: bool, allowed: bool) -> bool'
if source.count(signature) != 1:
    raise SystemExit('Review protected admission signature')
source = source.replace(signature, 'pub fn admitted(token_mode: bool, allowed: bool) -> (result: bool)\n ensures result == (token_mode && allowed),', 1)
if source[source.index('{'):] != original[start:end][original[start:end].index('{'):]:
    raise SystemExit('Protected admission adapter changed the production body')
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text('use vstd::prelude::*;\nverus! {\n' + source + '\n}\n')
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(a.output)], check=True)
