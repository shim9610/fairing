#!/usr/bin/env python3
"""Check compiler-owned containment of every library Element measure/paint entry.

This is a scoped structural certificate, not a gate/receiver/freshness refinement
or the whole-library mediation theorem. It consumes the strict detector inventory.
"""
import hashlib
import json
import subprocess
import re
from pathlib import Path
import mediation as m

ROOT = m.ROOT
OUT = ROOT / 'target/assurance/elements'

def check_bodies(bodies):
    sites = []
    for name, body in bodies.items():
        for block in body['blocks']:
            for statement in block['statements']:
                assert not re.search(r'Element[^\n]*>::(?:paint|measure)|elements::Element::(?:paint|measure)', statement), 'Element method referenced as an untracked function item'
            if block.get('indirect_function'):
                assert not any('elements::Element' in arg['type'] for arg in block.get('arguments', [])), 'Indirect Element callback forwarding is unresolved'
            if block['kind'] != 'call' or block.get('trait') != 'elements::Element':
                continue
            assert block['callee'] in {'elements::Element::measure', 'elements::Element::paint'}, 'New Element entry requires a contract'
            assert body['closure'], 'Element callback entry outside a guarded closure: ' + name
            consumers = []
            for parent_name, parent in bodies.items():
                for call in parent['blocks']:
                    if call['kind'] != 'call': continue
                    if name not in call.get('closure_arguments', []): continue
                    assert parent['crate'] == 'fairing' and call['local_callee'], 'Closure forwarded outside the library'
                    assert call['callee'].startswith('access::protected::PermissionBinding::<') and call['callee'].endswith('>::run'), 'Closure forwarded outside PermissionBinding::run: ' + parent_name
                    assert len(call['arguments']) == 3
                    operand = call['arguments'][2]
                    assert operand['type'].startswith('{closure@'), 'Unexpected closure argument shape'
                    match = re.fullmatch(r'move (_\d+)', operand['value'])
                    assert match, 'Guard must consume the callback closure exactly once'
                    local = match[1]
                    token = re.compile(r'(?<![A-Za-z0-9_])' + local + r'(?![A-Za-z0-9_])')
                    constructors = []
                    for other in parent['blocks']:
                        for statement in other['statements']:
                            if not token.search(statement): continue
                            if re.fullmatch(r'Storage(?:Live|Dead)\(' + local + r'\)', statement): continue
                            assert statement.startswith(local + ' = {closure@'), 'Callback closure copied/stored/escaped: ' + statement
                            constructors.append(statement)
                        if not token.search(other['terminator']): continue
                        assert other['block'] == call['block'], 'Callback closure has another terminator use'
                    assert len(constructors) == 1, 'Callback must have one construction and one guarded consumption'
                    consumers.append({'body': parent_name, 'block': call['block'], 'closure_local': local})
            assert len(consumers) == 1, 'Callback closure must have exactly one guarded consumer: ' + name
            sites.append({'body': name, 'block': block['block'], 'callee': block['callee'], 'location': block['location'], 'consumer': consumers[0]})
    for name in {site['body'] for site in sites}:
        assert sum(site['body'] == name for site in sites) == 1, 'Multiple Element entries in one admission closure'
        blocks = {block['block']: block for block in bodies[name]['blocks']}
        active = set(); done = set()
        def acyclic(node):
            if node in done: return
            assert node not in active, 'An admission closure could repeat an Element entry'
            active.add(node)
            for target in blocks[node]['successors']: acyclic(target)
            active.remove(node); done.add(node)
        acyclic(0)
    assert {site['callee'] for site in sites} == {'elements::Element::measure', 'elements::Element::paint'}, 'Element callback inventory missing'
    return sites

def check(profile):
    bodies, nodes, edges, _, _, _, _, unsupported, inventory = m.graph(profile)
    assert not unsupported, 'Unsupported compiler terminator in scoped inventory'
    sites = check_bodies(bodies)
    region = {node for node in nodes if any(site['body'] == node[0] for site in sites)}
    guards = {(site['consumer']['body'], site['consumer']['block']) for site in sites}
    roots = {(name, 0) for name, body in bodies.items() if body['public'] and not body['closure']}
    assert not roots & region, 'An Element callback closure is an external root'
    incoming = [(a, b) for a, b in edges if a not in region and b in region]
    assert incoming and all(a in guards for a, b in incoming), 'Compiler graph enters a callback outside the checked guard cut'
    return {'profile': profile, 'status': 'element-entry-closure-containment-checked', 'sites': sites, 'inventory': inventory, 'region_blocks': len(region), 'guard_cut': sorted(guards), 'crossing_edges': sorted(incoming), 'graph_roots': len(roots), 'unbounded_graph_cut': 'checked-instance-of-Verus-entry-cut-lemma',
        'scope': 'Every compiled library Element::measure/paint call occurs inside a closure constructed once, never copied/stored, and consumed only by PermissionBinding::run. Gate/receiver matching, context freshness, indirect protocol forwarding, dependency implementations and whole-library mediation need separate evidence.'}

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'coverage.json').unlink(missing_ok=True)
    compiler_inputs = m.source_hashes()
    paths = [Path(__file__), ROOT / 'verification/element_coverage_controls.py', ROOT / 'verification/verus/element_coverage.rs']
    before = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    source = ROOT / 'verification/verus/element_coverage.rs'
    verus = m.install.tool('verus')
    command = [str(verus), '--crate-type=lib', str(source)]
    with (OUT / 'verus-cut.log').open('w') as log:
        subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
    assert '2 verified, 0 errors' in (OUT / 'verus-cut.log').read_text()
    profiles = [check(profile) for profile in ['all', 'minimal']]
    negative = []
    for label, old in [('missing-cut', '        cut(edges, region, guards),\n'),
                       ('missing-root-condition', '        !region.contains(path[0]),\n'),
                       ('missing-no-guard-condition', '        forall |i:int| 0 <= i < path.len() ==> !guards.contains(#[trigger] path[i]),\n')]:
        text = source.read_text(); assert text.count(old) == 2
        mutant = OUT / (label + '.rs'); mutant.write_text(text.replace(old, ''))
        with (OUT / (label + '.log')).open('w') as log:
            result = subprocess.run([str(verus), '--crate-type=lib', str(mutant)], cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        log = (OUT / (label + '.log')).read_text()
        assert result.returncode and ('assertion failed' in log or 'postcondition not satisfied' in log), 'Unsound graph lemma control: ' + label
        negative.append({'case': label, 'status': 'rejected'})
    assert before == {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    assert compiler_inputs == m.source_hashes(), 'Compiler inputs changed during coverage checking'
    report = {'profiles': profiles, 'inputs': compiler_inputs, 'proof_inputs': before, 'verus_command': command, 'negative_proof_controls': negative}
    (OUT / 'coverage.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report['profiles'], indent=2))

if __name__ == '__main__': main()
