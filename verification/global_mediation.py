#!/usr/bin/env python3
"""Compiler certificates for the protected construction and capability invariant."""
from collections import defaultdict
import json

import central_mediation as c

SAFE = 'fairing::shell::builder::protected::CallbackFreeBuilder'
WALLPAPER = 'fairing::shell::builder::protected::CallbackFreeWallpaper'
BUILDER = 'fairing::shell::builder::ShellBuilder'
FACADE = 'fairing::shell::protected::ProtectedShell'
HOST = {'fairing::access::auth::Authenticator', 'fairing::access::AccessPolicy'}
HOST.update('fairing::services::' + name for name in [
    'ClockSource','PowerBackend','WifiBackend','BluetoothBackend','DisplayBackend',
    'AudioBackend','NetworkBackend','InfoBackend','Custom'])


class Types:
    def __init__(self, inventories, trusted_host=True):
        self.trusted_host = trusted_host
        self.definitions = {}
        for inventory in inventories:
            for record in inventory['storage_schema']:
                old = self.definitions.get(record['id'])
                if old is None or record['local']:
                    self.definitions[record['id']] = record

    def substitute(self, shape, arguments):
        if shape['kind'] == 'parameter':
            return arguments.get(shape['index'], shape)
        result=dict(shape)
        for key in ('value',):
            if key in result:result[key]=self.substitute(result[key],arguments)
        if 'values' in result:result['values']=[self.substitute(value,arguments) for value in result['values']]
        if 'arguments' in result:result['arguments']=[{'index':item['index'],'value':self.substitute(item['value'],arguments)} for item in result['arguments']]
        return result

    def violations(self, shape, arguments=None, active=frozenset(), path=(), stops=frozenset()):
        arguments = arguments or {}
        shape = self.substitute(shape, arguments)
        kind = shape['kind']
        if kind == 'scalar': return []
        if kind in ('reference','pointer','sequence'):
            return self.violations(shape['value'],arguments,active,path,stops)
        if kind == 'tuple':
            return [error for item in shape['values'] for error in self.violations(item,arguments,active,path,stops)]
        if kind == 'dynamic':
            return [] if self.trusted_host and set(shape['protocols']) <= HOST else [{'path':path,'kind':'dynamic-input','protocols':shape['protocols']}]
        if kind=='projection' and shape['id']=='alloc::borrow::ToOwned::Owned' and shape['values']==[{'kind':'scalar'}]:
            # Std's ToOwned for primitive scalars/str preserves callback freedom.
            return []
        if kind=='projection' and shape['id']=='core::sync::atomic::AtomicPrimitive::Storage' and shape['values']==[{'kind':'scalar'}]:
            # AtomicPrimitive is sealed by std; scalar atomics contain scalar storage.
            return []
        if kind != 'adt': return [{'path':path,'kind':kind, **({'index':shape['index']} if kind=='parameter' else {})}]
        name = shape['id']
        if name in stops: return []
        local = {item['index']: self.substitute(item['value'],arguments) for item in shape['arguments']}
        key = (name,json.dumps(local,sort_keys=True))
        if key in active: return []
        if len(active) > 150: return [{'path':path,'kind':'unresolved-type-recursion'}]
        definition = self.definitions.get(name)
        if definition is None: return [{'path':path,'kind':'missing-type','type':name}]
        return [error for field in definition['fields'] for error in self.violations(field['shape'],local,active|{key},path+(field['id'],),stops)]


