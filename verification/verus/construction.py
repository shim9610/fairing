#!/usr/bin/env python3
"""Prove the recursive production wallpaper filter and construction capability."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'verification'))
import install
from mediation_path import function

OUT=ROOT/'target/assurance/central-mediation/construction'
SUPPORT='''use vstd::prelude::*;
verus! {
pub enum Wallpaper {
    Themed { dark: Box<Wallpaper>, light: Box<Wallpaper> },
    Solid(()), Fixed(()), Gradient { top: (), bottom: () },
    Texture { id: () }, TextureFit { id: (), source_size: (), fit: () },
    Owned { texture: (), source_size: (), fit: () },
    Painter(()), ThemedPainter(()),
}
mod desktop { pub use super::Wallpaper; }
spec fn contains_callback(value: &Wallpaper) -> bool decreases value,
{
    match value {
        Wallpaper::Painter(_) | Wallpaper::ThemedPainter(_) => true,
        Wallpaper::Themed { dark, light } => contains_callback(dark) || contains_callback(light),
        _ => false,
    }
}
'''


def generate(predicate, wrapper):
    source=function(predicate,'fn wallpaper_has_callback(')
    signature='fn wallpaper_has_callback(wallpaper: &crate::desktop::Wallpaper) -> bool'
    annotated='''fn wallpaper_has_callback(wallpaper: &crate::desktop::Wallpaper) -> (result: bool)
        ensures result == contains_callback(wallpaper),
        decreases wallpaper,
    '''
    checked=source.replace(signature,annotated,1)
    assert checked.replace(annotated,signature,1)==source
    constructor=function(wrapper,'fn new(value: Wallpaper)')
    before='fn new(value: Wallpaper) -> Option<Self>'
    after='''fn new(value: Wallpaper) -> (result: Option<Self>)
        ensures match result { Some(wrapper) => !contains_callback(&wrapper.0), None => contains_callback(&value) },
    '''
    adapted=constructor.replace(before,after,1)
    assert adapted.replace(after,before,1)==constructor
    return SUPPORT+checked+'\nstruct CallbackFreeWallpaper(Wallpaper);\nimpl CallbackFreeWallpaper {\n'+adapted+'\n}\n}\n'


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    receipt=OUT/'receipt.json';receipt.unlink(missing_ok=True)
    paths=[ROOT/p for p in ['crates/fairing/src/shell/builder.rs','crates/fairing/src/shell/builder/protected.rs','verification/verus/construction.py','verification/tools.json']]
    def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    before=hashes();predicate=paths[0].read_text();wrapper=paths[1].read_text()
    verus=install.tool('verus')
    cases=[('production',predicate,wrapper,False),
        ('ignore-inactive-theme',predicate.replace('wallpaper_has_callback(dark) || wallpaper_has_callback(light)','wallpaper_has_callback(dark)'),wrapper,True),
        ('accept-procedural-painter',predicate.replace('Wallpaper::Painter(_) | Wallpaper::ThemedPainter(_) => true','Wallpaper::Painter(_) | Wallpaper::ThemedPainter(_) => false'),wrapper,True),
        ('skip-wrapper-validation',predicate,wrapper.replace('if wallpaper_has_callback(&value)', 'if false'),True)]
    results=[]
    for name,p,w,rejected in cases:
        file=OUT/(name+'.rs');file.write_text(generate(p,w))
        run=subprocess.run([str(verus),'--crate-type=lib',str(file)],capture_output=True,text=True,cwd=ROOT)
        diagnostic=run.stdout+run.stderr;file.with_suffix('.log').write_text(diagnostic)
        if rejected:
            assert run.returncode and ('postcondition not satisfied' in diagnostic or 'assertion failed' in diagnostic),(name,diagnostic)
        elif run.returncode:
            raise SystemExit(diagnostic)
        results.append({'case':name,'status':'contract-rejected' if rejected else 'verified'})
    assert before==hashes(),'Construction proof source changed during verification'
    receipt.write_text(json.dumps({'status':'unbounded-production-construction-verified','inputs':before,'controls':results,
      'contract':'Every returned callback-free wallpaper has no procedural callback at any finite nesting depth. Data variants are parametric and are checked separately against the compiler type schema; destructors and external platform-resource implementations remain trusted.'},indent=2)+'\n')
    print('unbounded-production-construction-verified')

if __name__=='__main__':main()
