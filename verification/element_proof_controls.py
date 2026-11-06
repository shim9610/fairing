#!/usr/bin/env python3
"""Require Kani to reject real production permission/plan mutations."""
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import element_proofs as p

OUT=p.OUT/'proof-controls'
CASES=[
 ('wrong-permission-value', 'crates/fairing/src/access/protected.rs',
  'gate: required.gate(),', 'gate: Gate::borrowed("read"),',
  'complete_leaf_traversal_uses_own_live_permission_and_prepared_revision', 'unauthorized measurement callback'),
 ('missing-preallocation-validation', 'crates/fairing/src/elements.rs',
  'if !self.validate(&plan, cx, stamp) {', 'if false {',
  'changed_requirement_invalidates_even_when_both_permissions_are_granted', 'stale requirement allocated geometry'),
]

def semantic_failure(log, marker):
 return any(marker in description for description in re.findall(r'Status: FAILURE\n\s*- Description: ([^\n]*)',log))

def main():
 # Regression: an undetermined property printed in a full log is not its failure.
 fixture='Status: UNDETERMINED\n - Description: unauthorized measurement callback\nStatus: FAILURE\n - Description: unwinding assertion loop 0\n'
 assert not semantic_failure(fixture,'unauthorized measurement callback')
 OUT.mkdir(parents=True,exist_ok=True);(OUT/'summary.json').unlink(missing_ok=True)
 before=p.hashes(); channel=p.install.PINS['kani_toolchain']; kani=p.install.tool('kani')
 sysroot=Path(subprocess.check_output(['rustup','run',channel,'rustc','--print','sysroot'],text=True).strip())
 env=dict(os.environ,RUSTUP_TOOLCHAIN=channel,LD_LIBRARY_PATH=str(sysroot/'lib'))
 env['PATH']=str(kani.parent)+os.pathsep+env['PATH']
 results=[]
 with tempfile.TemporaryDirectory(prefix='fairing-element-proof-controls-') as temporary:
  root=Path(temporary)
  for source in p.inputs():
   dest=root/source.relative_to(p.ROOT);dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,dest)
  for name, relative, old, new, target, marker in CASES:
   source=root/relative;original=source.read_text();assert original.count(old)==1
   source.write_text(original.replace(old,new,1))
   dest=OUT/name;harness=p.generate(dest,root)
   selected=['--harness',target]
   bounds=p.loop_arguments(harness,kani,env,selected)
   with (dest/'kani.log').open('w') as log:
    result=subprocess.run([str(kani),str(harness),*selected,*bounds],env=env,stdout=log,stderr=subprocess.STDOUT)
   source.write_text(original)
   log=(dest/'kani.log').read_text()
   assert result.returncode and 'VERIFICATION:- FAILED' in log and semantic_failure(log,marker),'Production mutation not rejected semantically: '+name
   results.append({'case':name,'status':'rejected','harness':target,'marker':marker,'log':str(dest/'kani.log')})
   print(name+': rejected',flush=True)
 assert p.hashes()==before,'Element proof inputs changed during mutation controls'
 (OUT/'summary.json').write_text(json.dumps({'inputs':before,'controls':results},indent=2)+'\n')

if __name__=='__main__':main()