class Values:
    """Unique-definition MIR value provenance; ambiguity is never accepted."""
    def __init__(self, body):
        self.body = body
        self.definitions = defaultdict(list)
        for fact in body['constructions']:
            self.definitions[fact['destination_value']['local']].append(fact)
        self.calls = defaultdict(list)
        for call in body['blocks']:
            if 'destination_value' in call:
                self.calls[call['destination_value']['local']].append(call)

    def operand(self, operand, active=frozenset()):
        if operand['kind'] == 'constant':
            return ('constant',operand['literal'])
        if operand['kind'] != 'place': return ('unknown',)
        value = self.local(operand['local'],active)
        for projection in operand['projections']:
            kind = projection['kind']
            if kind == 'deref':
                if value[0] == 'reference': value = value[1]
            elif kind == 'field':
                if value[0] == 'reference': value = value[1]
                if value[0] == 'aggregate':
                    value = value[3][projection['index']]
                else:
                    value = ('field',value,projection['id'],projection['index'])
            elif kind == 'downcast':
                pass
            else: value = ('projection',kind,value)
        return value

    def local(self, local, active=frozenset()):
        if local in active: return ('cycle',local)
        items = self.definitions[local]
        calls = self.calls[local]
        if 1 <= local <= self.body['argument_count']:
            return ('parameter',local) if not items and not calls else ('overwritten-parameter',local)
        if len(items)+len(calls) != 1: return ('ambiguous',local)
        active = active|{local}
        if calls:
            call = calls[0]
            if (call.get('canonical_callee')=='core::default::Default::default'
                and self.body['locals'][local]['shape'].get('id')=='core::option::Option'):
                return ('aggregate','core::option::Option','None',())
            return ('call', call.get('canonical_callee'), call['callee'], tuple(self.operand(value,active) for value in call['argument_values']))
        fact = items[0]
        values = tuple(self.operand(value,active) for value in fact['operand_facts'])
        if fact['kind'] == 'use' and len(values)==1: return values[0]
        if fact['kind'] == 'reference' and len(values)==1: return ('reference',values[0])
        if fact['kind'] == 'aggregate': return ('aggregate',fact['owner'],fact['variant'],values)
        return ('unknown-expression',)

    def field(self, fact, field):
        index = next(i for i, item in enumerate(fact['fields']) if item['field']==field)
        return self.operand(fact['operand_facts'][index])


def dominates(body, predecessor, target):
    if predecessor==target:return False  # A call terminates its block after assignments.
    pending=[0];seen=set()
    while pending:
        block=pending.pop()
        if block==predecessor or block in seen:continue
        if block==target:return False
        seen.add(block);pending.extend(body['blocks'][block]['successors'])
    return True


