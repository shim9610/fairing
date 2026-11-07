#!/usr/bin/env python3
"""Check the exact production Back classification and dispatch against independent predicates."""
import argparse
import os
from pathlib import Path
import subprocess
ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=ROOT / 'crates/fairing/src/shell/back_route.rs')
p.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-back.rs')
a = p.parse_args()
source = a.source.read_text()
if source.count('fn ') != 2:
    raise SystemExit('Review Back proof boundary')
source = source.replace('pub(crate)', 'pub')
contracts = {
 'classify': 'member(s, result), forall |g: BackGroup| member(s, g) ==> g == result',
 'dispatch': 'result == expected(group)',
}
for name, contract in contracts.items():
 start = source.index('pub fn ' + name + '(')
 end = source.index('{', start)
 signature = source[start:end]
 before, output = signature.split('->')
 replacement = before + '-> (result: ' + output.strip() + ')\n    ensures ' + contract + ',\n'
 source = source[:start] + source[start:].replace(signature, replacement, 1)
spec = '''
pub open spec fn member(s: BackState, g: BackGroup) -> bool {
 match g {
  BackGroup::Faulted => s.faulted,
  BackGroup::Locked => !s.faulted && s.prompt && s.lock,
  BackGroup::Unlock => !s.faulted && s.prompt && !s.lock,
  BackGroup::Info => !s.faulted && !s.prompt && s.info,
  BackGroup::Overlay => !s.faulted && !s.prompt && !s.info && s.overlay,
  BackGroup::Keyboard => !s.faulted && !s.prompt && !s.info && !s.overlay && s.keyboard,
  BackGroup::Overview => !s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && s.overview,
  BackGroup::Home => !s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && !s.overview && s.home,
  BackGroup::Workspace => !s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && !s.overview && !s.home,
 }
}
pub open spec fn expected(g: BackGroup) -> BackTarget {
 match g {
  BackGroup::Faulted | BackGroup::Locked | BackGroup::Home => BackTarget::Ignore,
  BackGroup::Unlock => BackTarget::CancelPrompt,
  BackGroup::Info => BackTarget::CloseInfo,
  BackGroup::Overlay => BackTarget::CloseOverlay,
  BackGroup::Keyboard => BackTarget::HideKeyboard,
  BackGroup::Overview => BackTarget::CloseOverview,
  BackGroup::Workspace => BackTarget::Workspace,
 }
}
'''
# Declarations may change visibility/signatures; executable bodies must stay identical.
def body(text, name):
 start = text.index('{', text.index('fn ' + name + '('))
 depth = 0
 for end in range(start, len(text)):
  depth += (text[end] == '{') - (text[end] == '}')
  if depth == 0: return text[start:end+1]
 raise SystemExit('Unbalanced Back source')
original = a.source.read_text()
for name in contracts:
 if body(source,name) != body(original,name): raise SystemExit('Back adapter modified executable code')
# Leading inner documentation must remain outside the verus block.
lines=source.splitlines()
while lines and lines[0].startswith('//!'): lines.pop(0)
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text('use vstd::prelude::*;\nverus! {\n' + '\n'.join(lines) + '\n' + spec + '\n}\n')
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(a.output)], check=True)
