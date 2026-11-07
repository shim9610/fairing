#!/usr/bin/env python3
"""Refine the opaque factory carrier using unchanged production method bodies."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import install
from central_proofs import generate as kernel_source, SUPPORT
from verus.mediation_path import function

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'target/assurance/central-mediation/carriers'
MODEL='''#![allow(dead_code)]
use std::{cell::{Cell,RefCell},rc::Rc};
#[path="@DISPATCH@"] mod protected_dispatch;
type Stamp=(u64,u64);
#[derive(Clone)] struct Gate { key:u8 }
#[derive(Clone,Copy)] struct State { token:bool, grant:bool, grant_key:u8, stamp:Option<Stamp> }
struct Cx<'a> { state:&'a Cell<State>, path:Option<&'a AuthorityPath<'a>>, resource_entries:&'a Cell<u8> }
impl<'a> Cx<'a> {
 fn token_mode(&self)->bool {self.state.get().token}
 fn requires_mediation(&self)->bool {true}
 fn authority_stamp(&self)->Option<Stamp> {self.state.get().stamp}
 fn authority_path(&self)->Option<&'a AuthorityPath<'a>> {self.path}
 fn allows_gate(&self,gate:&Gate)->bool {self.state.get().grant && self.state.get().grant_key==gate.key}
 fn with_protected_resources<R>(&mut self,path:&AuthorityPath<'_>,op:impl FnOnce(&mut Cx<'_>)->R)->R {
  self.resource_entries.set(self.resource_entries.get()+1);
  let mut scoped=Cx{state:self.state,path:Some(path),resource_entries:self.resource_entries}; op(&mut scoped)
 }
}
struct AuthorityPath<'a>{gate:&'a Gate,stamp:Stamp,parent:Option<&'a AuthorityPath<'a>>}
impl AuthorityPath<'_> {
 fn allows(&self,cx:&Cx<'_>)->bool {
  // Separately verified unbounded path contract. The parent here is the
  // actual inherited outer factory admission, not a cached allow bit.
  cx.token_mode() && cx.authority_stamp()==Some(self.stamp) && cx.allows_gate(self.gate)
    && self.parent.is_none_or(|p| p.stamp==self.stamp && cx.allows_gate(p.gate))
 }
}
trait Screen {}
struct Dummy; impl Screen for Dummy {}
type ScreenFactory=dyn FnMut()->Box<dyn Screen>;
struct FactoryCallback { body:Rc<RefCell<Box<ScreenFactory>>>, requirement:Requirement }
struct ScreenHandle { body:Option<Box<dyn Screen>>,factory:Option<Rc<RefCell<Box<ScreenFactory>>>>,requirement:Requirement }
'''
CLIENT='''
#[kani::proof]
#[kani::unwind(4)]
fn factory_and_screen_share_live_requirement() {
 let before=State{token:kani::any(),grant:kani::any(),grant_key:kani::any(),stamp:Some(kani::any())};
 let after=State{token:kani::any(),grant:kani::any(),grant_key:kani::any(),stamp:if kani::any(){Some(kani::any())}else{None}};
 let state=Rc::new(Cell::new(before));let resources=Cell::new(0);let calls=Rc::new(Cell::new(0u8));
 let factory_state=state.clone();let factory_calls=calls.clone();
 let required_key:u8=kani::any();
 let has_body:bool=kani::any();let has_factory:bool=kani::any();let legacy:bool=kani::any();
 let factory=FactoryCallback{body:Rc::new(RefCell::new(Box::new(move || {
  factory_calls.set(factory_calls.get()+1); factory_state.set(after); Box::new(Dummy) as Box<dyn Screen>
 }))),requirement:if legacy{Requirement::Legacy}else{Requirement::Required(Gate{key:required_key})}};
 let mut handle=factory.instance();
 assert_eq!(match (&factory.requirement,&handle.requirement){
  (Requirement::Required(a),Requirement::Required(b))=>a.key==b.key,
  (Requirement::Legacy,Requirement::Legacy)=>true,_=>false},true,"factory requirement was substituted");
 if has_body{handle.body=Some(Box::new(Dummy));}
 if !has_factory{handle.factory=None;}
 let outer=!legacy && before.token && before.grant && before.grant_key==required_key;
 let factory_expected=outer && !has_body && has_factory;
 let entered=Cell::new(0u8);let old:u64=kani::any();let secret:u64=kani::any();let effect=Cell::new(old);
 let mut cx=Cx{state:&state,path:None,resource_entries:&resources};
 let result=handle.invoke(&mut cx,|_,_|{entered.set(entered.get()+1);effect.set(secret);secret});
 let expected=outer && (has_body || has_factory) && (has_body || (after.token && after.grant && after.grant_key==required_key && after.stamp==before.stamp));
 assert_eq!(calls.get(),u8::from(factory_expected),"factory executed without its requirement");
 assert_eq!(entered.get(),u8::from(expected),"screen entered after revocation or changed authority");
 assert_eq!(effect.get(),if expected{secret}else{old},"denied factory result produced an effect");
 assert_eq!(result,if expected{Some(secret)}else{None},"denied factory result escaped");
}
'''

def generate(source):
    # Reuse the exact common-kernel extraction, without its independent client.
    from central_proofs import CLIENT as KCLIENT
    import tempfile
    with tempfile.NamedTemporaryFile(mode='w+',suffix='.rs') as f:
        kernel_source(source,Path(f.name));kernel=Path(f.name).read_text()
    assert kernel.startswith(SUPPORT.replace('@DISPATCH@',str(ROOT/'crates/fairing/src/access/protected_dispatch.rs')))
    kernel=kernel[len(SUPPORT.replace('@DISPATCH@',str(ROOT/'crates/fairing/src/access/protected_dispatch.rs'))):]
    assert kernel.endswith(KCLIENT);kernel=kernel[:-len(KCLIENT)]
    req_start=source.index('enum Requirement {');req_end=source.index('\n}',req_start)+2
    requirement='#[derive(Clone)]\n'+source[req_start:req_end]+"\nimpl Requirement {\n"+function(source[source.index('impl Requirement {'):],'fn admission(')+'\n}\n'
    instance=function(source[source.index('impl FactoryCallback {'):],'pub(crate) fn instance(')
    screen_invoke=function(source[source.index('impl ScreenHandle {'):],'fn invoke<R>(')
    return MODEL.replace('@DISPATCH@',str(ROOT/'crates/fairing/src/access/protected_dispatch.rs'))+kernel+requirement+'\nimpl FactoryCallback {\n'+instance+'\n}\nimpl ScreenHandle {\n'+screen_invoke+'\n}\n'+CLIENT

def main():
    OUT.mkdir(parents=True,exist_ok=True);receipt=OUT/'receipt.json';receipt.unlink(missing_ok=True)
    paths=[ROOT/p for p in ['crates/fairing/src/access/mediation.rs','crates/fairing/src/access/protected_dispatch.rs','verification/carrier_proofs.py','verification/central_proofs.py','verification/tools.json']]
    def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest()for p in paths}
    before=hashes();source=paths[0].read_text();kani=install.tool('kani');channel=install.PINS['kani_toolchain']
    sysroot=Path(subprocess.check_output(['rustup','run',channel,'rustc','--print','sysroot'],text=True).strip())
    env=dict(os.environ,RUSTUP_TOOLCHAIN=channel);env['PATH']=str(kani.parent)+os.pathsep+env.get('PATH','');env['LD_LIBRARY_PATH']=str(sysroot/'lib')+os.pathsep+env.get('LD_LIBRARY_PATH','')
    cases=[('production',source,None),
      ('factory-rebinding',source.replace('requirement: self.requirement.clone(),','requirement: Requirement::Legacy,'),'factory requirement was substituted'),
      ('skip-postfactory-admission',source.replace('invoke(requirement.admission(), cx, |cx| operation(&mut **body, cx))','Some(operation(&mut **body, cx))'),'screen entered after revocation or changed authority')]
    controls=[]
    for name,text,assertion in cases:
        file=OUT/(name+'.rs');file.write_text(generate(text))
        run=subprocess.run([str(kani),str(file)],cwd=ROOT,env=env,capture_output=True,text=True);log=run.stdout+run.stderr;file.with_suffix('.log').write_text(log)
        if assertion:
            failures=re.findall(r'Status: FAILURE\n\s*- Description: ([^\n]*)',log)
            assert run.returncode and any(v.strip('"')==assertion for v in failures),(name,log[-5000:])
            controls.append({'case':name,'status':'security-assertion-rejected'})
        else:
            assert run.returncode==0 and '1 successfully verified harnesses, 0 failures, 1 total' in log,log[-8000:]
    assert before==hashes(),'Carrier proof sources changed'
    receipt.write_text(json.dumps({'status':'production-factory-carrier-verified','inputs':before,'controls':controls,
      'contract':'Factory and resulting screen retain the same requirement. Both require admission; arbitrary issuer/token/revision changes inside a factory block the subsequent hook and result. Initial screen/factory presence is arbitrary, covering reuse without a history bound.',
      'assumptions':'Common kernel and unbounded ancestor-walk contract, standard Rc/RefCell/Box ownership, borrowed original authority state; UI effects inside an admitted operation remain trusted. This is carrier refinement, not an assertion that all global routes are closed.'},indent=2)+'\n')
    print('production-factory-carrier-verified')
if __name__=='__main__':main()