def data_construction(inventories, bodies, types):
    errors = types.violations({'kind':'adt','id':SAFE,'arguments':[]}, stops={WALLPAPER})
    wrappers=[]; capabilities=[]; conversions=[]; facade_constructors=[]
    for name, body in bodies.items():
        for fact in body['constructions']:
            if fact['owner']==WALLPAPER: wrappers.append((name,body,fact))
            if fact['owner']==SAFE: capabilities.append((name,body,fact))
            if fact['owner']==FACADE:facade_constructors.append((name,body,fact))
            if fact['owner']==BUILDER and 'CallbackFreeBuilder' in name:
                conversions.append((name,body,fact))
            if any(field.startswith(SAFE+'::') or field.startswith(WALLPAPER+'::') for field in fact['written_fields']):
                errors.append({'kind':'construction-capability-mutation','body':name})
            if fact['kind']=='reference' and body['locals'][fact['destination_value']['local']]['shape'].get('mutable'):
                fields=[(p.get('id') or '') for operand in fact['operand_facts'] for p in operand.get('projections',[])]
                if any(field.startswith(SAFE+'::') or field.startswith(WALLPAPER+'::') for field in fields):
                    errors.append({'kind':'construction-capability-mutable-borrow','body':name})
    for owner in (SAFE,WALLPAPER,FACADE):
        expected=owner.rsplit('::',1)[0]
        for field in types.definitions[owner]['fields']:
            if field['visibility']!=expected:
                errors.append({'kind':'construction-capability-field-visible','field':field['id']})
    if len(wrappers)!=1 or not wrappers[0][0].endswith('CallbackFreeWallpaper::new'):
        errors.append({'kind':'validated-wallpaper-constructor-inventory'})
    if len(capabilities)!=1 or not capabilities[0][0].endswith('CallbackFreeBuilder::from_builder'):
        errors.append({'kind':'callback-free-builder-constructor-inventory'})
    elif len(capabilities)==1:
        _,body,fact=capabilities[0]; values=Values(body)
        for field in fact['fields']:
            short=field['field'].rsplit('::',1)[-1]
            if short=='wallpaper':continue
            original=next((i for i,f in enumerate(types.definitions[BUILDER]['fields']) if f['id'].endswith('::'+short)),None)
            expected=('field',('parameter',1),BUILDER+'::'+short,original)
            actual=values.field(fact,field['field'])
            if actual!=expected:
                errors.append({'kind':'construction-substitutes-input','field':field['field'],'value':actual})
    if len(conversions)!=1:
        errors.append({'kind':'callback-free-builder-conversion-inventory'})
        return errors,{}
    forwarders=[b for b in bodies.values() if b['id']=='shell::builder::protected::CallbackFreeBuilder::build']
    expected_forward=('call','fairing::shell::builder::Impl::build','shell::builder::ShellBuilder::build',
        (('call','fairing::shell::builder::protected::Impl::into_builder',
          'shell::builder::protected::CallbackFreeBuilder::into_builder',(('parameter',1),)),('parameter',2)))
    if len(forwarders)!=1 or Values(forwarders[0]).local(0)!=expected_forward or len([b for b in forwarders[0]['blocks'] if b['kind']=='call'])!=2:
        errors.append({'kind':'callback-free-build-forwarding-contract'})
    name,body,fact=conversions[0]; values=Values(body)
    wallpaper_field=next(f for f in fact['fields'] if f['field']==BUILDER+'::wallpaper')
    wallpaper_value=values.field(fact,wallpaper_field['field'])
    wallpaper_source=('field',('parameter',1),SAFE+'::wallpaper',len(types.definitions[SAFE]['fields'])-1)
    if (wallpaper_value[0]!='call' or wallpaper_value[1]!='core::option::Impl::map'
        or wallpaper_value[3]!=(wallpaper_source,('constant','shell::builder::protected::CallbackFreeWallpaper::into_inner'))):
        errors.append({'kind':'validated-wallpaper-not-preserved','value':wallpaper_value})
    getters=[b for b in bodies.values() if b['id']=='shell::builder::protected::CallbackFreeWallpaper::into_inner']
    if len(getters)!=1 or Values(getters[0]).local(0)!=('field',('parameter',1),WALLPAPER+'::0',0):
        errors.append({'kind':'validated-wallpaper-getter-contract'})
    safe_fields={field['id'].rsplit('::',1)[-1] for field in types.definitions[SAFE]['fields']}
    erased={}
    for field in fact['fields']:
        short=field['field'].rsplit('::',1)[-1]
        actual=values.field(fact,field['field'])
        if short not in safe_fields:
            # Direct optional hooks are None; grouped hooks use a checked all-None
            # default constructor. No runtime callback from the old builder survives.
            if actual == ('aggregate','core::option::Option','None',()):
                erased[short]='None';continue
            if actual[0]=='call' and actual[2].endswith('::default'):
                group_type=next(f['shape']['id'] for f in types.definitions[BUILDER]['fields'] if f['id']==field['field'])
                targets=[b for b in bodies.values() if b['impl_protocol']=='core::default::Default'
                         and b['locals'][0]['shape'].get('id')==group_type and not b['closure']]
                target=targets[0] if len(targets)==1 else None
                if target is not None:
                    constructor=[a for a in target['constructions'] if a['owner']]
                    # All scalar Option fields in the concrete hook group default
                    # to None; inspect fields below using the actual ADT schema.
                    group=[a for a in constructor if a['owner'] in types.definitions and all(
                        'core::ops::function::' in proto
                        for f in types.definitions[a['owner']]['fields'] for proto in f['raw_protocols'])]
                    if len(group)==1:
                        group_values=Values(target)
                        if all(group_values.field(group[0],f['field'])==('aggregate','core::option::Option','None',()) for f in group[0]['fields']):
                            erased[short]='all-None default';continue
            errors.append({'kind':'untyped-hook-not-cleared','field':field['field'],'value':actual})
        elif short!='wallpaper':
            expected=('field',('parameter',1),SAFE+'::'+short,next(i for i,f in enumerate(types.definitions[SAFE]['fields']) if f['id'].endswith('::'+short)))
            if actual!=expected:
                errors.append({'kind':'safe-field-not-preserved','field':field['field'],'value':actual,'expected':expected})
    # The protected constructor accepts the capability, never a mutable legacy shell.
    facades=[b for b in bodies.values() if b['id']=='shell::protected::ProtectedShell::new']
    if len(facades)!=1 or facades[0]['locals'][1]['shape'].get('id')!=SAFE:
        errors.append({'kind':'protected-constructor-requires-legacy-state'})
    if len(facade_constructors)!=1 or not facade_constructors[0][0].endswith('ProtectedShell::new'):
        errors.append({'kind':'protected-facade-constructor-inventory'})
    else:
        _,body,fact=facade_constructors[0];values=Values(body)
        inner=values.field(fact,FACADE+'::inner')
        built=('call','fairing::shell::builder::protected::Impl::build',
               'shell::builder::protected::CallbackFreeBuilder::build',(('parameter',1),('parameter',2)))
        expected=('field',('call','core::ops::try_trait::Try::branch','std::ops::Try::branch',(built,)),
                  'core::ops::control_flow::ControlFlow::Continue::0',0)
        if inner!=expected:errors.append({'kind':'protected-facade-substitutes-shell','value':inner})
        shell_fields=types.definitions['fairing::shell::Shell']['fields']
        for method,field in [('access::Access::require_mediation','access'),
                             ('screen::registry::Registry::bind_initial_library_requirements','registry')]:
            calls=[k for k in body['blocks'] if k.get('callee')==method]
            index=next(i for i,f in enumerate(shell_fields) if f['id'].endswith('::'+field))
            expected_arg=('reference',('field',inner,'fairing::shell::Shell::'+field,index))
            if len(calls)!=1 or [values.operand(a) for a in calls[0]['argument_values']]!=[expected_arg] or not dominates(body,calls[0]['block'],fact['block']):
                errors.append({'kind':'protected-initialization-not-dominating','method':method})
    mode_field='fairing::access::Access::mediation_required'
    writes=[(name,fact)for name,body in bodies.items() for fact in body['constructions'] if mode_field in fact['written_fields']]
    if len(writes)!=1 or not writes[0][0].endswith('Access::require_mediation') or writes[0][1]['values']!=['const true']:
        errors.append({'kind':'mediation-mode-not-monotone'})
    for name,body in bodies.items():
        for fact in body['constructions']:
            if fact['kind']!='reference' or not body['locals'][fact['destination_value']['local']]['shape'].get('mutable'):continue
            if any(p.get('id')==mode_field for operand in fact['operand_facts'] for p in operand.get('projections',[])):
                errors.append({'kind':'mutable-mediation-mode-export','body':name})
    return errors,{'data_fields':sorted(safe_fields),'cleared_hooks':erased,'conversion':name}


