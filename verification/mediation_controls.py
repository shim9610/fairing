#!/usr/bin/env python3
"""Compile real Rust fixtures and require independent SAT/UNSAT mediation outcomes."""
import json
import os
from pathlib import Path
import subprocess

import install
from validation import require
import mediation as m

def main():
    dest=m.OUT/'fixture';dest.mkdir(parents=True,exist_ok=True)
    channel=install.PINS['kani_toolchain']
    sysroot=Path(subprocess.check_output(['rustup','run',channel,'rustc','--print','sysroot'],text=True).strip())
    env=dict(os.environ,FAIRING_MEDIATION_OUTPUT=str(dest),LD_LIBRARY_PATH=os.pathsep.join([str(sysroot/'lib'),str(sysroot/'lib/rustlib/x86_64-unknown-linux-gnu/lib')]))
    command=[str(m.OUT/'driver'),str(sysroot/'bin/rustc'),'--crate-name','fairing','--crate-type','lib',str(m.ROOT/'verification/fixtures/mediation.rs'),'--emit','metadata','-o',str(dest/'fixture.rmeta')]
    with (dest/'compiler.log').open('w') as log:subprocess.run(command,env=env,cwd=m.ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
    # No partner crate in this standalone fixture. Its empty inventory is explicit.
    (dest/'fairing_widgets.json').write_text(json.dumps({'crate':'fairing_widgets','mir_keys':0,'skipped_non_functions':[],'bodies':[]})+'\n')
    receipt={'inputs':m.source_hashes(),'profile':'fixture','inventories':{n:m.digest(dest/(n+'.json')) for n in m.CRATES}}
    (dest/'input-receipt.json').write_text(json.dumps(receipt)+'\n')
    bodies,nodes,edges,_,sites,_,_,unsupported,_=m.graph('fixture')
    require(not unsupported, 'Assurance condition failed: not unsupported')
    results=[]
    for name,expected in [('guarded_only','unsat'),('direct_bypass','sat'),('bypass_after_a_guard','sat'),('pointer_bypass','sat'),('virtual_bypass','sat'),('unreachable_callback','unsat')]:
        root=('fairing::'+name,0)
        require(root in nodes, 'Compiler omitted fixture body '+name)
        result=m.solve('fixture',name,[root],nodes,edges,sites)
        require(result['solver']==expected, (name,result['solver'],expected))
        results.append({'case':name,'expected':expected,'actual':result['solver']})
    # Receipt integrity must reject inventories whose contents/counts are altered.
    path=dest/'fairing.json';original=path.read_bytes();path.write_bytes(original+b' ')
    rejected=False
    try:m.graph('fixture')
    except AssertionError:rejected=True
    finally:path.write_bytes(original)
    require(rejected, 'Modified compiler inventory was silently accepted')
    results.append({'case':'tampered-compiler-inventory','actual':'rejected'})
    receipt_path=dest/'input-receipt.json';original_receipt=receipt_path.read_bytes()
    stale=json.loads(original_receipt);first=next(iter(stale['inputs']));stale['inputs'][first]='0'*64
    receipt_path.write_text(json.dumps(stale)+'\n');rejected=False
    try:m.graph('fixture')
    except AssertionError:rejected=True
    finally:receipt_path.write_bytes(original_receipt)
    require(rejected, 'Stale source receipt was silently accepted')
    results.append({'case':'stale-source-inventory','actual':'rejected'})
    (dest/'controls.json').write_text(json.dumps(results,indent=2)+'\n')
    print(json.dumps(results,indent=2))

if __name__=='__main__':main()
