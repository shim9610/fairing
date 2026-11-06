#!/usr/bin/env python3
"""Require actual compiler fixtures to reject missing/escaped Element admission."""
import json
import os
from pathlib import Path
import subprocess
import element_coverage as c
import mediation as m

OUT = c.OUT / 'coverage-controls'
HEADER = '''pub mod elements { pub trait Element { fn measure(&mut self); fn paint(&mut self); } }
pub mod access { pub mod protected {
 pub struct PermissionBinding<P>(pub P);
 impl<P> PermissionBinding<P> {
  pub fn run(&self, cx: &mut (), callback: impl FnOnce(&mut ())) { callback(cx); }
 }
} }
use access::protected::PermissionBinding;
use elements::Element;
pub fn guarded(b: &PermissionBinding<()>, cx: &mut (), e: &mut dyn Element) {
 b.run(cx, |_| e.measure()); b.run(cx, |_| e.paint());
}
'''
CASES = [
    ('guarded', '', True),
    ('direct-bypass', 'pub fn bad(e: &mut dyn Element) { e.paint(); }', False),
    ('after-guard', 'pub fn bad(b:&PermissionBinding<()>, cx:&mut (), e:&mut dyn Element) { b.run(cx, |_| e.measure()); e.paint(); }', False),
    ('foreign-forwarding', 'pub fn bad(e:&mut dyn Element) { forward(|| e.paint()); } fn forward(f:impl FnOnce()) { f(); }', False),
    ('stored-closure', 'pub fn bad(b:&PermissionBinding<()>, cx:&mut (), e:&mut dyn Element) { let f=Some(|_: &mut ()| e.paint()).unwrap(); b.run(cx,f); }', False),
    ('indirect-function-parameter', 'pub fn bad(e:&mut dyn Element, f:fn(&mut dyn Element)) { f(e); }', False),
    ('function-pointer', 'pub fn bad(e:&mut dyn Element) { let f=<dyn Element>::paint; f(e); }', False),
    ('two-entries-one-guard', 'pub fn bad(b:&PermissionBinding<()>, cx:&mut (), e:&mut dyn Element) { b.run(cx, |_| { e.measure(); e.paint(); }); }', False),
    ('repeated-entry-loop', 'pub fn bad(b:&PermissionBinding<()>, cx:&mut (), e:&mut dyn Element, again:bool) { b.run(cx, |_| { loop { e.paint(); if !again { break; } } }); }', False),
]

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'summary.json').unlink(missing_ok=True)
    channel = m.install.PINS['kani_toolchain']
    sysroot = Path(subprocess.check_output(['rustup','run',channel,'rustc','--print','sysroot'],text=True).strip())
    env = dict(os.environ, LD_LIBRARY_PATH=os.pathsep.join([str(sysroot/'lib'), str(sysroot/'lib/rustlib/x86_64-unknown-linux-gnu/lib')]))
    results=[]
    for name, extra, expected in CASES:
        dest=OUT/name; dest.mkdir(exist_ok=True)
        source=dest/'fixture.rs'; source.write_text(HEADER+extra)
        env['FAIRING_MEDIATION_OUTPUT']=str(dest)
        with (dest/'compiler.log').open('w') as log:
            subprocess.run([str(m.OUT/'driver'),str(sysroot/'bin/rustc'),'--crate-name','fairing','--crate-type','lib',str(source),'--emit','metadata','-o',str(dest/'fixture.rmeta')],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        data=json.loads((dest/'fairing.json').read_text())
        bodies={'fairing::'+body['id']:body|{'crate':'fairing'} for body in data['bodies']}
        reason=''
        try:c.check_bodies(bodies); accepted=True
        except AssertionError as error: accepted=False; reason=str(error)
        assert accepted==expected,(name,accepted,expected,reason)
        results.append({'case':name,'accepted':accepted,'reason':reason})
        print(name+(': accepted' if accepted else ': rejected ('+reason+')'),flush=True)
    (OUT/'summary.json').write_text(json.dumps(results,indent=2)+'\n')

if __name__=='__main__':main()