def check_data(inventories):
    bodies={item['crate']+'::'+body['id']:body for item in inventories for body in item['bodies']}
    types=Types(inventories)
    return data_construction(inventories,bodies,types)


def unreference(value):
    return value[1] if value[0]=='reference' else value


def normalized_borrow(value):
    if value[0]=='call' and value[1]=='core::option::Impl::as_deref_mut' and len(value[3])==1:
        return unreference(value[3][0])
    return unreference(value)


def default_constructor(bodies, name):
    matches=[body for body in bodies.values() if not body['closure']
        and body['impl_protocol']=='core::default::Default'
        and body['locals'][0]['shape'].get('id')==name]
    return matches[0] if len(matches)==1 else None


def callback_empty_constructor(bodies, types, name):
    body=default_constructor(bodies,name)
    if body is None:return False
    constructors=[b for b in bodies.values() if not b['closure'] and b['id'].endswith('::new')
                  and b['locals'][0]['shape'].get('id')==name]
    if len(constructors)!=1:return False
    new=constructors[0]
    if (Values(new).local(0)!=('call','core::default::Default::default','std::default::Default::default',())
        or len([block for block in new['blocks'] if block['kind']=='call'])!=1):return False
    constructors=[fact for fact in body['constructions'] if fact['owner']==name]
    if len(constructors)!=1:return False
    values=Values(body); fact=constructors[0]
    for field in types.definitions[name]['fields']:
        problems=types.violations(field['shape'])
        if not problems:continue
        actual=values.field(fact,field['id'])
        if actual==('aggregate','core::option::Option','None',()):continue
        if actual[0]=='call' and actual[1]=='core::default::Default::default':
            shape=field['shape']
            if shape.get('id') in ('alloc::vec::Vec','alloc::collections::btree::map::BTreeMap'):
                continue
        return False
    return True


