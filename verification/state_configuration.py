#!/usr/bin/env python3
"""Prove every production feature configuration uses a verified state layout."""
import json
from pathlib import Path
import re
import tomllib


class Meta:
    def __init__(self, text):
        self.tokens = re.findall(r'"(?:\\.|[^"\\])*"|::|[^\W\d]\w*|\d+|[^\s]', text)
        self.i = 0

    def take(self, expected=None):
        if self.i >= len(self.tokens):
            raise ValueError('Unexpected end of cfg metadata')
        value = self.tokens[self.i]
        self.i += 1
        if expected is not None and value != expected:
            raise ValueError('Expected ' + expected + ', got ' + value)
        return value

    def at(self, token):
        return self.i < len(self.tokens) and self.tokens[self.i] == token

    def parse(self):
        name = self.take()
        while self.at('::'):
            self.take()
            name += '::' + self.take()
        if self.at('='):
            self.take()
            return name, json.loads(self.take())
        if self.at('('):
            self.take()
            children = []
            while not self.at(')'):
                children.append(self.parse())
                if not self.at(','):
                    break
                self.take()
            self.take(')')
            return name, children
        return name, None


def conjunction(items):
    items = list(dict.fromkeys(items))
    if 'false' in items:
        return 'false'
    items = [x for x in items if x != 'true']
    return '(' + ' && '.join(items) + ')' if items else 'true'


def disjunction(items):
    items = list(dict.fromkeys(items))
    if 'true' in items:
        return 'true'
    items = [x for x in items if x != 'false']
    return '(' + ' || '.join(items) + ')' if items else 'false'


def negative(term):
    return {'true': 'false', 'false': 'true'}.get(term, '!(' + term + ')')


def feature_closures(features):
    result = {}
    for name in features:
        found = {name}
        while True:
            expanded = found | {dep for key in found for dep in features[key] if dep in features}
            if expanded == found:
                break
            found = expanded
        result[name] = found
    return result


def condition(node, closures, names, mask=None):
    name, args = node
    if name in {'all', 'any'}:
        values, pending = [], None
        for child in args:
            try:
                value = condition(child, closures, names, mask)
                if value == ('false' if name == 'all' else 'true'):
                    return value
                values.append(value)
            except ValueError as error:
                pending = error
        if pending is not None:
            raise pending
        return (conjunction if name == 'all' else disjunction)(values)
    if name == 'not' and isinstance(args, list) and len(args) == 1:
        return negative(condition(args[0], closures, names, mask))
    if name == 'feature' and isinstance(args, str):
        indices = [i for i, requested in enumerate(names) if args in closures[requested]]
        if mask is not None:
            return 'true' if any(mask & (1 << i) for i in indices) else 'false'
        return disjunction(['(mask & ' + str(1 << i) + 'u64 != 0)' for i in indices])
    if args is None and name in {'test', 'doctest', 'docsrs', 'doc', 'fairing_loom'}:
        return 'false'
    raise ValueError('Unreviewed production cfg atom: ' + repr(node))


def attribute(node, closures, names, mask=None):
    name, args = node
    if name == 'cfg' and isinstance(args, list) and len(args) == 1:
        return condition(args[0], closures, names, mask)
    if name == 'cfg_attr' and isinstance(args, list) and len(args) >= 2:
        try:
            predicate = condition(args[0], closures, names, mask)
        except ValueError:
            predicate = None
        if predicate == 'false':
            return 'true'
        effects = conjunction([attribute(x, closures, names, mask) for x in args[1:]])
        if effects == 'true':
            return 'true'
        if predicate is None:
            predicate = condition(args[0], closures, names, mask)
        return disjunction([negative(predicate), effects])
    if name in {'derive', 'doc', 'allow', 'deny', 'forbid', 'expect', 'serde',
                'must_use', 'non_exhaustive', 'deprecated'}:
        return 'true'
    if name == 'repr':
        if any(x[0] == 'packed' for x in args):
            raise ValueError('Packed borrowed projection requires separate review')
        return 'true'
    raise ValueError('Unreviewed state attribute: ' + repr(node))


def guard(items, closures, names, mask=None):
    values, pending = [], None
    for item in items:
        try:
            parser = Meta(item)
            value = attribute(parser.parse(), closures, names, mask)
            if parser.i != len(parser.tokens):
                raise ValueError('Unconsumed cfg tokens: ' + item)
            if value == 'false':
                return 'false'
            values.append(value)
        except ValueError as error:
            pending = error
    if pending is not None:
        raise pending
    return conjunction(values)


