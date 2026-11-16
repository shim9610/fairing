"""Connect the proved layout verdict to fail-closed compiled plan entry guards."""
import re


def check_plan_entries(bodies):
    from contract_types import Values, normalized_borrow
    errors=[]
    checked=[]
    for method,context in [('validate',2),('paint',3)]:
        matches=[body for body in bodies.values()
                 if body['canonical_id']=='fairing::elements::dispatch::Impl::'+method
                 and body['impl_owner']=='fairing::elements::Plan']
        if len(matches)!=1:
            errors.append({'kind':'layout-guard-inventory','method':method})
            continue
        body=matches[0];blocks={block['block']:block for block in body['blocks']}
        values=Values(body);entry=blocks[0]
        if any(fact['block']==0 and fact['destination_value']['projections']
               for fact in body['constructions']):
            errors.append({'kind':'layout-entry-mutation','method':method})
            continue
        valid=entry.get('canonical_callee')=='fairing::elements::layout_token::Impl::valid'
        arguments=entry.get('argument_values',[])
        if valid and len(arguments)==2:
            receiver=normalized_borrow(values.operand(arguments[0]))
            cx=normalized_borrow(values.operand(arguments[1]))
            valid=(receiver[:3]==('field',('parameter',1),'fairing::elements::Plan::layout')
                   and cx==('parameter',context))
        if not valid or len(entry.get('successors',[]))!=1:
            errors.append({'kind':'layout-entry-guard','method':method})
            continue
        switch=blocks[entry['successors'][0]]
        local=entry['destination_value']['local']
        if (switch['kind']!='switch' or not re.fullmatch(r'(?:copy|move) _'+str(local),switch.get('discriminant',''))
                or switch.get('statements') or len(switch.get('branches',[]))!=1
                or switch['branches'][0]['value']!='0'):
            errors.append({'kind':'layout-verdict-not-used','method':method})
            continue
        pending=[(switch['branches'][0]['target'],None,frozenset())]
        rejected=True;returned=False
        while pending:
            index,result,seen=pending.pop()
            if index in seen:
                rejected=False;break
            block=blocks[index]
            if block['kind'] not in ('goto','return'):
                rejected=False;break
            for fact in body['constructions']:
                if fact['block']!=index:continue
                # Refusal performs no further calls or non-result writes.
                if (fact['destination_value']['local']!=0 or fact['destination_value']['projections']
                        or fact['kind']!='use' or len(fact['operand_facts'])!=1):
                    rejected=False;break
                result=values.operand(fact['operand_facts'][0])==('constant','const false')
            if not rejected:break
            if block['kind']=='return':
                returned=True
                if result is not True:rejected=False;break
            pending.extend((target,result,seen|{index}) for target in block['successors'])
        if not rejected or not returned:
            errors.append({'kind':'layout-refusal-not-closed','method':method})
        else:
            checked.append(body['id'])
    return errors,checked


def check_root(bodies):
    from contract_types import Values, normalized_borrow
    matches=[body for body in bodies.values() if body['impl_owner']=='fairing::elements::ElementTree'
             and body['canonical_id']=='fairing::elements::Impl::ui'
             and body['impl_protocol']=='fairing::screen::Screen']
    if len(matches)!=1:return [{'kind':'layout-root-inventory'}],[]
    body=matches[0];values=Values(body);blocks={b['block']:b for b in body['blocks']}
    guards=[b for b in body['blocks'] if b.get('canonical_callee')=='fairing::elements::dispatch::Impl::validate']
    if len(guards)!=1:return [{'kind':'layout-root-guard'}],[]
    guard=guards[0];args=[normalized_borrow(values.operand(v)) for v in guard['argument_values']]
    if len(args)!=3 or args[1]!=('parameter',3):return [{'kind':'layout-root-context'}],[]
    plan,_,stamp=args
    def resolved(value):
        if not isinstance(value,tuple):return True
        if value and isinstance(value[0],str) and value[0] in ('ambiguous','cycle','unknown','unknown-expression','overwritten-parameter'):
            return False
        return all(resolved(v) for v in value)
    if not resolved(plan) or not resolved(stamp):return [{'kind':'layout-root-plan-binding'}],[]
    successors=[b for b in guard['successors'] if not blocks[b].get('cleanup',False)]
    if len(successors)!=1:return [{'kind':'layout-root-verdict'}],[]
    switch=blocks[successors[0]];local=guard['destination_value']['local']
    if (switch['kind']!='switch' or switch.get('statements')
            or not re.fullmatch(r'(?:copy|move) _'+str(local),switch.get('discriminant',''))
            or len(switch.get('branches',[]))!=1 or switch['branches'][0]['value']!='0'):
        return [{'kind':'layout-root-verdict'}],[]
    if switch['branches'][0]['target']==switch['otherwise']:
        return [{'kind':'layout-root-guard'}],[]
    cut=(switch['block'],switch['otherwise'])
    allowed_calls={
        'fairing::screen::cx::Impl::authority_stamp', 'egui::ui::Impl::available_size',
        'fairing::elements::dispatch::Impl::prepare', 'fairing::elements::dispatch::Impl::validate',
        'egui::sense::Impl::hover', 'egui::ui::Impl::ctx', 'egui::context::Impl::request_repaint',
        'egui::ui::Impl::allocate_exact_size', 'egui::ui::Impl::scope_id',
        'egui::id::Impl::with', 'fairing::elements::dispatch::Impl::paint',
    }
    if any(block['kind']=='call' and block.get('canonical_callee') not in allowed_calls for block in body['blocks']):
        return [{'kind':'layout-root-unclassified-call'}],[]
    if any(fact['destination_value']['projections'] for fact in body['constructions']):
        return [{'kind':'layout-root-write-through'}],[]
    sinks=[];allocation=0;paint=0
    for block in body['blocks']:
        callee=block.get('canonical_callee')
        if callee not in ('egui::ui::Impl::allocate_exact_size','fairing::elements::dispatch::Impl::paint'):continue
        actual=[normalized_borrow(values.operand(v)) for v in block['argument_values']]
        if callee=='egui::ui::Impl::allocate_exact_size':
            allocation+=1
            if (len(actual)!=3 or actual[0]!=('parameter',2)
                    or actual[1][:3]!=('field',plan,'fairing::elements::Plan::size')):
                return [{'kind':'layout-root-plan-binding'}],[]
        else:
            paint+=1
            if (len(actual)!=6 or actual[:3]!=[plan,('parameter',2),('parameter',3)] or actual[4]!=stamp):
                return [{'kind':'layout-root-plan-binding'}],[]
        sinks.append(block['block'])
    if allocation!=1 or paint!=1:return [{'kind':'layout-root-use-inventory'}],[]
    def reaches(starts, targets):
        pending=list(starts);seen=set()
        while pending:
            index=pending.pop()
            if index in targets:return True
            if index in seen:continue
            seen.add(index)
            pending.extend(target for target in blocks[index]['successors'] if (index,target)!=cut)
        return False
    # Every data-use path must cross the true verdict edge. Reusing a use-site
    # through a cycle also requires a fresh validation, rather than an older cut.
    if reaches([0],set(sinks)) or any(reaches(blocks[sink]['successors'],{sink}) for sink in sinks):
        return [{'kind':'layout-root-guard'}],[]
    return [],[body['id']]


def check(bodies):
    errors,checked=check_plan_entries(bodies)
    root_errors,root_checked=check_root(bodies)
    return errors+root_errors,checked+root_checked
