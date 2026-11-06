#!/usr/bin/env python3
"""Find unmediated callback candidates in compiler MIR; unresolved paths never certify safety."""
import argparse
from collections import Counter, deque
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import install

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/mediation'
PACKAGES = ('fairing', 'fairing-widgets')
CRATES = ('fairing', 'fairing_widgets')
GUARD = 'fairing::access::protected_dispatch::invoke'

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def source_hashes():
    paths = list((ROOT / 'crates').rglob('*.rs')) + list((ROOT / 'crates').rglob('Cargo.toml'))
    paths += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'verification/mediation_driver.rs', 'verification/mediation.py', 'verification/mediation_controls.py', 'verification/fixtures/mediation.rs', 'verification/examples/mediation_probe.rs', 'verification/tools.json']]
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(paths)}

def build(profile):
    before=source_hashes()
    channel = install.PINS['kani_toolchain']
    dest = OUT / profile
    dest.mkdir(parents=True, exist_ok=True)
    sysroot = Path(subprocess.check_output(['rustup', 'run', channel, 'rustc', '--print', 'sysroot'], text=True).strip())
    compiler = subprocess.check_output(['rustup', 'run', channel, 'rustc', '-vV'], text=True)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(OUT / 'build'),
               FAIRING_MEDIATION_OUTPUT=str(dest), RUSTC_WORKSPACE_WRAPPER=str(OUT / 'driver'),
               LD_LIBRARY_PATH=os.pathsep.join([str(sysroot/'lib'),str(sysroot/'lib/rustlib/x86_64-unknown-linux-gnu/lib')]))
    env.pop('RUSTFLAGS', None)
    with (dest/'compiler.log').open('w') as log:
        def run(command): subprocess.run(command,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        run(['rustup','run',channel,'rustc',str(ROOT/'verification/mediation_driver.rs'),'-o',str(OUT/'driver'),'-C','prefer-dynamic','-C','rpath','-L',f'native={sysroot / "lib"}'])
        # A successful Cargo "Fresh" is insufficient: remove only the selected
        # workspace artifacts and require newly emitted inventories for both crates.
        run(['cargo','+'+channel,'clean',*sum((['-p',p] for p in PACKAGES),[])])
        for name in CRATES: (dest/(name+'.json')).unlink(missing_ok=True)
        command=['cargo','+'+channel,'check','--locked',*sum((['-p',p] for p in PACKAGES),[])]
        command += ['--all-features'] if profile=='all' else ['--no-default-features']
        run(command)
    (dest/'compiler-version.txt').write_text(compiler)
    assert source_hashes()==before,'Source changed while extracting compiler MIR'
    (dest/'input-receipt.json').write_text(json.dumps({'inputs':before,'compiler':compiler,'profile':profile,'inventories':{n:digest(dest/(n+'.json')) for n in CRATES}},indent=2)+'\n')

def graph(profile):
    receipt=json.loads((OUT/profile/'input-receipt.json').read_text())
    assert receipt['inputs']==source_hashes(),'Stale compiler inventory; rebuild before mediation analysis'
    assert receipt['profile']==profile
    bodies={};inventory=[]
    for crate in CRATES:
        p=OUT/profile/(crate+'.json');data=json.loads(p.read_text())
        assert receipt['inventories'][crate]==digest(p),'Compiler inventory changed after extraction'
        assert data['crate']==crate
        assert data['mir_keys']==len(data['bodies'])+len(data['skipped_non_functions'])
        for body in data['bodies']:
            key=crate+'::'+body['id']
            if key in bodies:raise RuntimeError('Duplicate compiler body '+key)
            bodies[key]=body|{'crate':crate}
        inventory.append({'crate':crate,'runtime_bodies':len(data['bodies']),'compiler_mir_keys':data['mir_keys'],'non_function_bodies':len(data['skipped_non_functions']),'sha256':digest(p)})
    assert GUARD in bodies, 'Protected dispatcher omitted from compiler inventory'
    def resolve(body,name,local=False):
        # Preserve the compiler's crate ownership; a foreign symbol must never
        # resolve to a same-spelled local module. Closure IDs are fully qualified.
        key=body['crate']+'::'+name if local else name
        return key if key in bodies else None
    nodes={};edges=set();edge_kinds={};callbacks=[];boundaries=[];unsupported=[]
    def edge(a,b,kind):
        edges.add((a,b));edge_kinds.setdefault((a,b),set()).add(kind)
    for name,body in bodies.items():
        for block in body['blocks']:
            node=(name,block['block']);nodes[node]=block
            if block['kind']=='drop':boundaries.append({'body':name,'block':block['block'],'kind':'implicit-destructor','location':block['location']})
            if block['kind']=='unsupported':unsupported.append({'body':name,'block':block['block'],'terminator':block['terminator']})
            # Conservative normal/cleanup continuations remain reachable. Returning
            # from a guarded dispatch does not authorize the caller's next callback.
            for successor in block['successors']:edge(node,(name,successor),'cfg-continuation')
            if block['kind']!='call':continue
            callee=resolve(body,block['callee'],block['local_callee'])
            if callee==GUARD:continue
            if callee is not None:edge(node,(callee,0),'direct-call')
            # Upstream functions may execute closure arguments (e.g. egui and
            # catch_unwind). Include these edges without assuming they always do.
            for closure in block['closure_arguments']:
                target=resolve(body,closure)
                if target is not None:edge(node,(target,0),'possible-closure-forwarding')
                else:boundaries.append({'body':name,'block':block['block'],'kind':'unresolved-closure','callee':closure,'location':block['location']})
            trait=block['trait'] or ''
            if block['callable_trait'] or block['local_protocol'] or block['indirect_function']:
                callbacks.append(node)
            if trait or (callee is None and not block['closure_arguments']):
                boundaries.append({'body':name,'block':block['block'],'kind':'trait-or-external-call','callee':block['callee'],'trait':block['trait'],'location':block['location']})
    # Callbacks inside invoke are covered by its existing local contract. The
    # analysis does not treat a previous invocation as a permanent authorization.
    edges={e for e in edges if e[0][0]!=GUARD and e[1][0]!=GUARD}
    callback_sites=[{'body':n[0],'block':n[1],'location':nodes[n]['location'],'callee':nodes[n]['callee'],'receiver_type':nodes[n]['arguments'][0]['type'] if nodes[n]['arguments'] else None} for n in callbacks]
    return bodies,nodes,edges,edge_kinds,callbacks,callback_sites,boundaries,unsupported,inventory

def reachable(roots,edges):
    adj={}
    for a,b in sorted(edges):adj.setdefault(a,[]).append(b)
    previous={node:None for node in roots};queue=deque(roots)
    while queue:
        node=queue.popleft()
        for target in adj.get(node,[]):
            if target not in previous:previous[target]=node;queue.append(target)
    return previous

def solve(profile,scope,roots,nodes,edges,sites):
    ids={node:i for i,node in enumerate(sorted(nodes))}
    # Finite Horn clauses compute the least reachable set. Edges overapproximate
    # branch feasibility/dispatch: SAT is a candidate, not a concrete Rust trace.
    lines=['(set-option :fp.engine datalog)','(set-option :timeout 120000)',
           '(define-sort N () (_ BitVec 32))','(declare-rel Edge (N N))','(declare-rel Reach (N))','(declare-rel Sink (N))','(declare-rel Bad ())','(declare-var x N)','(declare-var y N)',
           '(rule (=> (and (Reach x) (Edge x y)) (Reach y)))','(rule (=> (and (Reach x) (Sink x)) Bad))']
    for a,b in sorted(edges):lines.append(f'(rule (Edge (_ bv{ids[a]} 32) (_ bv{ids[b]} 32)))')
    for node in roots:lines.append(f'(rule (Reach (_ bv{ids[node]} 32)))')
    for node in sites:
        if node[0]!=GUARD:lines.append(f'(rule (Sink (_ bv{ids[node]} 32)))')
    lines.append('(query Bad)')
    path=OUT/profile/(scope+'.smt2');path.write_text('\n'.join(lines)+'\n')
    z3=install.tool('verus').parent/'z3'
    result=subprocess.run([str(z3),str(path)],capture_output=True,text=True,check=True)
    (OUT/profile/(scope+'-solver.log')).write_text(result.stdout+result.stderr)
    verdict=result.stdout.strip();assert verdict in ('sat','unsat','unknown'),verdict
    previous=reachable(roots,edges);hits=[n for n in sites if n in previous and n[0]!=GUARD]
    assert verdict=='unknown' or (verdict=='sat')==bool(hits), 'Independent reachability disagrees with solver'
    witnesses=[]
    for hit in sorted(hits):
        path=[];node=hit
        while node is not None:path.append(node);node=previous[node]
        path.reverse()
        assert path[0] in roots and all((a,b) in edges for a,b in zip(path,path[1:])), 'Invalid witness certificate'
        witnesses.append({'sink':{'body':hit[0],'block':hit[1],'location':nodes[hit]['location']},'path':[{'body':n[0],'block':n[1],'location':nodes[n]['location']} for n in path]})
    return {'solver':verdict,'roots':len(roots),'reached_blocks':len(previous),'potential_unmediated_sites':len(hits),'witnesses':witnesses,'smt_sha256':digest(OUT/profile/(scope+'.smt2'))}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-build',action='store_true')
    parser.add_argument('--profile',choices=['all','minimal','both'],default='both')
    mode=parser.add_mutually_exclusive_group()
    mode.add_argument('--require-proved',action='store_true',help='Fail for candidates, unknown boundaries or an unproved compiler/model correspondence')
    mode.add_argument('--expect-counterexample',action='store_true',help='Detector regression: require the known compiler path AND its concrete denied-session reproduction')
    parser.add_argument('--skip-probe',action='store_true',help='Analysis only; cannot confirm or certify the concrete program')
    args=parser.parse_args();before=source_hashes();reports={}
    for profile in ['all','minimal'] if args.profile=='both' else [args.profile]:
        if not args.skip_build:build(profile)
        bodies,nodes,edges,kinds,sites,callbacks,boundaries,unsupported,inventory=graph(profile)
        roots=[(name,0) for name,body in bodies.items() if body['public'] and not body['closure'] and not name.startswith(GUARD)]
        protected=[(name,0) for name,body in bodies.items() if body['public'] and name.startswith('fairing::shell::protected::ProtectedShell::')]
        assert roots and protected
        report={'compiler_inventory':inventory,'body_count':len(bodies),'blocks':len(nodes),'edges':len(edges),'callback_sites':callbacks,'unresolved_boundaries':boundaries,'unsupported_terminators':unsupported,
                'all_public_roots':solve(profile,'all-public',roots,nodes,edges,sites),
                'protected_shell_roots':solve(profile,'protected-shell',protected,nodes,edges,sites)}
        (OUT/profile/'report.json').write_text(json.dumps(report,indent=2)+'\n');reports[profile]={k:report[k] for k in ['compiler_inventory','body_count','blocks','edges']}
        reports[profile]['potential_unmediated_sites']=report['protected_shell_roots']['potential_unmediated_sites'];reports[profile]['unresolved_boundaries']=len(boundaries);reports[profile]['unsupported_terminators']=len(unsupported)
    probe=None
    if not args.skip_probe:
        command=['cargo','run','--locked','--manifest-path',str(ROOT/'verification/Cargo.toml'),'--example','mediation_probe','--','--expect-counterexample']
        env=dict(os.environ,CARGO_INCREMENTAL='0')
        result=subprocess.run(command,cwd=ROOT,env=env,capture_output=True,text=True)
        (OUT/'concrete-probe.log').write_text(result.stdout+result.stderr)
        records=[json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        assert records,'Concrete probe failed without a structured observation; inspect concrete-probe.log'
        probe=records[-1]|{'command':command,'exit_code':result.returncode}
        if probe['status']=='counterexample-confirmed':
            assert result.returncode==0 and probe['unauthenticated'] and probe['required_permission_denied'] and probe['app_state_writes']>0 and probe['secret_in_output_shapes']
            for profile in reports:
                report=json.loads((OUT/profile/'report.json').read_text())
                assert report['protected_shell_roots']['solver']=='sat','Solver did not confirm the candidate reachability query'
                witnesses=[w for w in report['protected_shell_roots']['witnesses'] if w['sink']['body']=='fairing::chrome::status_bar::StatusBar::ui_with']
                assert witnesses,'Native counterexample has no matching compiler path; model correspondence changed'
                (OUT/profile/'confirmed-status-painter-path.json').write_text(json.dumps(witnesses[0],indent=2)+'\n')
    assert source_hashes()==before,'Inputs changed during mediation analysis'
    confirmed=probe is not None and probe['status']=='counterexample-confirmed'
    summary={'property':'Every application callback entry is mediated by a current permission check.','status':'violated-confirmed' if confirmed else 'not-proved','profiles':reports,'inputs':before,'concrete_probe':probe,
             'scope':'Compiler MIR call/CFG inventory and finite Horn reachability over a conservative graph. SAT paths are candidates until reproduced. Generic/trait dispatch, external bodies, branch feasibility and complete compiler/model refinement remain unresolved; no absence theorem is claimed. The local invoke contract is assumed only for its scoped callback execution, never for later calls.'}
    (OUT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:v for k,v in summary.items() if k!='inputs'},indent=2))
    if args.expect_counterexample:
        assert confirmed,'Expected mediation counterexample was not reproduced'
    elif args.require_proved or confirmed:
        raise SystemExit('COMPLETE_MEDIATION_VIOLATED' if confirmed else 'UNPROVED_COMPLETE_MEDIATION: inspect report paths and unresolved boundaries')

if __name__=='__main__':main()
