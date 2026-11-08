#!/usr/bin/env python3
"""Prove the monitor and require automatic coverage of every protected contract."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from validation import PROOF_INPUTS, require
import central_mediation as c
import mediation as m


def receipt(path,status):
    data=json.loads(path.read_text())
    require(data['status']==status, (path,data.get('status')))
    require(set(data['inputs']) == set(PROOF_INPUTS[status]), 'Incomplete proof inputs: '+str(path))
    require(data['inputs'] and all((m.ROOT/name).is_file() and m.digest(m.ROOT/name)==value for name,value in data['inputs'].items()), 'Stale kernel proof: '+str(path))
    return {'path':str(path.relative_to(m.ROOT)),'sha256':m.digest(path),'status':status}


def verify(skip_build=False,prove=True,profiles=('all','minimal')):
    require(bool(profiles) and len(set(profiles)) == len(profiles) and set(profiles) <= {'all', 'minimal'}, 'Expected distinct all/minimal profiles')
    before=m.source_hashes();out=m.ROOT/'target/assurance/protected-contracts';out.mkdir(parents=True,exist_ok=True)
    report=out/'summary.json';report.unlink(missing_ok=True)
    if prove:subprocess.run([sys.executable,str(m.ROOT/'verification/central_proofs.py')],cwd=m.ROOT,check=True)
    proofs=[receipt(m.ROOT/'target/assurance/central-mediation'/file,status)for file,status in [
        ('kernel-proof.json','shared-production-admission-kernel-verified'),
        ('path-proof.json','unbounded-production-path-verified')]]
    results={}
    for profile in profiles:
        if not skip_build:m.build(profile)
        results[profile]=c.check(profile)
    good=all(result['status']=='registered-payload-confinement-checked' and result['routing_certificate']['solver']=='unsat' for result in results.values())
    require(m.source_hashes()==before, 'Source changed during kernel/coverage connection')
    summary={'status':'protected-contracts-verified' if good else 'rejected',
        'property':'Every library-managed protected payload entry is routed through the proved common mediation kernel.',
        'inputs':before,'kernel_proofs':proofs,'profiles':results,
        'contract_source':'crates/fairing/src/access/mediation/contract.rs',
        'assumptions':'Permission metadata and authoritative gate/stamp observations obey their declared interfaces; ordinary Rust privacy, borrowing and standard-library container semantics; compiler MIR extraction and solver. Admitted application bodies, external authentication/backends, destructors and explicit standalone legacy APIs retain their documented trust boundary. This is a mediation/routing certificate, not an unrestricted Rust information-flow or physical-time cancellation theorem.'}
    report.write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({'status':summary['status'],'property':summary['property'],'profiles':{
        profile:{'contracts':len(result['discovered_contracts']),'solver':result['routing_certificate']['solver'],'errors':result['errors'],'unguarded_payload_entries':result['unguarded_payload_entries']}for profile,result in results.items()}},indent=2))
    return summary


def main():
    import argparse
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--skip-build',action='store_true');parser.add_argument('--reuse-kernel-proof',action='store_true');args=parser.parse_args()
    result=verify(args.skip_build,not args.reuse_kernel_proof)
    raise SystemExit(0 if result['status']=='protected-contracts-verified' else 1)
if __name__=='__main__':main()
