#!/usr/bin/env python3
"""Check the exact common admission kernel and the unbounded ancestor walk."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

import install
from validation import KERNEL_INPUTS, require
sys.path.insert(0, str(Path(__file__).resolve().parent / 'verus'))
from mediation_path import function

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/central-mediation'
SUPPORT = '''#![allow(dead_code)]
use std::cell::Cell;
#[path = "@DISPATCH@"] mod protected_dispatch;
type Stamp = (u64,u64);
#[derive(Clone)] struct Gate { key: u64 }
struct Cx<'a> {
    token: bool, strict: bool, grant: bool, stamp: Option<Stamp>, parent_allowed: bool,
    path: Option<&'a AuthorityPath<'a>>, resource_entries: &'a Cell<u8>,
}
impl<'a> Cx<'a> {
    fn token_mode(&self) -> bool { self.token }
    fn requires_mediation(&self) -> bool { self.strict }
    fn allows_gate(&self, _: &Gate) -> bool { self.grant }
    fn authority_stamp(&self) -> Option<Stamp> { self.stamp }
    fn authority_path(&self) -> Option<&'a AuthorityPath<'a>> { self.path }
    fn with_protected_resources<R>(&mut self, path: &AuthorityPath<'_>, operation: impl FnOnce(&mut Cx<'_>) -> R) -> R {
        self.resource_entries.set(self.resource_entries.get()+1);
        let mut scoped = Cx { token: self.token, strict: self.strict, grant: self.grant,
            stamp: self.stamp, parent_allowed: self.parent_allowed, path: Some(path), resource_entries: self.resource_entries };
        operation(&mut scoped)
    }
}
struct AuthorityPath<'a> { gate: &'a Gate, stamp: Stamp, parent: Option<&'a AuthorityPath<'a>> }
impl AuthorityPath<'_> {
    // Summary of the separately verified arbitrary ancestor walk under a stable
    // observation contract. The inherited prefix is not enumerated here.
    fn allows(&self, cx: &Cx<'_>) -> bool {
        cx.token && cx.stamp == Some(self.stamp) && cx.grant && cx.parent_allowed
    }
}
'''
CARRIER_CLIENT = r"""
#[derive(Clone)] enum Requirement { Legacy, Required(Gate) }
@REQUIREMENT@
trait Permission {}
impl Permission for u8 {}
struct PermissionBinding<P:Permission> { gate:Gate, marker:std::marker::PhantomData<P> }
impl<P:Permission> PermissionBinding<P> { fn into_gate(self)->Gate { self.gate } }
struct Callback<T: ?Sized> { body: Box<T>, requirement: Requirement }
impl<T: ?Sized> Callback<T> { @SELECTOR@ @BINDING@ }
#[kani::proof]
fn every_payload_selector_uses_its_own_requirement() {
 let resources=Cell::new(0u8);let stamp:Stamp=kani::any();let key:u64=kani::any();let original_key:u64=kani::any();
 let legacy:bool=kani::any();let value:u64=kani::any();
 let mut carrier=Callback{body:Box::new(value),requirement:if legacy{Requirement::Legacy}else{Requirement::Required(Gate{key:original_key})}};
 let bind:bool=kani::any();
 if bind { carrier=carrier.required(PermissionBinding{gate:Gate{key},marker:std::marker::PhantomData::<u8>}); }
 let legacy=legacy && !bind;
 let key=if bind{key}else{original_key};
 let mut cx=Cx{token:kani::any(),strict:kani::any(),grant:kani::any(),stamp:if kani::any(){Some(stamp)}else{None},parent_allowed:kani::any(),path:None,resource_entries:&resources};
 let allowed=if legacy{!cx.strict}else{cx.token && cx.grant && cx.stamp.is_some() && cx.parent_allowed};
 let before:u64=kani::any();let mut effect=before;let mut entered=false;
 let result=carrier.invoke(&mut cx,|body,scoped|{
  entered=true;effect=*body;
  assert!(legacy || scoped.authority_path().is_some_and(|path|path.gate.key==key),"payload requirement identity changed");
  *body
 });
 assert_eq!(entered,allowed,"payload selector bypassed its requirement");
 assert_eq!(result,if allowed{Some(value)}else{None},"payload selector exposed a denied result");
 assert_eq!(effect,if allowed{value}else{before},"payload selector exposed a denied effect");
}
"""

CLIENT = '''
#[kani::proof]
fn common_admission_is_necessary_for_every_operation() {
    let class: u8 = kani::any(); kani::assume(class < 3);
    let current: Stamp = kani::any(); let expected_stamp: Stamp = kani::any();
    let has_session: bool = kani::any(); let has_parent: bool = kani::any();
    let parent_gate = Gate { key: 1 }; let gate = Gate { key: 2 };
    let parent = AuthorityPath::new(&parent_gate, current, None);
    let resources = Cell::new(0u8);
    let mut cx = Cx { token: kani::any(), strict: kani::any(), grant: kani::any(),
        stamp: if has_session { Some(current) } else { None },
        parent_allowed: !has_parent || kani::any(),
        path: if has_parent { Some(&parent) } else { None }, resource_entries: &resources };
    let expected = if class == 0 { !cx.strict }
        else { cx.token && has_session && cx.grant && cx.parent_allowed && (class == 1 || expected_stamp == current) };
    let before: u64 = kani::any(); let secret: u64 = kani::any();
    let mut effect = before; let mut entries = 0u8; let mut inherited = false;
    let operation = |scoped: &mut Cx<'_>| {
        entries += 1; effect = secret;
        inherited = scoped.authority_path().is_some_and(|path| path.gate.key == gate.key
            && path.stamp == current && path.parent.is_some() == has_parent
            && path.parent.is_none_or(|parent| parent.gate.key == parent_gate.key));
        secret
    };
    let result = match class {
        0 => invoke(Admission::Legacy, &mut cx, operation),
        1 => invoke(Admission::Gate(&gate), &mut cx, operation),
        _ => invoke_requirement(&gate, expected_stamp, &mut cx, operation),
    };
    assert_eq!(entries, u8::from(expected), "common admission was bypassed or duplicated");
    assert_eq!(result, if expected { Some(secret) } else { None }, "denied dispatch exposed a result");
    assert_eq!(effect, if expected { secret } else { before }, "denied dispatch produced an effect");
    assert_eq!(resources.get(), u8::from(expected && class != 0), "protected resources were bypassed");
    assert!(!expected || class == 0 || inherited, "ancestor context was not inherited");
}
'''


def generate(text, path):
    enum_start = text.index("enum Admission<'a>")
    enum_end = text.index('\n}', enum_start) + 2
    admission = function(text[text.index("impl Admission<'_>"):], 'fn allows(&self, cx: &Cx')
    invoke = function(text, 'fn invoke<R>(')
    source = SUPPORT.replace('@DISPATCH@', str(ROOT / 'crates/fairing/src/access/protected_dispatch.rs'))
    constructor = function(text, "fn new(gate: &'a Gate")
    source += "impl<'a> AuthorityPath<'a> {\n" + constructor + '\n}\n'
    source += text[enum_start:enum_end] + "\nimpl Admission<'_> {\n" + admission + '\n}\n' + invoke + '\n'
    source += function(text, 'fn invoke_bound<R>(') + '\n' + function(text, 'pub(crate) fn invoke_requirement<R>(') + '\n' + CLIENT + CARRIER_CLIENT.replace('@REQUIREMENT@', 'impl Requirement { '+function(text[text.index('impl Requirement {'):], 'fn admission(')+' }').replace('@SELECTOR@',function(text[text.index('impl<T: ?Sized> Callback<T>'):], 'fn invoke<R>(')).replace('@BINDING@',function((ROOT/'crates/fairing/src/access/mediation/contract.rs').read_text(),'pub(crate) fn required<P:'))
    path.write_text(source)
    return path


def execute(path, env, kani):
    result = subprocess.run([str(kani), str(path)], cwd=ROOT, env=env, capture_output=True, text=True)
    path.with_suffix('.log').write_text(result.stdout + result.stderr)
    return result


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = OUT / 'kernel-proof.json'; receipt.unlink(missing_ok=True)
    paths = [ROOT / p for p in KERNEL_INPUTS]
    def hashes(): return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    before = hashes()
    subprocess.run([sys.executable, ROOT / 'verification/verus/mediation_path.py'], cwd=ROOT, check=True)
    channel = install.PINS['kani_toolchain']
    sysroot = Path(subprocess.check_output(['rustup', 'run', channel, 'rustc', '--print', 'sysroot'], text=True).strip())
    kani = install.tool('kani')
    env = dict(os.environ, RUSTUP_TOOLCHAIN=channel)
    env['PATH'] = str(kani.parent) + os.pathsep + env.get('PATH', '')
    env['LD_LIBRARY_PATH'] = os.pathsep.join([str(sysroot / 'lib'), env.get('LD_LIBRARY_PATH', '')])
    source = paths[0].read_text()
    result = execute(generate(source, OUT / 'kernel.rs'), env, kani)
    if result.returncode or '2 successfully verified harnesses, 0 failures, 2 total' not in result.stdout + result.stderr:
        raise SystemExit((result.stdout + result.stderr)[-12000:])
    controls = []
    for label, old, new, assertion in [
        ('bypass-admission', 'let allowed = admission.allows(cx);', 'let allowed = true;', 'common admission was bypassed or duplicated'),
        ('legacy-in-protected-mode', 'Self::Legacy => !cx.requires_mediation(),', 'Self::Legacy => true,', 'common admission was bypassed or duplicated'),
        ('skip-resource-isolation', 'cx.with_protected_resources(path, operation)', 'operation(cx)', 'protected resources were bypassed'),
        ('discard-parent-at-entry', 'AuthorityPath::new(gate, stamp, cx.authority_path())', 'AuthorityPath::new(gate, stamp, None)', 'ancestor context was not inherited'),
    ]:
        require(source.count(old) == 1, 'Assurance condition failed: source.count(old) == 1')
        result = execute(generate(source.replace(old, new, 1), OUT / (label + '-kani.rs')), env, kani)
        content = result.stdout + result.stderr
        # The security assertion itself must be FAILURE, not merely mentioned in
        # a proof summary after an unrelated failure/undetermined result.
        failed = re.findall(r'Status: FAILURE\n\s*- Description: ([^\n]*)', content)
        require(result.returncode and any(text.strip('"') == assertion for text in failed), 'Mutation did not fail its security assertion: ' + label)
        controls.append({'case': label, 'status': 'security-assertion-rejected'})
    require(before == hashes(), 'Proof inputs changed during checking')
    receipt.write_text(json.dumps({'status': 'shared-production-admission-kernel-verified', 'inputs': before, 'controls': controls,
        'assumptions': 'Cx methods satisfy their observation and invocation-resource contracts; AuthorityPath is abstracted by its separately verified boolean decision. Kernel source bodies and branch predicates are unchanged. Concrete Cx resource storage, atomic trace refinement and whole-library routing are separate obligations.',
        'role': 'kernel-contract'}, indent=2) + '\n')
    print('shared-production-admission-kernel-verified (semantic mutation controls rejected)')


if __name__ == '__main__': main()
