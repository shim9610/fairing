#!/usr/bin/env python3
"""Verify the exact retained-interaction protocol against independent group and operation specifications."""
import argparse, os, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--source',type=Path,default=ROOT/'crates/fairing-widgets/src/interaction/rules.rs')
p.add_argument('--output',type=Path,default=ROOT/'target/assurance/verus-interaction.rs')
a=p.parse_args(); original=a.source.read_text(); source=original.replace('pub(crate)','pub')
contracts={
 'adjacent':'result == recent(seen, now)',
 'live':'result == (recent(seen, now) && !(press_scoped && fresh_press && seen != now))',
 'classify':'member(s, result), forall |g: Group| member(s, g) ==> g == result',
 'permitted':'result == promised(s, operation)',
 'step':'result.active == next_active(s, operation), result.effect == next_effect(s, operation)',
}
if original.count('fn ')!=len(contracts): raise SystemExit('Review interaction proof boundary')
for name,contract in contracts.items():
 start=source.index('pub fn '+name+'('); end=source.index('{',start); sig=source[start:end]
 before,output=sig.split('->'); replacement=before+'-> (result: '+output.strip()+')\n ensures '+contract+',\n'
 source=source[:start]+source[start:].replace(sig,replacement,1)
def body(s,name):
 start=s.index('{',s.index('fn '+name+'(')); depth=0
 for end in range(start,len(s)):
  depth+=(s[end]=='{')-(s[end]=='}')
  if depth==0:return s[start:end+1]
 raise SystemExit('Unbalanced protocol source')
for name in contracts:
 if body(original,name)!=body(source,name):raise SystemExit('Adapter modified protocol executable body')
spec='''
pub open spec fn recent(seen:u64, now:u64) -> bool { now as int >= seen as int && now as int - seen as int <= 1 }
pub open spec fn member(s:Snapshot,g:Group)->bool {
 match g {
  Group::Absent => !s.present,
  Group::Disabled => s.present && !s.enabled,
  Group::Expired => s.present && s.enabled && !s.continuous,
  Group::Idle => s.present && s.enabled && s.continuous && !s.active,
  Group::Active => s.present && s.enabled && s.continuous && s.active,
 }
}
pub open spec fn promised(s:Snapshot,o:Operation)->bool {
 match o {
  Operation::Observe | Operation::Cancel => true,
  Operation::Start => s.present && s.enabled,
  Operation::Continue | Operation::Finish => member(s,Group::Active),
 }
}
pub open spec fn next_active(s:Snapshot,o:Operation)->bool {
 match o {
  Operation::Observe => member(s,Group::Active),
  Operation::Start | Operation::Continue => promised(s,o),
  Operation::Finish | Operation::Cancel => false,
 }
}
pub open spec fn next_effect(s:Snapshot,o:Operation)->Effect {
 match o {
  Operation::Observe => if s.active && !member(s,Group::Active) { Effect::Retired } else { Effect::Unchanged },
  Operation::Start => if s.present && s.enabled {Effect::Began}else{Effect::Refused},
  Operation::Continue => if member(s,Group::Active) {Effect::Continued}else{Effect::Refused},
  Operation::Finish => if member(s,Group::Active) {Effect::Completed}else{Effect::Refused},
  Operation::Cancel => Effect::Retired,
 }
}
'''
lines=source.splitlines()
while lines and lines[0].startswith('//!'):lines.pop(0)
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text('use vstd::prelude::*;\nverus! {\n'+'\n'.join(lines)+'\n'+spec+'\n}\n')
subprocess.run([os.environ['VERUS'],'--crate-type=lib',str(a.output)],check=True)
