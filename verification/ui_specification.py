#!/usr/bin/env python3
"""Verify the joint UI model and reject semantic mutations; does not certify product refinement."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

import install

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'verification/verus/ui_state.rs'
OUT = ROOT / 'target/assurance/ui-specification'
THEOREMS = [
    'joint_partition', 'unbounded_collection_partition',
    'stale_presentation_cannot_withdraw_promises', 'text_has_one_recipient',
    'group_determines_promises', 'focus_identity_is_observable',
    'blocked_promised_input_is_a_violation', 'fault_quarantine_and_cleanup',
    'recovery_contract_survives_fault', 'authentication_contract_survives_lock',
    'cancellation_has_a_successor', 'release_preserves_a_new_owner',
]

# Each control violates a different independently stated obligation.
CONTROLS = {
    'lost-focus-owner': ('s.focus==Some(a.target) && a.invocation==Invocation::Keyboard',
                         'a.invocation==Invocation::Keyboard'),
    'hidden-input-withdraws-promise': ('enabled(s,id) && !quarantined(s,id) && !locked(s,id) && in_scope(s,id)',
                                     'enabled(s,id) && !quarantined(s,id) && !locked(s,id) && in_scope(s,id) && has_hit(s,id)'),
    'stale-popup-is-closed': ('else if s.popup_order.contains(id) {PopupGroup::StaleSurface}',
                            'else if s.popup_order.contains(id) {PopupGroup::Closed}'),
    'wrong-capture-owner': ('else if owner==id {CaptureGroup::This}',
                            'else if owner==id {CaptureGroup::Other}'),
    'animation-phase-erased': ('Phase::Entering=>MotionGroup::Entering',
                               'Phase::Entering=>MotionGroup::Shown'),
    'fault-priority-lost': ('if s.faulted {ShellGroup::Faulted}',
                           'if s.faulted {ShellGroup::Workspace}'),
    'recovery-blocked': ('s.shell.faulted && !recovery_widget(s,id)', 's.shell.faulted'),
    'authentication-blocked': ('s.widgets[id].role!=Role::Authentication', 'true'),
    'cancel-retains-focus': ('focus:if s.focus==Some(id) {None} else {s.focus}', 'focus:s.focus'),
    'cancel-steals-peer-capture': ('s.capture.dom().filter(|p:u64| s.capture[p]!=id)', 'Set::empty()'),
}


def inputs():
    paths = [SOURCE, Path(__file__), ROOT / 'verification/tools.json']
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def verify():
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = OUT / 'receipt.json'
    receipt.unlink(missing_ok=True)
    before = inputs()
    source = SOURCE.read_text()
    code = re.sub(r'//[^\n]*', '', source)
    if re.search(r'\b(assume|admit|axiom|assume_specification)\b|verifier::external', code):
        raise RuntimeError('Joint specification contains a trusted proof escape')
    names = re.findall(r'pub proof fn (\w+)', source)
    if names != THEOREMS:
        raise RuntimeError('Review the joint model theorem inventory')
    tool = install.tool('verus')
    results = []
    for name, path in [('joint-model', SOURCE)] + [(name, OUT / (name + '.rs')) for name in CONTROLS]:
        negative = name in CONTROLS
        if negative:
            old, new = CONTROLS[name]
            if source.count(old) != 1:
                raise RuntimeError('Review semantic control: ' + name)
            path.write_text(source.replace(old, new))
        log = OUT / (name + '.log')
        with log.open('w') as stream:
            result = subprocess.run([str(tool), '--crate-type=lib', str(path)], cwd=ROOT,
                                    env=os.environ.copy(), stdout=stream, stderr=subprocess.STDOUT)
        text = log.read_text()
        valid = (result.returncode != 0 and ('error: postcondition not satisfied' in text or 'error: assertion failed' in text)
                 and re.search(r'verification results:: \d+ verified, [1-9]\d* errors', text)
                 and 'resource limit' not in text and 'timed out' not in text) if negative else (
                     result.returncode == 0 and 'verification results:: 12 verified, 0 errors' in text)
        if not valid:
            raise RuntimeError('Joint model check failed: ' + name + '\n' + text[-8000:])
        results.append({'name': name, 'exit_code': result.returncode, 'negative_control': negative,
                        'log': str(log.relative_to(ROOT))})
    if before != inputs():
        raise RuntimeError('Joint model changed during verification')
    report = {'model_status': 'proved', 'product_refinement': 'open',
              'scope': 'All values of the declared joint model, arbitrary collection sizes and payloads; not all product states',
              'theorems': names, 'inputs': before, 'commands': results}
    receipt.write_text(json.dumps(report, indent=2) + '\n')
    print('Joint UI specification: 12 theorems verified; 10 semantic controls rejected; product refinement open', flush=True)
    return report


if __name__ == '__main__':
    verify()