def context_isolation(bodies,types):
    errors=[];bindings=[]
    cx='fairing::screen::cx::Cx'
    for name,body in bodies.items():
        for fact in body['constructions']:
            if fact['owner']==cx and body['id'].startswith('screen::cx::CxParts'):
                values=Values(body)
                parts=types.definitions['fairing::screen::cx::CxParts']['fields']
                part=lambda short:('field',('parameter',1),'fairing::screen::cx::CxParts::'+short,
                                  next(i for i,f in enumerate(parts) if f['id'].endswith('::'+short)))
                for field in types.definitions[cx]['fields']:
                    short=field['id'].rsplit('::',1)[-1]
                    if short=='authority_path':continue
                    actual=normalized_borrow(values.field(fact,field['id']))
                    if short in ('pane','event','registry'):
                        expected=('parameter',{'pane':2,'event':3,'registry':4}[short])
                    elif short=='session':
                        expected=('call','fairing::access::Impl::session','access::Access::session',(part('access'),))
                    else:expected=part(short)
                    if actual!=expected:
                        errors.append({'kind':'root-context-substitutes-owner','field':field['id'],'value':actual})
            if fact['owner']==cx and body['id'].endswith('::with_protected_resources'):
                values=Values(body)
                for field in types.definitions[cx]['fields']:
                    short=field['id'].rsplit('::',1)[-1]
                    actual=values.field(fact,field['id'])
                    if short=='authority_path':continue  # Independently checked by c.context_contracts.
                    if short in ('icons','widget_painters'):
                        resource='fairing_widgets::icons::IconSet' if short=='icons' else 'fairing_widgets::widgets::painters::WidgetPainters'
                        expected='fairing_widgets::icons::IconSet::new' if short=='icons' else 'fairing_widgets::widgets::WidgetPainters::new'
                        if (actual[0]!='reference' or actual[1][0]!='call' or actual[1][2]!=expected or actual[1][3]
                            or not callback_empty_constructor(bodies,types,resource)):
                            errors.append({'kind':'scoped-resource-aliases-global','field':field['id'],'value':actual})
                    else:
                        expected=('field',('parameter',1),field['id'],types.definitions[cx]['fields'].index(field))
                        if normalized_borrow(actual)!=expected:
                            errors.append({'kind':'scoped-context-substitutes-owner','field':field['id'],'value':actual})
                bindings.append(name)
            if fact['owner']=='fairing::screen::cx::CxParts':
                values=Values(body)
                for short,source in [('access','access'),('shell','handle'),('icons','icons'),('widget_painters','widget_painters')]:
                    field=next(f for f in fact['fields'] if f['field'].endswith('::'+short))
                    actual=normalized_borrow(values.field(fact,field['field']))
                    expected_field='fairing::shell::Shell::'+source
                    if actual[:3]!=('field',('parameter',1),expected_field):
                        errors.append({'kind':'root-context-owner-mismatch','body':name,'field':short,'value':actual})
    if len(bindings)!=1:errors.append({'kind':'scoped-context-inventory'})
    return errors,{'scoped_constructor':bindings,'resource_policy':'fresh callback-empty owned stores; all other fields borrow the original context owner'}


