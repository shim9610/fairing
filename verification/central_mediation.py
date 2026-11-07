#!/usr/bin/env python3
"""Discover protected contracts from rustc and certify the common admission cut.

The single runtime declaration macro emits carrier/declaration markers. No
per-element type/protocol lists are maintained by this checker.
"""
import sys
sys.modules.setdefault("central_mediation", sys.modules[__name__])
import argparse
from collections import defaultdict, deque
import json
import re
from pathlib import Path
import mediation as m

KERNEL = 'fairing::access::mediation::'
PROTOCOLS = set()
FUNCTIONS = {'core::ops::function::Fn', 'core::ops::function::FnMut', 'core::ops::function::FnOnce'}
CARRIERS = set()
# Fixed kernel interfaces; these are metadata/authority contracts, not a payload roster.
OBSERVATION_PROTOCOLS = {'fairing::access::AccessPolicy'} | {KERNEL+'protected::'+name for name in ('Permission','PermissionBound','sealed::Declaration')}


def kernel(name):
    return name.startswith(KERNEL)


def kernel_entry(body):
    visibility = body['visibility']
    private_to_kernel = visibility == KERNEL.removesuffix('::') or visibility.startswith(KERNEL)
    return kernel(body['canonical_id']) and not body['closure'] and not private_to_kernel


def context_contracts(bodies):
    """Check the concrete constructor/forwarding premises of the Cx proof model."""
    errors, scopes = [], []
    cx_type = 'fairing::screen::cx::Cx'
    path_field = cx_type + '::authority_path'
    constructors = []
    path_constructors = []
    for name, body in bodies.items():
        facts = body['constructions']
        for fact in facts:
            if path_field in fact['written_fields']:
                errors.append({'kind': 'authority-path-write', 'body': name})
            if fact['owner'] == cx_type:
                constructors.append((name, body, fact))
            if fact['owner'] == KERNEL + 'AuthorityPath':
                path_constructors.append((name, body))
    if len(path_constructors) != 1 or path_constructors[0][1]['id'] != "access::mediation::AuthorityPath::<'a>::new":
        errors.append({'kind': 'authority-path-constructor-inventory'})
    else:
        constructor = path_constructors[0][1]['id']
        consumers = [name for name, body in bodies.items() for block in body['blocks']
                     if block.get('local_callee') and block.get('callee') == constructor]
        if consumers != ['fairing::access::mediation::invoke_bound']:
            errors.append({'kind': 'unclassified-authority-path-construction', 'consumers': consumers})
    for name, body, fact in constructors:
        definitions = defaultdict(list)
        for assignment in body['constructions']:
            definitions[assignment['destination']].append(assignment)

        def value(operand, active=frozenset()):
            operand = re.sub(r'^(?:copy|move) ', '', operand)
            if operand in active:
                return ('unknown-cycle',)
            if operand in ('_2', '_3'):
                if definitions[operand]:
                    return ('reassigned-parameter', operand)
                return ('parameter', operand)
            items = definitions[operand]
            if len(items) != 1:
                return ('unknown-definition', operand)
            item = items[0]
            if item is fact:
                return ('scoped-context',)
            arguments = tuple(value(arg, active | {operand}) for arg in item['values'])
            if item['kind'] == 'use' and len(arguments) == 1:
                return arguments[0]
            if item['kind'] == 'reference' and len(arguments) == 1:
                return ('borrow', arguments[0])
            if item['kind'] == 'aggregate':
                return ('aggregate', item['owner'], item['variant'], arguments)
            return ('unknown-expression',)

        field = next(f for f in fact['fields'] if f['field'] == path_field)
        actual = value(field['value'])
        if body['id'] == "screen::cx::CxParts::<'_>::cx_in":
            if actual != ('aggregate', 'core::option::Option', 'None', ()):
                errors.append({'kind': 'root-context-contract', 'body': name, 'actual': actual})
        elif body['id'].startswith('screen::cx::Cx::') and body['id'].endswith('::with_protected_resources'):
            expected = ('aggregate', 'core::option::Option', 'Some', (('parameter', '_2'),))
            if actual != expected:
                errors.append({'kind': 'scope-context-contract', 'body': name, 'actual': actual})
            calls = [block for block in body['blocks'] if block.get('callable_trait')]
            forwarded = ('aggregate', None, None, (('borrow', ('scoped-context',)),))
            if len(calls) != 1 or value(calls[0]['arguments'][0]['value']) != ('parameter', '_3') or value(calls[0]['arguments'][1]['value']) != forwarded:
                errors.append({'kind': 'scope-callback-forwarding', 'body': name})
            scopes.append({'body': name, 'path_source': 'second argument', 'callback_context': 'borrow of constructed scoped Cx'})
        else:
            errors.append({'kind': 'unclassified-context-constructor', 'body': name})
    if len(constructors) != 2 or len(scopes) != 1:
        errors.append({'kind': 'context-constructor-inventory', 'constructors': len(constructors)})
    return errors, scopes


