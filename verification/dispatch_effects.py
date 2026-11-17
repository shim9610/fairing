"""Interprocedural fresh-admission effects, including matching call continuations.

Two abstract states suffice: fresh authority, or consumed authority. A payload
entry consumes it; a mediated invocation checks its operation from fresh state
and returns consumed. Function summaries preserve both states and reachable
violations, even on nonreturning paths. Fixed points include recursive helpers.
"""
from collections import defaultdict, deque


def check(bodies, calls, sinks, guards, bridge_roots, clobbers=frozenset()):
    owners = defaultdict(set)
    callers = defaultdict(set)
    eventful = set()
    for name, body in bodies.items():
        for block in body['blocks']:
            node = (name, block['block'])
            for target, _ in calls.get(node, set()) | guards.get(node, set()):
                owners[name].add(target)
                callers[target].add(name)
            if node in sinks or node in guards or node in clobbers or block['kind'] == 'unsupported':
                eventful.add(name)
    queue = deque(eventful)
    while queue:
        for caller in callers[queue.popleft()]:
            if caller not in eventful:
                eventful.add(caller)
                queue.append(caller)
    summaries = {(name, fresh): (frozenset(), False) for name in eventful for fresh in (False, True)}

    def summary(name, fresh):
        return summaries.get((name, fresh), (frozenset([fresh]), False))

    def evaluate(name, initial):
        body = bodies[name]
        pending = [(0, initial)]
        seen = set()
        exits = set()
        violation = False
        while pending:
            index, fresh = pending.pop()
            if (index, fresh) in seen:
                continue
            seen.add((index, fresh))
            block = body['blocks'][index]
            node = (name, index)
            if block['kind'] == 'unsupported':
                violation = True
            if block['kind'] == 'return':
                exits.add(fresh)
                continue
            states = set()
            if node in guards:
                # A nested attempt may run arbitrary admitted code. Its return
                # never restores an older caller authorization, even if denied.
                states.add(False)
                if not guards[node]:
                    violation = True
                for target, _ in guards[node]:
                    _, error = summary(target, True)
                    violation |= error
            elif node in clobbers:
                states.add(False)
            else:
                if node in sinks:
                    violation |= not fresh
                    states.add(False)
                for target, _ in calls.get(node, ()):
                    result, error = summary(target, fresh)
                    states.update(result)
                    violation |= error
                if node not in sinks and not calls.get(node):
                    states.add(fresh)
            for state in states:
                pending.extend((target, state) for target in block['successors'])
        return frozenset(exits), violation

    pending = deque(eventful)
    queued = set(eventful)
    while pending:
        name = pending.popleft()
        queued.remove(name)
        changed = False
        for fresh in (False, True):
            old = summaries[name, fresh]
            new = evaluate(name, fresh)
            # Monotone union retains nonterminating-path violations too.
            merged = (old[0] | new[0], old[1] or new[1])
            if merged != old:
                summaries[name, fresh] = merged
                changed = True
        if changed:
            for caller in callers[name]:
                if caller not in queued:
                    queued.add(caller)
                    pending.append(caller)
    return [{'kind': 'callback-without-fresh-admission', 'body': name}
            for name in sorted(bridge_roots) if summary(name, True)[1]]