def observer_bindings(bodies,types):
    errors=[];checked=[]
    cx='fairing::screen::cx::Cx';access='fairing::access::Access'
    index=next(i for i,f in enumerate(types.definitions[cx]['fields']) if f['id']==cx+'::access')
    owner=('field',('parameter',1),cx+'::access',index)
    for method,target in [('allows_gate','allows'),('authority_stamp','authority_stamp'),
                          ('requires_mediation','requires_mediation'),('token_mode','uses_session_tokens')]:
        matches=[b for b in bodies.values() if b['id'].startswith('screen::cx::Cx::') and b['id'].endswith('::'+method)]
        arguments=(owner,('parameter',2)) if method=='allows_gate' else (owner,)
        expected=('call','fairing::access::Impl::'+target,'access::Access::'+target,arguments)
        if len(matches)!=1 or Values(matches[0]).local(0)!=expected or len([k for k in matches[0]['blocks'] if k['kind']=='call'])!=1:
            errors.append({'kind':'context-observer-binding','method':method})
        else:checked.append(method)
    for method,field,reference in [('requires_mediation','mediation_required',False),
                                   ('uses_session_tokens','token_sessions',False),('session','session',True)]:
        matches=[b for b in bodies.values() if b['id']=='access::Access::'+method]
        index=next(i for i,f in enumerate(types.definitions[access]['fields']) if f['id']==access+'::'+field)
        expected=('field',('parameter',1),access+'::'+field,index)
        if reference:expected=('reference',expected)
        if len(matches)!=1 or Values(matches[0]).local(0)!=expected:
            errors.append({'kind':'authority-boolean-observer-binding','method':method})
        else:checked.append('Access::'+method)
    return errors,checked


def wallpaper_shape(types):
    errors=[];fields=types.definitions['fairing::desktop::Wallpaper']['fields']
    for field in fields:
        variant=field['id'].removeprefix('fairing::desktop::Wallpaper::').split('::')[0]
        if variant in ('Painter','ThemedPainter'):
            if not set(field['raw_protocols'])&c.FUNCTIONS:errors.append({'kind':'wallpaper-callback-shape','field':field['id']})
        elif variant=='Themed':
            shape=field['shape']
            if (shape.get('id')!='alloc::boxed::Box' or shape['arguments'][0]['value'].get('id')!='fairing::desktop::Wallpaper'):
                errors.append({'kind':'wallpaper-recursion-shape','field':field['id']})
        else:
            errors.extend(types.violations(field['shape'],path=(field['id'],)))
    return errors


def payload_parameters(bodies):
    """Follow payload type parameters through private helpers and captures.

    Tracking only functions with a carrier argument would miss a helper accepting
    Box<T>, or a closure capturing it after the outer carrier was destructured.
    """
    taint={name:set(body['input_payload_parameters']) for name,body in bodies.items()}
    changed=True
    while changed:
        changed=False
        for name,body in bodies.items():
            if body['closure'] and body['lexical_parent'] in bodies:
                before=len(taint[name]);taint[name].update(taint[body['lexical_parent']]);changed|=before!=len(taint[name])
            for block in body['blocks']:
                if block['kind']!='call':continue
                callee=body.get('_crate',name.split('::',1)[0])+'::'+block['callee'] if block['local_callee'] else block['callee']
                if callee not in bodies:continue
                for argument in block.get('generic_objects',[]):
                    if any(obj['kind']=='parameter' and obj['index'] in taint[name] for obj in argument['objects']):
                        before=len(taint[callee]);taint[callee].add(argument['index']);changed|=before!=len(taint[callee])
    return taint


def kernel_exports(bodies,types):
    errors=[]
    taint=payload_parameters(bodies)
    metadata=c.CARRIERS|{c.KERNEL+'protected::PermissionBinding'}
    for name,body in bodies.items():
        if not c.kernel_entry(body):continue
        shape=body['locals'][0]['shape']
        problems=types.violations(shape,stops=metadata)
        for problem in problems:
            if problem['kind']=='parameter' and problem['index'] not in taint[name]:continue
            errors.append({'kind':'executable-kernel-export','body':name,'detail':problem})
    return errors


