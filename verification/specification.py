#!/usr/bin/env python3
"""Verify the expanded UI model and audit the existing product-connected carrier projection.

A single reachable collision with different required UI observations refutes
completeness. Absence of this collision is never accepted as a universal proof.
The default command is a strict check: an established gap exits with status 2.
"""
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import sys

import install
import ui_specification

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/specification'
TEST = 'widgets::text_field::specification::reachable_focus_states_emit_current_specification_witnesses'
WITNESS_SOURCE = ROOT / 'crates/fairing-widgets/src/widgets/text_field/specification/tests.rs'


def execute(name, command, env, *, rejected=False, marker=''):
    path = OUT / (name + '.log')
    with path.open('w') as log:
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env,
                                stdout=log, stderr=subprocess.STDOUT)
    output = path.read_text()
    if rejected:
        valid = result.returncode != 0 and marker in output
        if name.startswith('kani-'):
            failures = re.findall(r'Check \d+: ([^\n]+)\n\s+- Status: FAILURE', output)
            valid = valid and bool(failures) and all(
                check.startswith('current_specification_is_complete.assertion.') for check in failures)
            if 'UNDETERMINED' in output:
                valid = False
    else:
        valid = result.returncode == 0
    if not valid:
        raise RuntimeError('Specification checker did not produce its required verdict: ' + str(path)
                           + '\n' + output[-8000:])
    return {'name': name, 'exit_code': result.returncode, 'expected_rejection': rejected,
            'log': str(path.relative_to(ROOT))}


def fingerprints():
    files = list((ROOT / 'crates/fairing-widgets/src').rglob('*.rs'))
    files += [ROOT / 'Cargo.lock', ROOT / 'Cargo.toml', ROOT / 'verification/tools.json',
              ROOT / 'verification/state-schema.json', ROOT / 'verification/verus/concrete.py', Path(__file__)]
    files += [ROOT / 'verification/ui_specification.py', ROOT / 'verification/verus/ui_state.rs']
    return {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(files)}


def verus_source(states):
    def state(index):
        s = states[index]
        values = ','.join(str(v) + 'u64' for v in s['view'])
        return ('pub open spec fn state_' + str(index) + '()->State { State { view:seq![' + values
                + '], focus:' + str(s['focused']) + 'u64, receiver:' + str(s['receiver']) + 'u8 } }\n')
    return '''use vstd::prelude::*;
verus! {
pub struct State { pub view:Seq<u64>, pub focus:u64, pub receiver:u8 }
pub open spec fn same_spec(a:State,b:State)->bool { a.view==b.view }
pub open spec fn same_observation(a:State,b:State)->bool { a.focus==b.focus && a.receiver==b.receiver }
''' + state(0) + state(1) + '''
// These two reachable records form a subset of the product's concrete states.
// Completeness over all product states necessarily implies it over this subset.
pub open spec fn executed(s:State)->bool { s==state_0() || s==state_1() }
pub open spec fn complete_on_executed_states()->bool {
 forall |a:State,b:State| executed(a) && executed(b) && same_spec(a,b) ==> same_observation(a,b)
}
pub proof fn executed_collision_is_a_counterexample()
 ensures executed(state_0()),executed(state_1()),same_spec(state_0(),state_1()),
         !same_observation(state_0(),state_1())
{}
// Quantify over every possible interpretation of the current abstract view.
// An interpreter cannot recover two different observations from the same input.
pub proof fn no_interpretation_can_recover_the_missing_state(
 a:State,b:State,meaning:spec_fn(Seq<u64>)->(u64,u8))
 requires same_spec(a,b),!same_observation(a,b)
 ensures meaning(a.view)!=(a.focus,a.receiver) || meaning(b.view)!=(b.focus,b.receiver)
{}
// Positive control: adding the missing focus observation separates these records.
// This is a control over this obligation, not proof of the whole refined product.
pub proof fn focus_aware_control()
 ensures forall |a:State,b:State| executed(a) && executed(b)
  && a.view==b.view && a.focus==b.focus ==> same_observation(a,b)
{}
// STRICT_OBLIGATION: this is the requested claim's necessary condition.
pub proof fn current_specification_is_complete()
 ensures complete_on_executed_states()
{
 let a=state_0(); let b=state_1();
 assert(executed(a) && executed(b) && same_spec(a,b));
 assert(same_observation(a,b));
}
}
'''


