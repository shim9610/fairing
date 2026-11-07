#!/usr/bin/env python3
"""Prove the exact production viewport clipping arithmetic for every usize input."""
import argparse
import os
from pathlib import Path
import subprocess
ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=ROOT / 'crates/fairing-widgets/src/memory/display.rs')
p.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-display.rs')
a = p.parse_args()
s = a.source.read_text()
signature = 'pub(crate) const fn display_skip(retained: usize, limit: usize) -> usize '
if s.count(signature) != 1: raise SystemExit('Review production display-clipping adapter')
start = s.index('{', s.index(signature)); depth = 0
for end in range(start, len(s)):
    depth += (s[end] == '{') - (s[end] == '}')
    if depth == 0:
        body = s[start:end+1]
        break
else: raise SystemExit('Unbalanced display function')
header = '''use vstd::prelude::*;
verus! {
pub fn display_skip(retained: usize, limit: usize) -> (result: usize)
    ensures result <= retained,
        retained - result <= limit,
        result == if retained > limit { retained - limit } else { 0 },
'''
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(header + body + '\n}\n')
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(a.output)], check=True)