def capability_check(inventories):
    bodies={item['crate']+'::'+body['id']:body for item in inventories for body in item['bodies']}
    types=Types(inventories)
    errors,construction=data_construction(inventories,bodies,types)
    for owner in types.definitions.values():
        if not owner['local'] or not c.kernel(owner['id']) or owner['id'] in c.CARRIERS|{c.KERNEL+'protected::PermissionBinding'}:continue
        for field in owner['fields']:
            for problem in types.violations(field['shape'],stops=c.CARRIERS|{c.KERNEL+'protected::PermissionBinding'}):
                errors.append({'kind':'unclassified-executable-kernel-storage','field':field['id'],'detail':problem})
    errors.extend(wallpaper_shape(types))
    isolated,context=context_isolation(bodies,types);errors.extend(isolated)
    observers,bound=observer_bindings(bodies,types);errors.extend(observers)
    errors.extend(kernel_exports(bodies,Types(inventories,trusted_host=False)))
    return {'status':'capability-invariant-checked' if not errors else 'rejected','errors':errors,
            'construction':construction,'context_isolation':context,'observer_bindings':bound}


def operand_shape(body,operand,types):
    # The compiler resolves tuple projections, closure captures, autodereferences
    # and associated types. Missing evidence rejects; Python never guesses a type.
    return operand['shape']


def payload_boundaries(inventories,confinement):
    """No void-returning export of executable payloads to an unguarded setter.

    Ordinary execution reachability alone misses a callback handed to an installer
    for later execution. Check actual argument shapes as well as call targets.
    Standard ownership-only containers are explicit contracts, never a general
    exemption for external code or a user Clone implementation.
    """
    types=Types(inventories,trusted_host=False)
    bodies={d['crate']+'::'+b['id']:b for d in inventories for b in d['bodies']}
    taint=payload_parameters(bodies)
    stops=c.CARRIERS|{c.KERNEL+'protected::PermissionBinding','fairing::screen::cx::Cx',c.KERNEL+'AuthorityPath'}
    errors=[];transfers=[]
    def executable(body,shape):
        return [p for p in types.violations(shape,stops=stops) if
            (p['kind']=='dynamic-input' and set(p['protocols'])&(c.FUNCTIONS|c.PROTOCOLS|{'function-pointer'}))
            or (p['kind']=='parameter' and p['index'] in taint[body['_identity']])]
    for name,body in bodies.items():body['_identity']=name
    reached=set(map(tuple,confinement['pre_admission_blocks']))
    for name,body in bodies.items():
        for fact in body['constructions']:
            if (name,fact['block']) not in reached:continue
            sources=[p for operand in fact['operand_facts'] for p in executable(body,operand_shape(body,operand,types))]
            if not sources:continue
            destination=fact['destination_value']
            if any(p['kind']=='deref' for p in destination['projections']):
                errors.append({'kind':'executable-payload-write-through-borrow','body':name,'block':fact['block'],'destination':destination})
            elif fact['kind']=='other' and not executable(body,operand_shape(body,destination,types)):
                errors.append({'kind':'executable-payload-erasure','body':name,'block':fact['block'],'destination':destination})
    for name,index in confinement['pre_admission_blocks']:
        body=bodies[name];block=body['blocks'][index]
        if block['kind']!='call':continue
        target=block.get('canonical_callee') or ''
        if target==c.m.GUARD or c.kernel(target):continue
        for operand in block['argument_values']:
            shape=operand_shape(body,operand,types)
            payload=executable(body,shape)
            if not payload:continue
            unwrapped=shape['value'] if shape['kind']=='reference' else shape
            safe=(target=='core::cell::Impl::new' and unwrapped.get('id')=='alloc::boxed::Box'
                  or target=='alloc::rc::Impl::new' and unwrapped.get('id')=='core::cell::RefCell'
                  or target=='core::clone::Clone::clone' and unwrapped.get('id')=='alloc::rc::Rc')
            if safe:transfers.append({'body':name,'block':index,'contract':target,'shape':unwrapped['id']})
            else:errors.append({'kind':'executable-payload-export','body':name,'block':index,'target':target,'detail':payload})
    return errors,transfers


def fresh_receipt(path,status):
    """Never accept a receipt because its status string alone says verified."""
    record=json.loads(path.read_text())
    assert record['status']==status,(path,record['status'])
    assert record['inputs'] and all((c.m.ROOT/name).is_file() and c.m.digest(c.m.ROOT/name)==value for name,value in record['inputs'].items()),'Stale proof receipt: '+str(path)
    return {'path':str(path.relative_to(c.m.ROOT)),'sha256':c.m.digest(path),'status':status}


