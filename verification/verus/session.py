#!/usr/bin/env python3
"""Verify exact session decisions and an unbounded terminal-revocation induction lemma."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=ROOT / 'crates/fairing/src/access/session_rules.rs')
p.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-session.rs')
a = p.parse_args()
original = a.source.read_text()
contracts = {
 'next_identity': 'match result { Some(next) => next as int == current as int + 1 && next > current, None => current == u64::MAX }',
 'next_revision': 'match result { Some(next) => current % 2 == 0 && next % 2 == 0 && next as int == current as int + 2 && next > current, None => current % 2 != 0 || current >= u64::MAX - 1 }',
 'observe': '''match result {
 Observation::Active(revision) => !revoked && now < until && before == after && before % 2 == 0 && revision == before,
 Observation::Revoked => revoked,
 Observation::Expired => !revoked && now >= until,
 Observation::Busy => !revoked && now < until && (before != after || before % 2 != 0),
 }''',
 'permission_allows': 'result == (live && (base_until > now || temporary_until > now))',
 'authorize': 'result == (!quarantined && issued && policy)',
 'transition': '''result.revision >= state.revision,
 state.revoked ==> result.revoked && result.revision == state.revision,
 match event {
 ControlEvent::Revoke => result.revoked && result.revision == state.revision,
 ControlEvent::Publish { now, until } => {
 result.revoked == (state.revoked || now >= until || state.revision % 2 != 0 || state.revision >= u64::MAX - 1)
 && (!result.revoked ==> result.revision % 2 == 0 && result.revision as int == state.revision as int + 2)
 && (result.revoked ==> result.revision == state.revision)
 },
 }''',
}
if original.count('pub(crate) fn ') != len(contracts) or original.count('fn ') != len(contracts):
 raise SystemExit('Session decision inventory changed; review independent contracts')
source = original.replace('pub(crate)', 'pub')
for name, contract in contracts.items():
 start = source.index('pub fn ' + name + '(')
 brace = source.index('{', start)
 signature = source[start:brace]
 before, output = signature.split('->')
 source = source[:start] + source[start:].replace(signature, before + '-> (result: ' + output.strip() + ')\n ensures ' + contract + ',\n', 1)
def body(text, name):
 start = text.index('{', text.index('fn ' + name + '(')); depth = 0
 for end in range(start, len(text)):
  depth += (text[end] == '{') - (text[end] == '}')
  if depth == 0:return text[start:end+1]
 raise SystemExit('Unbalanced session body')
for name in contracts:
 # Contracts can contain braces; select body after the generated ensures terminator.
 marker = ' ensures ' + contracts[name] + ',\n'
 at = source.index(marker, source.index('fn ' + name + '(')) + len(marker)
 depth = 0; begin = source.index('{',at)
 for end in range(begin,len(source)):
  depth += (source[end]=='{')-(source[end]=='}')
  if depth==0:break
 if source[begin:end+1] != body(original,name):raise SystemExit('Session adapter changed production body: '+name)
lines = source.splitlines()
while lines and lines[0].startswith('//!'):lines.pop(0)
lemma = '''
pub open spec fn terminal_step(a: ControlState, b: ControlState) -> bool {
 a.revoked ==> b.revoked && b.revision == a.revision
}
pub proof fn terminal_history(states: Seq<ControlState>, n: int)
 requires 0 <= n < states.len(), states[0].revoked,
          forall|i: int| 0 <= i < states.len()-1 ==> #[trigger] terminal_step(states[i], states[i+1]),
 ensures states[n].revoked, states[n].revision == states[0].revision,
 decreases n,
{
 if n > 0 {
  terminal_history(states, n-1);
  let i = n - 1;
  assert(0 <= i < states.len()-1);
  assert(terminal_step(states[i], states[i+1]));
  assert(states[i+1] == states[n]);
 }
}
'''
# Under the SC atomic contract, a nonwrapping revision is monotone. Permission
# stores occur only at odd revisions. This lemma applies to arbitrary trace lengths,
# indices and permission-cell counts; the concrete Rust SC trace refinement is
# guarded structurally and exercised separately with Loom, not assumed proved here.
lemma += r'''
pub proof fn stable_even_interval(epochs: Seq<u64>, lo: int, hi: int)
 requires 0 <= lo <= hi < epochs.len(), epochs[lo] == epochs[hi], epochs[lo] % 2 == 0,
          forall|i: int, j: int| 0 <= i <= j < epochs.len() ==> #[trigger] epochs[i] <= #[trigger] epochs[j],
 ensures forall|k: int| lo <= k <= hi ==> #[trigger] epochs[k] == epochs[lo] && epochs[k] % 2 == 0,
{
 assert forall|k: int| lo <= k <= hi implies #[trigger] epochs[k] == epochs[lo] && epochs[k] % 2 == 0 by {
  assert(epochs[lo] <= epochs[k]);
  assert(epochs[k] <= epochs[hi]);
 }
}
'''
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text('use vstd::prelude::*;\nverus! {\n'+'\n'.join(lines)+'\n'+lemma+'\n}\n')
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(a.output)],check=True)