def kani_source(states):
    def data(index):
        s = states[index]
        return ('const VIEW_' + str(index) + ': [u64;' + str(len(s['view'])) + '] = ['
                + ','.join(str(v) + 'u64' for v in s['view']) + '];\n'
                + 'const FOCUS_' + str(index) + ':u64=' + str(s['focused']) + 'u64;\n'
                + 'const RECEIVER_' + str(index) + ':u8=' + str(s['receiver']) + 'u8;\n')
    return data(0) + data(1) + '''
fn same_view()->bool {
    let mut equal=true;
    for i in 0..VIEW_0.len() { equal = equal && VIEW_0[i]==VIEW_1[i]; }
    equal
}
#[kani::proof]
#[kani::unwind(40)]
fn counterexample_and_all_possible_predictions() {
    assert!(same_view());
    assert_ne!(FOCUS_0,FOCUS_1);
    assert_ne!(RECEIVER_0,RECEIVER_1);
    let predicted_focus:u64=kani::any();
    let predicted_receiver:u8=kani::any();
    assert!(!(predicted_focus==FOCUS_0 && predicted_receiver==RECEIVER_0
           && predicted_focus==FOCUS_1 && predicted_receiver==RECEIVER_1));
}
#[kani::proof]
fn focus_aware_control() {
    let a:u8=kani::any(); let b:u8=kani::any();
    kani::assume(a<2 && b<2);
    let fa=if a==0 {FOCUS_0}else{FOCUS_1};
    let fb=if b==0 {FOCUS_0}else{FOCUS_1};
    let ra=if a==0 {RECEIVER_0}else{RECEIVER_1};
    let rb=if b==0 {RECEIVER_0}else{RECEIVER_1};
    if fa==fb {assert_eq!(ra,rb);}
}
#[kani::proof]
#[kani::unwind(40)]
fn current_specification_is_complete() {
    if same_view() {assert_eq!((FOCUS_0,RECEIVER_0),(FOCUS_1,RECEIVER_1));}
}
'''


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'result.json').unlink(missing_ok=True)
    before = fingerprints()
    expanded = ui_specification.verify()
    witness = OUT / 'reachable-states.json'
    witness.unlink(missing_ok=True)
    env = dict(os.environ, CARGO_INCREMENTAL='0', FAIRING_SPECIFICATION_WITNESS=str(witness))
    commands = [execute('native-witness', ['cargo', 'test', '--locked', '-p', 'fairing-widgets',
                    '--lib', TEST, '--', '--exact'], env)]
    data = json.loads(witness.read_text())
    if data.get('schema') != 1 or data.get('scene') != 'two-editors' or data.get('same_context_collision') is not True or len(data.get('states', [])) != 2:
        raise RuntimeError('Unsupported or missing concrete witness')
    states = data['states']
    for state in states:
        if not isinstance(state['view'], list) or len(state['view']) != 31:
            raise RuntimeError('Current view layout changed: review the completeness obligation')
        if any(type(v) is not int or not 0 <= v < 2**64 for v in state['view']):
            raise RuntimeError('Invalid encoded state value')
        if state['focused'] not in [state['view'][17], state['view'][24]]:
            raise RuntimeError('Focus did not refer to either concrete editor')
        expected = 1 if state['focused'] == state['view'][17] else 2
        if state['receiver'] != expected:
            raise RuntimeError('Recorded input effect did not agree with concrete focus')
    if states[0]['view'] != states[1]['view']:
        raise RuntimeError('The known witness no longer collides; no universal completeness conclusion follows')
    if states[0]['focused'] == states[1]['focused'] or states[0]['receiver'] == states[1]['receiver']:
        raise RuntimeError('Concrete counterexample lost its distinguishing observations')

    verus = install.tool('verus')
    source = OUT / 'obligation-verus.rs'
    source.write_text(verus_source(states))
    strict = execute('verus-completeness', [verus, '--crate-type=lib', source], env,
                     rejected=True, marker='assertion failed')
    commands.append(strict)
    # Validate the refutation independently of the intentionally failing claim.
    positive = OUT / 'refutation-verus.rs'
    text = source.read_text()
    text = text[:text.index('// STRICT_OBLIGATION:')] + '}\n'
    positive.write_text(text)
    commands.append(execute('verus-refutation-and-control', [verus, '--crate-type=lib', positive], env))

    kani = install.tool('kani')
    toolchain = install.PINS['kani_toolchain']
    sysroot = subprocess.check_output(['rustup', 'run', toolchain, 'rustc', '--print', 'sysroot'], text=True).strip()
    kani_env = dict(env, RUSTUP_TOOLCHAIN=toolchain)
    kani_env['PATH'] = str(kani.parent) + os.pathsep + kani_env['PATH']
    kani_env['LD_LIBRARY_PATH'] = str(Path(sysroot) / 'lib') + os.pathsep + kani_env.get('LD_LIBRARY_PATH', '')
    source = OUT / 'obligation-kani.rs'
    source.write_text(kani_source(states))
    commands.append(execute('kani-completeness', [kani, source, '--harness', 'current_specification_is_complete'],
                            kani_env, rejected=True, marker='VERIFICATION:- FAILED'))
    commands.append(execute('kani-refutation', [kani, source, '--harness', 'counterexample_and_all_possible_predictions'], kani_env))
    commands.append(execute('kani-focus-aware-control', [kani, source, '--harness', 'focus_aware_control'], kani_env))
    if before != fingerprints():
        raise RuntimeError('Product/specification inputs changed while checking completeness')
    report = {'specification_complete': False, 'verdict': 'incomplete-product-refinement',
              'carrier_projection_verdict': 'refuted',
              'expanded_model': {'model_status': expanded['model_status'],
                  'theorems': expanded['theorems'], 'product_refinement': expanded['product_refinement'],
                  'evidence': 'target/assurance/ui-specification/receipt.json'},
              'obligation': 'Equal specified states must preserve required UI observations',
              'counterexample': {'scene': data['scene'], 'same_current_view': True, 'same_context_collision': data['same_context_collision'],
                 'focused_editor': [s['focused'] for s in states],
                 'text_receiver': [s['receiver'] for s in states],
                 'missing_state_dimension': 'focused editor identity'},
              'scope': 'The product-connected carrier-only view is refuted. The expanded joint model has proved partitions, but its total product projection remains open.',
              'trusted_boundary': 'Native execution establishes witness reachability and the product-connected view; solvers validate the resulting specification obligation',
              'inputs': before,
              'evidence_sha256': {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in [witness, OUT / 'obligation-verus.rs', OUT / 'refutation-verus.rs', OUT / 'obligation-kani.rs']},
              'commands': commands}
    (OUT / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print('STATE_SPECIFICATION_INCOMPLETE: joint model verified; total concrete projection and input realization remain open')
    print('Verus and Kani refuted the carrier-only projection; result: ' + str(OUT / 'result.json'))
    return 2


if __name__ == '__main__':
    sys.exit(main())