def check(profile):
    c.m.graph(profile)
    inventories=[json.loads((c.m.OUT/profile/(crate+'.json')).read_text()) for crate in c.m.CRATES]
    confinement=c.analyze(inventories)
    capability=capability_check(inventories)
    exports,transfers=payload_boundaries(inventories,confinement)
    protected=c.analyze(inventories,lambda b:b['public'] and b['id'].startswith('shell::protected::ProtectedShell::'))
    proofs=[]
    directory=c.m.ROOT/'target/assurance/central-mediation'
    for path,status in [('kernel-proof.json','shared-production-admission-kernel-verified'),
                        ('path-proof.json','unbounded-production-path-verified'),
                        ('construction/receipt.json','unbounded-production-construction-verified'),
                        ('carriers/receipt.json','production-factory-carrier-verified')]:
        proofs.append(fresh_receipt(directory/path,status))
    checked=(confinement['status']=='registered-payload-confinement-checked'
             and capability['status']=='capability-invariant-checked' and not exports)
    obligations=[
        {'property':'registered-payload ownership and necessary admission cut','discharged':confinement['status']=='registered-payload-confinement-checked'},
        {'property':'callback-free protected construction, including arbitrary finite wallpaper nesting','discharged':capability['status']=='capability-invariant-checked'},
        {'property':'original authority owner and invocation-local callback stores','discharged':not capability['errors']},
        {'property':'no executable payload return or pre-admission forwarding export','discharged':not exports and not capability['errors']},
        {'property':'factory requirement preservation and post-construction live admission','discharged':True},
        {'property':'every entry in an arbitrary finite inherited authority path','discharged':True},
        {'property':'protected-root legacy-hook noninstallation and complete receiver provenance','discharged':False,
         'reason':'Construction and scoped stores are connected, but the context-insensitive root graph still contains unresolved legacy receivers and cross-caller generic forwarding. Their absence is not inferred from successful construction.',
         'candidate_entries':len(protected['unguarded_payload_entries'])},
        {'property':'production authority observers refine the stable-interval path contract','discharged':False,
         'reason':'The production SC publication/read sequence and mutable narrowing-policy observations are not yet refined to the path proof observer contract. Existing component/SC lemmas do not discharge this correspondence.'},
    ]
    return {'status':'connections-checked' if checked else 'rejected',
            'complete_mediation':all(item['discharged'] for item in obligations),
            'obligations':obligations,'proofs':proofs,'capabilities':capability,
            'confinement':confinement,'payload_export_errors':exports,
            'standard_ownership_transfers':transfers,
            'protected_root_candidates':protected['unguarded_payload_entries'],
            'scope':'Compiler-bound construction, resource ownership, deep exports, opaque carrier admission, factory refinement and unbounded inherited requirements. Remaining obligations stay explicit and keep the complete-mediation gate nonzero.'}


def run_proofs():
    import subprocess
    import sys
    for script in ('central_proofs.py','verus/construction.py','carrier_proofs.py'):
        subprocess.run([sys.executable,str(c.m.ROOT/'verification'/script)],cwd=c.m.ROOT,check=True)


def main():
    import argparse
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-build',action='store_true')
    parser.add_argument('--prove',action='store_true',help='Regenerate all connected proof receipts and semantic controls')
    args=parser.parse_args();before=c.m.source_hashes()
    output=c.m.ROOT/'target/assurance/central-mediation/connections.json'
    output.parent.mkdir(parents=True,exist_ok=True);output.unlink(missing_ok=True)
    if args.prove:run_proofs()
    reports={}
    for profile in ('all','minimal'):
        if not args.skip_build:c.m.build(profile)
        reports[profile]=check(profile)
    assert before==c.m.source_hashes(),'Connection inputs changed while checking'
    output.write_text(json.dumps({'inputs':before,'profiles':reports},indent=2)+'\n')
    print(json.dumps({p:{k:v[k] for k in ('status','complete_mediation','obligations')}for p,v in reports.items()},indent=2))
    raise SystemExit(0 if all(r['status']=='connections-checked' for r in reports.values()) else 1)

if __name__=='__main__':main()
