#!/usr/bin/env python3
"""Verify exact production input admission, modal priority and quiet-pass progress."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=ROOT / 'crates/fairing/src/shell/input_route.rs')
p.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-input.rs')
a = p.parse_args()
s = a.source.read_text()
if s.count('fn ') != 2:
    raise SystemExit('Review input proof scope')

def body(name):
    signature = 'pub(crate) fn ' + name + '('
    if s.count(signature) != 1:
        raise SystemExit('Review production function: ' + name)
    start = s.index('{', s.index(signature))
    depth = 0
    for end in range(start, len(s)):
        depth += (s[end] == '{') - (s[end] == '}')
        if depth == 0:
            return s[start:end+1]
    raise SystemExit('Unbalanced production function')

header = '''use vstd::prelude::*;
verus! {
pub fn defer_ordinary_ui(geometry_changed: bool, fresh_primary_press: bool, region_claimed: bool)
    -> (result: bool)
    ensures
        result ==> geometry_changed && fresh_primary_press && region_claimed,
        geometry_changed && fresh_primary_press && region_claimed ==> result,
        !fresh_primary_press ==> !result,
'''
modal = '''
pub fn regions_can_capture(overlay_closed: bool, prompt_open: bool, overview_open: bool)
    -> (result: bool)
    ensures result == (overlay_closed && !prompt_open && !overview_open),
'''
progress = '''
pub fn quiet_pass_after_any_history(changed: bool, fresh: bool, claimed: bool,
                                  next_changed: bool, next_claimed: bool) -> (deferred: bool)
    ensures !deferred,
{
    let _previous = defer_ordinary_ui(changed, fresh, claimed);
    defer_ordinary_ui(next_changed, false, next_claimed)
}
}
'''
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(header + body('defer_ordinary_ui') + '\n' + modal + body('regions_can_capture') + '\n' + progress)
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(a.output)], check=True)
