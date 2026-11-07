#!/usr/bin/env python3
"""Check opaque protocol storage and admission cuts in compiler-owned MIR.

The check is compositional over carrier operations, rather than a manually listed
set of screen/action call sites. It deliberately does not discharge the separate
legacy-hook construction/noninstallation invariant or atomic observation model.
"""
import argparse
from collections import defaultdict, deque
import json
import re
from pathlib import Path
import mediation as m

KERNEL = 'fairing::access::mediation::'
PROTOCOLS = {'fairing::screen::Screen', 'fairing::elements::Element'}
FUNCTIONS = {'core::ops::function::Fn', 'core::ops::function::FnMut', 'core::ops::function::FnOnce'}
CARRIERS = {KERNEL + name for name in ('Callback', 'FactoryCallback', 'ScreenHandle', 'element::ElementHandle', 'declared::Declared', 'protected::Protected')}
CARRIER_FIELDS = {
    KERNEL+'Callback': {'body','requirement'},
    KERNEL+'FactoryCallback': {'body','requirement'},
    KERNEL+'ScreenHandle': {'body','factory','requirement'},
    KERNEL+'element::ElementHandle': {'body'},
    KERNEL+'declared::Declared': {'body','binding'},
    KERNEL+'protected::Protected': {'body','binding'},
}


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


def analyze(data, root_filter=kernel_entry):
    bodies = {}
    for inventory in data:
        for body in inventory['bodies']:
            name = inventory['crate'] + '::' + body['id']
            assert name not in bodies, 'Duplicate MIR definition identity'
            bodies[name] = body
    assert m.GUARD in bodies, 'Admission primitive missing'
    assert any(kernel(b['canonical_id']) for b in bodies.values()), 'No opaque kernel in inventory'
    errors = []
    context_errors, context_scopes = context_contracts(bodies)
    errors.extend(context_errors)
    stores = []
    legacy_stores = []
    observed_carriers=set()
    for inventory in data:
        for owner in inventory['storage_schema']:
            if not owner['local']:
                continue
            if owner['id'] in CARRIERS:
                observed_carriers.add(owner['id'])
                if {field['id'].rsplit('::',1)[-1] for field in owner['fields']}!=CARRIER_FIELDS[owner['id']]:
                    errors.append({'kind':'carrier-field-contract','owner':owner['id']})
            for field in owner['fields']:
                if field['id'] == 'fairing::screen::cx::Cx::authority_path' and field['visibility'] != 'fairing::screen::cx':
                    errors.append({'kind': 'authority-path-field-visibility', 'field': field['id']})
                raw = set(field['raw_protocols'])
                if raw & PROTOCOLS and not kernel(owner['id']):
                    errors.append({'kind': 'raw-protocol-storage', 'field': field['id']})
                if kernel(owner['id']) and field['id'].endswith('::body'):
                    if owner['id'] not in CARRIERS:
                        errors.append({'kind':'unclassified-payload-owner','owner':owner['id']})
                    if not field['visibility'].startswith('fairing::access::mediation'):
                        errors.append({'kind': 'payload-field-visibility', 'field': field['id']})
                    stores.append(field['id'])
                elif raw & FUNCTIONS or 'function-pointer' in raw:
                    legacy_stores.append(field['id'])
    if observed_carriers!=CARRIERS:
        errors.append({'kind':'carrier-definition-inventory','missing':sorted(CARRIERS-observed_carriers)})
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
    roots = {(name, 0) for name, body in bodies.items() if root_filter(body)}
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
                # The only cut. No other function receives assumed admission.
                continue
            if block.get('canonical_trait') in PROTOCOLS:
                sinks[node] = 'application-protocol'
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
    # Unsupported code outside the reachable kernel graph is inventoried, not
    # silently called safe. An unsupported reachable node rejects the cut.
    unsupported_reachable = sorted(set(unsupported) & set(previous))
    return {'status': 'registered-payload-confinement-checked' if not errors and not bypasses and not unsupported_reachable else 'rejected',
        'errors': errors, 'unguarded_payload_entries': bypasses, 'unsupported_reachable': unsupported_reachable,
        'opaque_payload_fields': stores, 'protocol_references': references, 'kernel_roots': sorted(roots),
        'reachable_blocks_without_admission': len(previous), 'callable_flow_constraints': len(constraints),
        'pre_admission_blocks': sorted(previous),
        'context_scope_contracts': context_scopes,
        'legacy_callable_fields_requiring_construction_proof': sorted(legacy_stores),
        'global_mediation': 'not-proved',
        'scope': 'Compiler inventory of raw Screen/Element storage/references, opaque body visibility/returns, and conservative callable-type forwarding from kernel entries to the single admission cut. Does not establish legacy-hook absence/noninstallation, full receiver taint/erasure, factory/binding data refinement, concurrent authority observations, or all function-pointer origins.'}


def check(profile):
    # Validate source/inventory receipts before consuming any compiler facts.
    m.graph(profile)
    return analyze([json.loads((m.OUT / profile / (crate + '.json')).read_text()) for crate in m.CRATES])


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