def node_shape(record):
    definition = record['definition']
    variants = definition.get('variants', [{'name': None, 'fields': definition.get('fields', [])}])
    return tuple((v['name'], tuple(f['name'].removeprefix('r#') for f in v['fields'])) for v in variants)


def selection(items, expected, closures, names, member=None):
    predicates = [(item['name'].removeprefix('r#') if isinstance(item['name'], str) else None,
                   guard(item['guards'], closures, names), item) for item in items]
    by_name = {}
    for name, predicate, item in predicates:
        by_name.setdefault(name, []).append((predicate, item))
    if not set(expected).issubset(by_name):
        raise ValueError('Compiled member is absent from source declaration')
    terms = []
    for name, alternatives in by_name.items():
        if name not in expected:
            terms.append(negative(disjunction([p for p, _ in alternatives])))
        else:
            terms.append(disjunction([conjunction([p, member(item, name) if member else 'true'])
                                      for p, item in alternatives]))
    order = list(expected)
    for i, (a, pa, _) in enumerate(predicates):
        for b, pb, _ in predicates[i + 1:]:
            if a != b and a in expected and b in expected and order.index(a) > order.index(b):
                terms.append(negative(conjunction([pa, pb])))
    return conjunction(terms)


def inspect(root, source_manifest, projections):
    grouped = {}
    for projection in projections:
        grouped.setdefault(projection['source_id'], set()).add(node_shape(projection))
    terms, report = [], {}
    for package in ['fairing', 'fairing-widgets']:
        manifest = tomllib.loads((root / 'crates' / package / 'Cargo.toml').read_text())
        features = manifest.get('features', {})
        names = sorted(features)
        if len(names) > 20:
            raise ValueError('Review feature proof resource bound')
        closures = feature_closures(features)
        prefix = 'crates/' + package + '/src/'
        entries = [e for e in source_manifest
                   if e.get('source', e.get('id', '')).startswith(prefix)]
        edges = [e for e in entries if e['record'] == 'module']
        definitions = [e for e in entries if e['record'] == 'type']
        files = {prefix + 'lib.rs': 'true'}
        for _ in range(len(edges) + 1):
            changed = False
            for edge in edges:
                if edge['source'] not in files:
                    continue
                enabled = conjunction([files[edge['source']], guard(edge['guards'], closures, names)])
                if enabled == 'false':
                    continue
                source = Path(edge['source'])
                if edge['path'] is not None:
                    candidate = source.parent.joinpath(*edge['inline'], edge['path'])
                else:
                    base = source.parent if source.name in {'lib.rs', 'mod.rs'} else source.parent / source.stem
                    base = base.joinpath(*edge['inline'])
                    candidate = base / (edge['name'] + '.rs')
                    if not (root / candidate).is_file():
                        candidate = base / edge['name'] / 'mod.rs'
                if not (root / candidate).is_file():
                    raise ValueError('Unresolved production source module: ' + str(candidate))
                key = candidate.as_posix()
                if key in files and files[key] != enabled:
                    raise ValueError('Multiple source module identities need review: ' + key)
                if key not in files:
                    files[key] = enabled
                    changed = True
            if not changed:
                break
        count = 0
        for definition in definitions:
            source = definition['id'].rsplit('#', 1)[0]
            if source not in files:
                continue
            present = conjunction([files[source], guard(definition['guards'], closures, names)])
            if present == 'false':
                continue
            shapes = []
            for shape in grouped.get(definition['id'], set()):
                expected = dict(shape)
                shapes.append(selection(definition['variants'], expected, closures, names,
                    lambda variant, name: selection(variant['fields'], expected[name], closures, names)))
            terms.append((definition['id'], len(names), disjunction([negative(present), disjunction(shapes)])))
            count += 1
        for entry in entries:
            if entry['record'].startswith('unsupported') and entry['source'] in files:
                active = conjunction([files[entry['source']], guard(entry['guards'], closures, names)])
                terms.append((entry['source'] + '/' + entry['record'], len(names), negative(active)))
        report[package] = {'feature_parameters': names, 'requested_feature_sets': 1 << len(names),
                           'reachable_source_files': len(files), 'source_declarations': count}
    return terms, report


def theorem_source(terms):
    lines = ['use vstd::prelude::*;', 'verus! {']
    for i, (identity, bits, formula) in enumerate(terms):
        lines += ['// ' + identity, 'pub proof fn configuration_coverage_' + str(i) + '(mask:u64)',
                  'requires mask < ' + str(1 << bits) + 'u64,', 'ensures ' + formula + ',', '{}']
    return '\n'.join(lines + ['}', ''])