def analyze(data):
    global CARRIERS, PROTOCOLS
    contracts=[contract for inventory in data for contract in inventory['contracts']]
    CARRIERS={contract['owner'] for contract in contracts if contract['role']=='carrier'}
    PROTOCOLS=({protocol for contract in contracts for protocol in contract['protocols']} | {p for inventory in data for body in inventory['bodies'] for local in body['locals'] for p in local['protected_protocols']}) - FUNCTIONS
    assert CARRIERS and PROTOCOLS, 'No compiler-discovered protected contract'
    bodies = {}
    for inventory in data:
        for body in inventory['bodies']:
            name = inventory['crate'] + '::' + body['id']
            assert name not in bodies, 'Duplicate MIR definition identity'
            bodies[name] = body
    assert m.GUARD in bodies, 'Admission primitive missing'
    assert any(kernel(b['canonical_id']) for b in bodies.values()), 'No opaque kernel in inventory'
    errors = [{'kind':'contract-not-generated','owner':contract['owner']} for contract in contracts if not contract['generated']]
    errors.extend({'kind':'carrier-outside-kernel','owner':owner}for owner in CARRIERS if not kernel(owner))
    declared={contract['owner'] for contract in contracts if contract['role']=='declaration'}
    # Sealed registration and its contract marker must be emitted together.
    for inventory in data:
        for body in inventory['bodies']:
            if body['impl_protocol']==KERNEL+'protected::sealed::Declaration' and not body['closure']:
                if body['impl_owner'] not in declared:
                    errors.append({'kind':'unregistered-protected-declaration','body':body['id']})
    context_errors, context_scopes = context_contracts(bodies)
    errors.extend(context_errors)
    stores = []
    legacy_stores = []
    for inventory in data:
        for owner in inventory['storage_schema']:
            if not owner['local']: continue
            for field in owner['fields']:
                if field['id'] == 'fairing::screen::cx::Cx::authority_path' and field['visibility'] != 'fairing::screen::cx':
                    errors.append({'kind': 'authority-path-field-visibility', 'field': field['id']})
                raw = set(field['raw_protocols'])
                if raw & PROTOCOLS and not kernel(owner['id']):
                    errors.append({'kind': 'raw-protocol-storage', 'field': field['id']})
                if owner['id'] in CARRIERS:
                    if not field['visibility'].startswith('fairing::access::mediation'):
                        errors.append({'kind': 'payload-field-visibility', 'field': field['id']})
                    stores.append(field['id'])
                elif raw & FUNCTIONS or 'function-pointer' in raw:
                    legacy_stores.append(field['id'])
    references = []
    for name, body in bodies.items():
        for ref in body['protocol_references']:
            if ref['protocol'] in PROTOCOLS:
                references.append({'body': name, 'target': ref['id']})
                if not kernel(body['canonical_id']):
                    errors.append({'kind': 'raw-protocol-reference-outside-kernel', 'body': name, 'target': ref['id']})
        if kernel_entry(body):
            if set(body['return_raw_protocols']) & (PROTOCOLS | FUNCTIONS | {'function-pointer', 'core::any::Any'}):
                errors.append({'kind': 'raw-payload-return', 'body': name, 'type': body['return_type']})
            if set(body['input_payload_parameters']) & set(body['return_parameters']):
                errors.append({'kind': 'generic-payload-return', 'body': name, 'type': body['return_type']})
    assert stores and references, 'Missing payload storage/operation inventory'

    # Type-parameter points-to constraints. Definitions preserve disambiguators
    # from rustc, not display-name substring aliases or hand-written call lists.
    points = defaultdict(set)
    constraints = []
    parameter_indices = defaultdict(set)

    def object_sources(name, objects):
        values, params = set(), []
        for obj in objects:
            if obj['kind'] == 'definition':
                values.add(('definition', obj['id'], obj.get('protocol')))
            elif obj['kind'] == 'dynamic':
                values.add(('unknown', '', None))
            elif obj['kind'] == 'parameter':
                key = (name, obj['index'])
                parameter_indices[name].add(obj['index'])
                params.append(key)
            else:
                raise AssertionError('Unsupported callable value kind: ' + str(obj))
        return values, params

    def resolve(body, block):
        callee = block['callee']
        # Each body stores its compiler crate separately below.
        if block['local_callee']:
            callee = body['_crate'] + '::' + callee
        return callee if callee in bodies else None

    for inventory in data:
        for body in inventory['bodies']:
            body['_crate'] = inventory['crate']
    for name, body in bodies.items():
        for block in body['blocks']:
            if block['kind'] != 'call':
                continue
            for objects in block.get('argument_objects', []):
                object_sources(name, objects)
            callee = resolve(body, block)
            for arg in block.get('generic_objects', []):
                values, params = object_sources(name, arg['objects'])
                if callee:
                    destination = (callee, arg['index'])
                    points[destination].update(values)
                    constraints.extend((source, destination) for source in params)
    # A closure inherits its lexical function's generic substitution. Captures
    # are not executed merely because a closure was constructed or stored.
    for name, body in bodies.items():
        parent = body['lexical_parent']
        if body['closure'] and parent in bodies:
            for index in parameter_indices[name]:
                constraints.append(((parent, index), (name, index)))
    roots = {(name, 0) for name, body in bodies.items() if kernel(body['canonical_id']) and not body['closure']}
    assert roots, 'No callable kernel entry roots'
    for name, _ in roots:
        for index in parameter_indices[name]:
            points[(name, index)].add(('unknown', '', None))
    changed = True
    while changed:
        changed = False
        for source, destination in constraints:
            old = len(points[destination])
            points[destination].update(points[source])
            changed |= old != len(points[destination])

    def targets(name, objects):
        values, params = object_sources(name, objects)
        for param in params:
            values.update(points[param] or {('unknown', '', None)})
        return values

    adjacency = defaultdict(set)
    sinks = {}
    unsupported = []
    for name, body in bodies.items():
        for block in body['blocks']:
            node = (name, block['block'])
            if block['kind'] == 'unsupported':
                unsupported.append(node)
            for successor in block['successors']:
                adjacency[node].add((name, successor))
            if block['kind'] != 'call':
                continue
            callee = resolve(body, block)
            if callee == m.GUARD:
                # Only the source-proved kernel may create this admission cut.
                # A new adapter cannot authorize itself with invoke(true, ...).
                if name != KERNEL + 'invoke':
                    errors.append({'kind':'unproved-admission-cut','body':name,'block':block['block']})
                continue
            if block.get('canonical_trait') in PROTOCOLS:
                sinks[node] = 'application-protocol'
            elif block.get('canonical_trait') and not block['canonical_trait'].startswith(('core::','alloc::','std::')) and block['canonical_trait'] not in OBSERVATION_PROTOCOLS:
                sinks[node] = 'unclassified-protocol-dispatch'
            elif block.get('callable_trait'):
                objects = block.get('argument_objects', [[]])[0]
                possible = targets(name, objects)
                if not possible:
                    sinks[node] = 'unresolved-callable'
                for kind, definition, protocol in possible:
                    if kind == 'unknown' or protocol in PROTOCOLS:
                        sinks[node] = 'application-callable'
                    elif definition in bodies:
                        adjacency[node].add((definition, 0))
                    else:
                        sinks[node] = 'unresolved-function-item'
            elif block.get('indirect_function'):
                sinks[node] = 'unresolved-function-pointer'
            elif callee:
                adjacency[node].add((callee, 0))
            else:
                # External code may call function arguments. This is conservative
                # forwarding, not an assumption that std/UI bodies are verified.
                for objects in block.get('argument_objects', []):
                    for kind, definition, protocol in targets(name, objects):
                        if kind == 'definition' and definition in bodies:
                            adjacency[node].add((definition, 0))
                        elif protocol in PROTOCOLS:
                            sinks[node] = 'externally-forwarded-protocol'
                callable_parameters = set(block.get('callable_generic_parameters', []))
                for argument in block.get('generic_objects', []):
                    if argument['index'] not in callable_parameters:
                        continue
                    possible = targets(name, argument['objects'])
                    if not possible or any(kind == 'unknown' for kind, _, _ in possible):
                        sinks[node] = 'unresolved-external-callable-forwarding'
    previous = {root: None for root in roots}
    queue = deque(sorted(roots))
    while queue:
        node = queue.popleft()
        for target in sorted(adjacency[node]):
            if target not in previous:
                previous[target] = node
                queue.append(target)
    bypasses = []
    for node in sorted(set(sinks) & set(previous)):
        path = [node]
        while previous[path[-1]] is not None:
            path.append(previous[path[-1]])
        bypasses.append({'kind': sinks[node], 'path': list(reversed(path))})
    # An adapter is one protected entry, not a reusable authorization for a
    # sequence or loop of payload calls. Application bodies outside the monitor
    # retain their stated effects boundary; monitor bridges must be linear.
    for name,body in bodies.items():
        if not kernel(body['canonical_id']) or not body['closure']:continue
        entries=[block['block'] for block in body['blocks'] if block['kind']=='call' and
                 (block.get('canonical_trait') in PROTOCOLS or block.get('callable_trait') or block.get('indirect_function'))]
        if len(entries)>1:errors.append({'kind':'multiple-protected-entries','body':name,'blocks':entries})
        for entry in entries:
            pending=list(body['blocks'][entry]['successors']);seen=set()
            while pending:
                current=pending.pop()
                if current==entry:
                    errors.append({'kind':'kernel-adapter-cycle','body':name,'block':entry});break
                if current in seen:continue
                seen.add(current);pending.extend(body['blocks'][current]['successors'])
    # Unsupported code outside the reachable kernel graph is inventoried, not
    # silently called safe. An unsupported reachable node rejects the cut.
    unsupported_reachable = sorted(set(unsupported) & set(previous))
    from contract_types import Types, kernel_exports, payload_boundaries, context_isolation, observer_bindings
    types=Types(data,trusted_host=False)
    metadata={KERNEL+'protected::PermissionBinding'}
    # A generated marker cannot hide executable storage behind a false Payload.
    # Compare actual fields with the compiled associated type, through containers.
    for contract in contracts:
        if contract['role']!='carrier':continue
        expected=types.violations(contract['payload_shape'],stops=CARRIERS|metadata)
        for problem in expected:
            if problem['kind'] not in ('parameter','dynamic-input','callable'):
                errors.append({'kind':'unclassified-payload-contract','owner':contract['owner'],'detail':problem})
        def covered(problem):
            if problem['kind']=='dynamic-input':
                declared_protocols={p for item in expected if item['kind']=='dynamic-input' for p in item['protocols']}
                actual=set(problem['protocols']);produced=actual-FUNCTIONS
                # A factory may produce the declared protocol. A bare callable
                # still has to be declared; an empty Payload cannot hide it.
                return actual <= declared_protocols or bool(produced) and produced <= declared_protocols
            return any({k:v for k,v in item.items() if k!='path'}=={k:v for k,v in problem.items() if k!='path'} for item in expected)
        for field in types.definitions[contract['owner']]['fields']:
            for problem in types.violations(field['shape'],stops=CARRIERS|metadata):
                if not covered(problem):errors.append({'kind':'carrier-payload-contract-mismatch','field':field['id'],'detail':problem})
    errors.extend(kernel_exports(bodies,types))
    resource_errors,resource_contract=context_isolation(bodies,types);errors.extend(resource_errors)
    observer_errors,observer_contract=observer_bindings(bodies,types);errors.extend(observer_errors)
    exports, transfers=payload_boundaries(data,{'pre_admission_blocks':sorted(previous)})
    errors.extend(exports)
    metadata={KERNEL+'protected::PermissionBinding'}
    for inventory in data:
        for owner in inventory['storage_schema']:
            if not owner['local']:continue
            if owner['id'] in declared or (kernel(owner['id']) and owner['id'] not in CARRIERS|metadata):
                for field in owner['fields']:
                    for problem in types.violations(field['shape'],stops=CARRIERS|metadata):
                        errors.append({'kind':'unclassified-executable-storage','field':field['id'],'detail':problem})
    nodes={(name,block['block']):block for name,body in bodies.items() for block in body['blocks']}
    edges={(source,target) for source,targets in adjacency.items() for target in targets}
    # Only protected protocol/callable payload entries are sinks. Ordinary
    # library closures remain graph edges, not automatically executed payloads.
    certificate=m.solve(data[0].get('_profile','all'),'protected-contracts',sorted(roots),nodes,edges,list(sinks))
    if certificate['solver']!='unsat':errors.append({'kind':'routing-certificate-not-unsat','solver':certificate['solver']})
    return {'status': 'registered-payload-confinement-checked' if not errors and not bypasses and not unsupported_reachable else 'rejected',
        'resource_contract':resource_contract, 'observer_contract':observer_contract,
        'routing_certificate':certificate,
        'errors': errors, 'unguarded_payload_entries': bypasses, 'unsupported_reachable': unsupported_reachable,
        'opaque_payload_fields': stores, 'protocol_references': references, 'kernel_roots': sorted(roots),
        'reachable_blocks_without_admission': len(previous), 'callable_flow_constraints': len(constraints),
        'context_scope_contracts': context_scopes,
        'legacy_callable_fields_requiring_construction_proof': sorted(legacy_stores),
        'discovered_contracts': contracts,
        'pre_admission_blocks':sorted(previous), 'ownership_transfers':transfers,
        'scope': 'Compiler inventory of raw Screen/Element storage/references, opaque body visibility/returns, and conservative callable-type forwarding from kernel entries to the single admission cut. All contract-owned protected payloads are compiler-discovered; raw protocol storage/references, payload export and reachable unknown forwarding reject the certificate. Authorization decisions, ordinary Rust/standard-library semantics and arbitrary admitted application effects are interface assumptions; legacy standalone APIs are outside protected registration.'}


def check(profile):
    # Validate source/inventory receipts before consuming any compiler facts.
    m.graph(profile)
    data=[json.loads((m.OUT / profile / (crate + '.json')).read_text()) for crate in m.CRATES]
    data[0]['_profile']=profile
    return analyze(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-build', action='store_true')
    args = parser.parse_args()
    before = m.source_hashes()
    output = m.ROOT / 'target/assurance/central-mediation'
    output.mkdir(parents=True, exist_ok=True)
    path = output / 'confinement.json'
    path.unlink(missing_ok=True)
    reports = {}
    for profile in ['all', 'minimal']:
        if not args.skip_build:
            m.build(profile)
        reports[profile] = check(profile)
    assert before == m.source_hashes(), 'Sources changed while checking the admission cut'
    path.write_text(json.dumps({'profiles': reports, 'inputs': before}, indent=2) + '\n')
    print(json.dumps({p: {k: v[k] for k in ['status', 'errors', 'unguarded_payload_entries', 'unsupported_reachable']} for p, v in reports.items()}, indent=2))
    raise SystemExit(0 if all(r['status'] == 'registered-payload-confinement-checked' for r in reports.values()) else 1)


if __name__ == '__main__':
    main()
