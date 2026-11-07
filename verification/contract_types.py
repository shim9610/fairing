"""Compiler-derived type/value checks shared by every declared contract."""
from collections import defaultdict
import json
import central_mediation as c
HOST=set()

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
            return self.violations(shape['value'],{},active,path,stops)
        if kind == 'tuple':
            return [error for item in shape['values'] for error in self.violations(item,{},active,path,stops)]
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
        # The outer substitution is complete. Residual parameters belong to the
        # caller, not to this ADT's binder, even when their numeric indices match.
        local = {item['index']: item['value'] for item in shape['arguments']}
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
        return [p for p in types.violations(shape,stops=stops) if p['kind']=='callable' or
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
