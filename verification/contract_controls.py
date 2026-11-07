#!/usr/bin/env python3
"""Compile extension/bypass controls against the unchanged contract checker."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import central_mediation as c
import mediation as m

ROOT=m.ROOT
OUT=ROOT/'target/assurance/protected-contracts/controls'
EXTENSION='''
pub(crate) trait ContractExtension { fn execute(&mut self, cx: &mut Cx<'_>); }
protected_contract!(operation Callback<dyn ContractExtension>, extension(), |body,cx|body.execute(cx));
'''
CASES=[
 ('one-line-extension','crates/fairing/src/access/mediation.rs',None,EXTENSION,None),
 ('extension-bypass','crates/fairing/src/access/mediation.rs',None,EXTENSION+'''\nimpl Callback<dyn ContractExtension> { pub(crate) fn bypass(&mut self,cx:&mut Cx<'_>) {self.body.execute(cx);} }\n''','unguarded-payload-entry'),
 ('generic-one-line-extension','crates/fairing/src/access/mediation.rs',None,EXTENSION.split('protected_contract!')[0]+"protected_contract!(operation [T: ContractExtension] Callback<T>, extension_generic(), |body,cx|body.execute(cx));\n",None),
 ('generic-extension-bypass','crates/fairing/src/access/mediation.rs',None,EXTENSION.split('protected_contract!')[0]+"protected_contract!(operation [T: ContractExtension] Callback<T>, extension_generic(), |body,cx|body.execute(cx));\nimpl<T:ContractExtension> Callback<T>{pub(crate)fn bypass_generic(&mut self,cx:&mut Cx<'_>){self.body.execute(cx);}}\n",'unguarded-payload-entry'),
 ('forged-admission-cut','crates/fairing/src/access/mediation.rs',None,EXTENSION+"impl Callback<dyn ContractExtension>{pub(crate)fn forged_cut(&mut self,cx:&mut Cx<'_>){protected_dispatch::invoke(true,||self.body.execute(cx));}}\n",'unproved-admission-cut'),
 ('duplicate-entry','crates/fairing/src/access/mediation.rs',None,EXTENSION+"protected_contract!(operation Callback<dyn ContractExtension>, double_entry(), |body,cx|{body.execute(cx);body.execute(cx)});\n",'multiple-protected-entries'),
 ('looped-entry','crates/fairing/src/access/mediation.rs',None,EXTENSION+"protected_contract!(operation Callback<dyn ContractExtension>, looped_entry(), |body,cx|{for _ in 0..2 {body.execute(cx);}});\n",'kernel-adapter-cycle'),
 ('raw-protocol-call','crates/fairing/src/screen/mod.rs',None,"\npub(crate) fn bypass(screen:&mut dyn Screen,ui:&mut egui::Ui,cx:&mut Cx<'_>){screen.ui(ui,cx);}\n",'raw-protocol-reference-outside-kernel'),
 ('raw-protocol-storage','crates/fairing/src/screen/mod.rs',None,'\nstruct Bypass { body:Box<dyn Screen> }\n','raw-protocol-storage'),
 ('raw-extraction','crates/fairing/src/access/mediation.rs',None,'\nimpl ScreenHandle {pub(crate) fn bypass(self)->Option<Box<dyn Screen>>{self.body}}\n','raw-payload-return'),
 ('generic-helper-export','crates/fairing/src/access/mediation.rs',None,'\nimpl<T:?Sized> Callback<T>{pub(crate) fn bypass(self,slot:&mut Option<Box<T>>){sink(self.body,slot);}}\nfn sink<T:?Sized>(body:Box<T>,slot:&mut Option<Box<T>>){*slot=Some(body);}\n','executable-payload-write-through-borrow'),
 ('unregistered-storage','crates/fairing/src/access/mediation.rs',None,'\nstruct Bypass<T>{payload:T}\n','unclassified-executable-storage'),
 ('forged-contract','crates/fairing/src/access/mediation.rs',None,'\nstruct Bypass;impl contract::Carrier for Bypass{type Payload=dyn Screen;}\n','contract-not-generated'),
 ('exposed-payload','crates/fairing/src/access/mediation.rs','    body: Option<Box<dyn Screen>>,','    pub(crate) body: Option<Box<dyn Screen>>,','payload-field-visibility'),
 ('macro-bypass','crates/fairing/src/access/mediation/contract.rs','self.invoke(cx, |body,cx| body.$name($($arg,)*cx));','if let Some(body)=&mut self.body {body.$name($($arg,)*cx);}','unguarded-payload-entry'),
 ('discard-ancestors','crates/fairing/src/screen/cx.rs','authority_path: Some(path),','authority_path: None,','scope-context-contract'),
 ('replace-global-icons','crates/fairing/src/screen/cx.rs','icons: &mut icons,','icons: &mut *self.icons,','scoped-resource-aliases-global'),
 ('wrong-permission','crates/fairing/src/screen/cx.rs','self.access.allows(gate)','self.access.allows(&Gate::borrowed("public"))','context-observer-binding'),
 ('external-forwarding','crates/fairing/src/access/mediation.rs',None,"\nimpl ActionCallback{pub(crate)fn bypass(&mut self,cx:&mut Cx<'_>){let _=Some(cx).map(&mut *self.body);}}\n",'unguarded-payload-entry'),
]

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--only',action='append',choices=[x[0] for x in CASES]);args=parser.parse_args()
 cases=[case for case in CASES if not args.only or case[0] in args.only]
 OUT.mkdir(parents=True,exist_ok=True);report=OUT/'summary.json';report.unlink(missing_ok=True)
 before=m.source_hashes();selected=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=ROOT).decode().split('\0');results=[]
 with tempfile.TemporaryDirectory(prefix='contract-controls-',dir=ROOT/'target') as temporary:
  checkout=Path(temporary)
  for name in selected:
   if not name or not(name.startswith(('crates/','verification/','xtask/','.cargo/')) or name in ('Cargo.toml','Cargo.lock','rust-toolchain.toml')):continue
   path=ROOT/name
   if path.is_file():dest=checkout/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
  m.ROOT=checkout;m.OUT=checkout/'target/mediation';m.OUT.mkdir(parents=True)
  (m.OUT/'build').symlink_to(ROOT/'target/mediation/build',target_is_directory=True)
  for label,file,old,new,expected in cases:
   path=checkout/file;original=path.read_text()
   if old is not None:assert original.count(old)==1,(label,old)
   path.write_text(original+new if old is None else original.replace(old,new,1))
   try:
    m.build('all');m.graph('all');data=[json.loads((m.OUT/'all'/(n+'.json')).read_text())for n in m.CRATES]
    outcome=c.analyze(data);kinds={e['kind']for e in outcome['errors']}
    if outcome['unguarded_payload_entries']:kinds.add('unguarded-payload-entry')
    if expected:
     assert outcome['status']=='rejected' and expected in kinds,(label,outcome)
    else:
     assert outcome['status']=='registered-payload-confinement-checked',(label,outcome)
     assert any(p.endswith('::ContractExtension')for p in c.PROTOCOLS),'New protocol was not discovered automatically'
    (OUT/(label+'.json')).write_text(json.dumps(outcome,indent=2)+'\n')
    results.append({'case':label,'status':'compiled-and-rejected' if expected else 'compiled-and-automatically-covered','reason':expected})
    print(label+': '+results[-1]['status'],flush=True)
   except subprocess.CalledProcessError:
    shutil.copy2(m.OUT/'all/compiler.log',OUT/(label+'-compiler.log'));raise
   finally:path.write_text(original)
 m.ROOT=ROOT;m.OUT=ROOT/'target/mediation'
 assert m.source_hashes()==before,'Source changed while checking contract controls'
 report.write_text(json.dumps({'inputs':before,'controls':results},indent=2)+'\n')
if __name__=='__main__':main()
