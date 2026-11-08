#!/usr/bin/env python3
"""Verify the actual stored carriers and their concrete-to-protocol adapters, for arbitrary payload types."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=ROOT / 'crates/fairing-widgets/src/interaction/concrete.rs')
p.add_argument('--output', type=Path, default=ROOT / 'target/assurance/verus-concrete.rs')
a = p.parse_args()
rules_path = a.output.with_name(a.output.stem + '-rules.rs')
subprocess.run([sys.executable, ROOT / 'verification/verus/interaction.py', '--output', rules_path], check=True)
rules = rules_path.read_text()
original = a.source.read_text()
source = original.replace('pub(crate)', 'pub')
contracts = {
    'lease_group': 'lease_member(saved, pass, result), forall |g: LeaseGroup| lease_member(saved, pass, g) ==> g == result',
    'restore': 'result.is_some() == live(saved, pass), match result { Some(value) => match saved { Some(s) => *value == s.payload, None => false }, None => true }',
    'hold_snapshot': 'result.present, result.enabled == enabled, result.continuous == saved.is_some(), result.active == hold_active(saved)',
    'hold_operation': 'result == hold_request(pointer)',
    'hold_admitted': 'result == (enabled && pointer.held && (pointer.fresh || hold_active(saved)))',
    'hold_finish': 'result.completed == (admitted && full && previous != HoldPhase::Done), result.phase == hold_phase(previous, admitted, full)',
    'capture_snapshot': 'result.present, result.enabled == enabled, result.continuous, result.active == capture_active(saved)',
    'capture_operation': 'result == capture_request(pointer)',
    'capture_step': 'result.admitted == expected_admission(saved, enabled, pointer), result.captured == expected_capture(saved, enabled, pointer)',
    'apply_capture': 'result.payload == saved.payload, result.captured == next.captured',
    'classify_capture': 'concrete_member(saved, pass, result), forall |g: ConcreteGroup| concrete_member(saved, pass, g) ==> g == result',
    'restore_capture': 'result.is_some() == live(saved, pass), match result { Some(value) => match saved { Some(s) => *value == s.payload, None => false }, None => true }',
}
if original.count('fn ') != len(contracts):
    raise SystemExit('Review the concrete carrier proof boundary')
for name, contract in contracts.items():
    start = source.index('pub fn ' + name)
    end = source.index('{', start)
    signature = source[start:end]
    before, output = signature.rsplit('->', 1)
    source = source[:start] + source[start:].replace(signature, before + '-> (result: ' + output.strip() + ')\n ensures ' + contract + ',\n', 1)

def body(text, name, adapted=False):
    function = text.index('fn ' + name)
    start = text.index('\n{', function) + 1 if adapted else text.index('{', function)
    depth = 0
    for end in range(start, len(text)):
        depth += (text[end] == '{') - (text[end] == '}')
        if depth == 0:
            return text[start:end + 1]
    raise SystemExit('Unbalanced concrete source')

for name in contracts:
    if body(source, name, True) != body(original, name):
        raise SystemExit('Adapter modified executable concrete code: ' + name)

spec = '''
pub open spec fn live<T>(s:Option<&Session<T>>,p:Pass)->bool {
 p.enabled && match s { None=>false, Some(s)=>
   s.seen as int <= p.now as int && p.now as int - s.seen as int <= 1
   && !(p.lifetime == Lifetime::Press && p.fresh_press && s.seen != p.now) }
}
pub open spec fn lease_member<T>(s:Option<&Session<T>>,p:Pass,g:LeaseGroup)->bool {
 match g {
  LeaseGroup::Disabled => !p.enabled,
  LeaseGroup::Missing => p.enabled && s.is_none(),
  LeaseGroup::Future => p.enabled && match s {Some(s)=>s.seen > p.now,None=>false},
  LeaseGroup::Expired => p.enabled && match s {Some(s)=>s.seen as int <= p.now as int && p.now as int-s.seen as int>1,None=>false},
  LeaseGroup::ReplacedPress => p.enabled && match s {Some(s)=>p.now as int-s.seen as int==1 && p.lifetime==Lifetime::Press && p.fresh_press,None=>false},
  LeaseGroup::Live => live(s,p),
 }
}
pub open spec fn hold_active(s:Option<HoldPhase>)->bool {
 match s { Some(HoldPhase::Armed) | Some(HoldPhase::Done) => true, Some(HoldPhase::Idle) | None => false }
}
pub open spec fn hold_request(p:Pointer)->Operation {
 if p.held { if p.fresh {Operation::Start}else{Operation::Continue} }else{Operation::Cancel}
}
pub open spec fn hold_phase(previous:HoldPhase,admitted:bool,full:bool)->HoldPhase {
 if !admitted {HoldPhase::Idle} else if previous==HoldPhase::Done || full {HoldPhase::Done}else{HoldPhase::Armed}
}
pub open spec fn capture_active<T>(s:Option<&Capture<T>>)->bool {
 match s {Some(s)=>s.captured,None=>false}
}
pub open spec fn starting(p:Pointer)->bool { p.fresh && (p.held || p.clicked) }
pub open spec fn finishing(p:Pointer)->bool { !p.fresh && (p.stopped || p.clicked) }
pub open spec fn continuing(p:Pointer)->bool { !p.fresh && !finishing(p) && (p.held || p.dragged) }
pub open spec fn capture_request(p:Pointer)->Operation {
 if starting(p) {Operation::Start} else if finishing(p) {Operation::Finish}
 else if continuing(p) {Operation::Continue} else {Operation::Cancel}
}
pub open spec fn expected_admission<T>(s:Option<&Capture<T>>,e:bool,p:Pointer)->bool {
 e && (starting(p) || capture_active(s) && (finishing(p) || continuing(p)))
}
pub open spec fn expected_capture<T>(s:Option<&Capture<T>>,e:bool,p:Pointer)->bool {
 e && p.down && (starting(p) || capture_active(s) && continuing(p))
}
pub open spec fn concrete_member<T>(s:Option<&Session<Capture<T>>>,p:Pass,g:ConcreteGroup)->bool {
 match g {
  ConcreteGroup::Disabled=>lease_member(s,p,LeaseGroup::Disabled),
  ConcreteGroup::Missing=>lease_member(s,p,LeaseGroup::Missing),
  ConcreteGroup::Future=>lease_member(s,p,LeaseGroup::Future),
  ConcreteGroup::Expired=>lease_member(s,p,LeaseGroup::Expired),
  ConcreteGroup::ReplacedPress=>lease_member(s,p,LeaseGroup::ReplacedPress),
  ConcreteGroup::Idle=>live(s,p) && match s {Some(s)=>!s.payload.captured,None=>false},
  ConcreteGroup::Captured=>live(s,p) && match s {Some(s)=>s.payload.captured,None=>false},
 }
}
pub open spec fn chosen_group<T>(s:Option<&Session<Capture<T>>>,p:Pass)->ConcreteGroup {
 if !p.enabled {ConcreteGroup::Disabled} else { match s {
  None=>ConcreteGroup::Missing,
  Some(s)=>if s.seen>p.now {ConcreteGroup::Future}
    else if p.now as int-s.seen as int>1 {ConcreteGroup::Expired}
    else if p.lifetime==Lifetime::Press && p.fresh_press && s.seen!=p.now {ConcreteGroup::ReplacedPress}
    else if s.payload.captured {ConcreteGroup::Captured}else{ConcreteGroup::Idle}
 } }
}
pub proof fn concrete_partition<T>(s:Option<&Session<Capture<T>>>,p:Pass)
 ensures concrete_member(s,p,chosen_group(s,p)),
   forall |a:ConcreteGroup,b:ConcreteGroup| concrete_member(s,p,a) && concrete_member(s,p,b) ==> a==b
{}
// Arbitrarily many owners and all their joint combinations; no finite history or
// collection-length bound. This does not prove the egui map-to-sequence relation.
pub proof fn capture_collection_partition<T>(states:Seq<Option<&Session<Capture<T>>>>,p:Pass)
 ensures forall |i:int| #![trigger states[i]] 0<=i<states.len() ==> concrete_member(states[i],p,chosen_group(states[i],p)),
   forall |i:int,a:ConcreteGroup,b:ConcreteGroup| 0<=i<states.len()
     && concrete_member(states[i],p,a) && concrete_member(states[i],p,b) ==> a==b
{
 assert forall |i:int| #![trigger states[i]] 0<=i<states.len() implies concrete_member(states[i],p,chosen_group(states[i],p)) by {
   concrete_partition(states[i],p);
 }
 assert forall |i:int,a:ConcreteGroup,b:ConcreteGroup| 0<=i<states.len()
   && concrete_member(states[i],p,a) && concrete_member(states[i],p,b) implies a==b by {
   concrete_partition(states[i],p);
 }
}
\n// The proof is generic in the full payload, including arbitrary animation states.
pub proof fn payload_noninterference<A,B>(a:Option<&Capture<A>>,b:Option<&Capture<B>>,enabled:bool,p:Pointer)
 requires capture_active(a)==capture_active(b)
 ensures expected_admission(a,enabled,p)==expected_admission(b,enabled,p),
         expected_capture(a,enabled,p)==expected_capture(b,enabled,p)
{}
// This inductive step has no history bound: once Done, every admitted successor
// stays Done regardless of animation progress until cancellation retires the press.
pub proof fn completion_latch(previous:HoldPhase,admitted:bool,full:bool)
 requires previous==HoldPhase::Done, admitted
 ensures hold_phase(previous,admitted,full)==HoldPhase::Done,
         !(admitted && full && previous!=HoldPhase::Done)
{}
'''
lines = source.splitlines()
while lines and lines[0].startswith('//!'):
    lines.pop(0)
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text('pub mod rules {\n' + rules + '\n}\npub mod concrete {\nuse vstd::prelude::*;\nverus! {\n'
                    + '\n'.join(lines) + '\n' + spec + '\n}\n}\n')
subprocess.run([os.environ['VERUS'], '--crate-type=lib', str(a.output)], check=True)
