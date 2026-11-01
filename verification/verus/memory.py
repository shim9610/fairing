#!/usr/bin/env python3
"""Prove the exact pool generation function cannot wrap into an old key."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', type=Path, default=ROOT / 'crates/fairing-widgets/src/memory.rs')
parser.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-memory.rs')
args = parser.parse_args()
source = args.source.read_text()
signature = 'pub(crate) const fn next_generation(current: u64) -> Option<u64> '
if source.count(signature) != 1:
    raise SystemExit('Review the generation proof adapter')
start = source.index('{', source.index(signature))
depth = 0
for end in range(start, len(source)):
    depth += (source[end] == '{') - (source[end] == '}')
    if depth == 0:
        body = source[start:end+1]
        break
else:
    raise SystemExit('Unbalanced generation function')
# Only declaration visibility/const qualifier and postconditions change. The entire
# production body is concatenated unchanged, without external-body assumptions.
header = '''use vstd::prelude::*;
verus! {
pub fn next_generation(current: u64) -> (result: Option<u64>)
    ensures match result {
        Some(next) => next > current && next as int == current as int + 1,
        None => current == u64::MAX,
    },
'''
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(header + body + '\n}\n')
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(args.output)], check=True)
