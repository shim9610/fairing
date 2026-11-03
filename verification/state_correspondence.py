#!/usr/bin/env python3
"""Source-bound lossless concrete state representation; no operation/progress claim."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tomllib

import install
import state_configuration
import state_schema

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/state-correspondence'
PROBE = OUT / 'probe'
TOOLS = ROOT / 'verification/state-projection'
TARGET = ROOT / 'target/state-projection-tools'
PACKAGES = ['fairing', 'fairing-widgets']
COMMANDS = []


def fingerprints():
    paths = set((ROOT / 'crates').rglob('*.rs')) | set((ROOT / 'crates').rglob('Cargo.toml'))
    paths |= {ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
        'verification/state-schema.json', 'verification/state_schema.py', 'verification/tools.json',
        'verification/install.py', 'tools/ferrocene/coverage-tool.json',
        'verification/state_correspondence.py', 'verification/state_configuration.py',
        'verification/state_projection_rustdoc.py', 'verification/verus/state_representation.rs']}
    paths |= {p for p in TOOLS.rglob('*') if p.is_file()}
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(paths)}


def check_tool_lock():
    primary = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
    approved = {(p['name'], p['version'], p.get('source'), p.get('checksum')) for p in primary}
    for package in tomllib.loads((TOOLS / 'Cargo.lock').read_text())['package']:
        if 'source' not in package and package['name'] == 'fairing-state-projection':
            continue
        key = (package['name'], package['version'], package.get('source'), package.get('checksum'))
        if key not in approved:
            raise SystemExit('Projection tool dependency is not in the reviewed product lock: ' + str(key))


def run(name, command, cwd=ROOT, env=None, rejection=False, verified=None, error_marker=None):
    log = OUT / (name + '.log')
    with log.open('w') as output:
        result = subprocess.run([str(x) for x in command], cwd=cwd, env=env,
                                stdout=output, stderr=subprocess.STDOUT)
    text = log.read_text()
    accepted = result.returncode == 0
    if rejection:
        accepted = (result.returncode != 0
                    and re.search(r'verification results:: \d+ verified, [1-9]\d* errors', text) is not None
                    and ('postcondition not satisfied' in text or 'assertion failed' in text)
                    and not re.search(r'resource limit|timed out|timeout', text, re.I))
    if error_marker is not None:
        accepted = result.returncode != 0 and error_marker in text
    if verified is not None:
        accepted = accepted and ('verification results:: ' + str(verified) + ' verified, 0 errors') in text
    COMMANDS.append({'name': name, 'exit_code': result.returncode, 'negative_control': rejection,
                     'log': str(log.relative_to(ROOT))})
    if not accepted:
        raise SystemExit(name + ' did not establish the required result:\n' + text[-8000:])
    print(name + ': ' + ('rejected as required' if rejection or error_marker else 'passed'), flush=True)


def prepare():
    available = subprocess.check_output(['rustup', 'toolchain', 'list'], text=True)
    for channel in [state_schema.NIGHTLY, install.PINS['verus_toolchain']]:
        if not any(line.startswith(channel + '-') or line.startswith(channel + ' ') for line in available.splitlines()):
            run('install-' + channel, ['rustup', 'toolchain', 'install', channel, '--profile', 'minimal'])
    run('projection-tool', ['cargo', 'build', '--locked', '--manifest-path', TOOLS / 'Cargo.toml',
                            '--target-dir', TARGET])
    if PROBE.exists():
        shutil.rmtree(PROBE)
    PROBE.mkdir(parents=True)
    tracked = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).split(b'\0')
    paths = {ROOT / os.fsdecode(p) for p in tracked if p}
    paths |= {p for p in (ROOT / 'crates').rglob('*') if p.is_file()}
    for source in sorted(paths):
        if source.is_file():
            destination = PROBE / source.relative_to(ROOT)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
    manifest = []
    for package in PACKAGES:
        crate = PROBE / 'crates' / package
        with (crate / 'Cargo.toml').open('a') as output:
            output.write('\n[dependencies.fairing-state-projection]\npath = ' + json.dumps(str(TOOLS)) + '\n')
        for source in sorted((crate / 'src').rglob('*.rs')):
            original = str(source.relative_to(PROBE))
            text = subprocess.check_output([str(TARGET / 'debug/instrument'), str(source), original], text=True)
            manifest += [json.loads(line) for line in text.splitlines() if line]
    (OUT / 'source-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    wrapper = OUT / 'rustdoc-wrapper'
    wrapper.write_text('#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' '
                       + shlex.quote(str(ROOT / 'verification/state_projection_rustdoc.py')) + ' "$@"\n')
    wrapper.chmod(0o755)


def build(profile, mutation=None):
    destination = OUT / profile
    destination.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(ROOT / 'target/state-schema'),
               RUSTDOC=str(OUT / 'rustdoc-wrapper'), FAIRING_STATE_EXPANDED=str(destination))
    for variable in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'FAIRING_STATE_PROJECTION_MUTATION']:
        env.pop(variable, None)
    if mutation:
        env['FAIRING_STATE_PROJECTION_MUTATION'] = mutation
    for name in ['RUSTDOC', 'RUSTC']:
        env['FAIRING_STATE_' + name] = subprocess.check_output(
            ['rustup', 'which', '--toolchain', state_schema.NIGHTLY, name.lower()], text=True).strip()
    marker = 'fairing_state_' + re.sub('[^a-zA-Z0-9_]', '_', profile)
    env['RUSTDOCFLAGS'] = ('-Z unstable-options --output-format json --document-private-items '
                          '--document-hidden-items --cfg ' + marker + ' --check-cfg cfg(' + marker + ')')
    command = ['cargo', '+' + state_schema.NIGHTLY, 'doc', '--no-deps',
               '--manifest-path', str(PROBE / 'Cargo.toml')]
    for package in PACKAGES:
        command += ['-p', package]
    if profile == 'all' or mutation:
        command += ['--all-features']
    else:
        command += ['--no-default-features']
        if profile in {'osk-only', 'overlay-only'}:
            command += ['--features', 'fairing/' + profile.removesuffix('-only')]
    run('compile-' + profile, command, cwd=PROBE, env=env)
    for package in PACKAGES:
        filename = package.replace('-', '_') + '.json'
        shutil.copyfile(ROOT / 'target/state-schema/doc' / filename, destination / filename)


class Body:
    """Closed grammar: pure exhaustive field borrowing, labels and sum constructors only."""
    def __init__(self, text):
        self.tokens = re.findall(r'"(?:\\.|[^"\\])*"|::|=>|r#[^\W\d]\w*|[^\W\d]\w*|\d+|[^\s]', text)
        self.i = 0

    def at(self, token):
        return self.i < len(self.tokens) and self.tokens[self.i] == token

    def take(self, token=None):
        if self.i >= len(self.tokens):
            raise ValueError('Unexpected end of projection body')
        result = self.tokens[self.i]
        self.i += 1
        if token is not None and result != token:
            raise ValueError('Expected ' + token + ', got ' + result)
        return result

    def sequence(self, tokens):
        for token in tokens.split():
            self.take(token)

    def optional_comma(self):
        if self.at(','):
            self.take()

    def pattern(self):
        self.take('Self')
        variant = None
        if self.at('::'):
            self.take()
            variant = self.take().removeprefix('r#')
        fields = {}
        if self.at('{'):
            self.take()
            while not self.at('}'):
                source = self.take().removeprefix('r#')
                self.take(':')
                binding = self.take()
                if binding in fields:
                    raise ValueError('Duplicate binding')
                fields[binding] = source
                if not self.at(','):
                    break
                self.take()
            self.take('}')
        elif self.at('('):
            self.take()
            while not self.at(')'):
                fields[self.take()] = str(len(fields))
                if not self.at(','):
                    break
                self.take()
            self.take(')')
        return variant, fields

    def record(self, fields):
        self.take('(')
        rows = []
        while not self.at(')'):
            self.take('(')
            label = json.loads(self.take()).removeprefix('r#')
            self.take(',')
            binding = self.take()
            self.optional_comma()
            self.take(')')
            rows.append({'label': label, 'source': fields[binding]})
            if not self.at(','):
                break
            self.take()
        self.take(')')
        return rows

    def leaf(self, fields):
        if self.at('::'):
            self.sequence(':: core :: result :: Result ::')
            constructor = self.take()
            if constructor not in {'Ok', 'Err'}:
                raise ValueError('Unexpected sum constructor')
            self.take('(')
            path, label, rows = self.leaf(fields)
            self.optional_comma()
            self.take(')')
            return [constructor] + path, label, rows
        self.take('(')
        label = json.loads(self.take()).removeprefix('r#')
        self.take(',')
        rows = self.record(fields)
        self.optional_comma()
        self.take(')')
        return [], label, rows

    def parse(self):
        self.sequence('{ ( :: core :: marker :: PhantomData :: < Self > , {')
        cases = []
        if self.at('let'):
            self.take()
            variant, fields = self.pattern()
            self.sequence('= self ;')
            cases.append({'variant': variant, 'fields': fields, 'path': [], 'label': None,
                          'rows': self.record(fields)})
        else:
            self.sequence('match self {')
            while not self.at('}'):
                variant, fields = self.pattern()
                self.take('=>')
                path, label, rows = self.leaf(fields)
                cases.append({'variant': variant, 'fields': fields, 'path': path, 'label': label, 'rows': rows})
                self.optional_comma()
            self.take('}')
        self.take('}')
        self.optional_comma()
        self.sequence(') }')
        if self.i != len(self.tokens):
            raise ValueError('Unconsumed projection expression')
        return cases


def projection_models(profile):
    shapes, records = {}, []
    for package in PACKAGES:
        name = package.replace('-', '_')
        metadata = OUT / profile / (name + '.json')
        doc = json.loads(metadata.read_text())
        shapes.update(state_schema.extract(metadata))
        expanded = OUT / profile / (name + '-expanded.rs')
        text = subprocess.check_output([str(TARGET / 'debug/instrument'), str(expanded),
                                        '--inspect-expanded'], text=True)
        entries = [json.loads(line) for line in text.splitlines() if line]
        (OUT / profile / (name + '-bodies.json')).write_text(json.dumps(entries, indent=2) + '\n')
        definitions, bodies = {}, {}
        for entry in entries:
            destination = definitions if entry['record'] == 'definition' else bodies
            if entry['id'] in destination:
                raise ValueError('Duplicate compiler source identity: ' + entry['id'])
            destination[entry['id']] = entry
        if definitions.keys() != bodies.keys():
            raise ValueError('Not every compiled datatype has exactly one actual projection')
        symbols = {}
        for key, item in doc['index'].items():
            if item['crate_id'] != 0 or not ({'struct', 'enum'} & item['inner'].keys()):
                continue
            kind = next(k for k in ['struct', 'enum'] if k in item['inner'])
            methods = []
            for impl_id in item['inner'][kind]['impls']:
                impl_item = doc['index'][str(impl_id)]['inner']['impl']
                methods += [doc['index'][str(m)] for m in impl_item['items']
                            if doc['index'][str(m)]['name'] == '__fairing_state_projection']
            if len(methods) != 1:
                raise ValueError('Rustdoc projection is not unique for ' + str(item['name']))
            first = methods[0]['docs'].splitlines()[0]
            if not first.startswith('FAIRING_STATE_PROJECTION:'):
                raise ValueError('Missing compiler projection identity')
            identity = first.removeprefix('FAIRING_STATE_PROJECTION:')
            symbol = '::'.join(doc['paths'][str(key)]['path'])
            symbols[identity] = symbol, shapes[symbol]
        for identity, entry in definitions.items():
            symbol, type_schema = symbols.get(identity, (identity + '::' + entry['name'], None))
            definition = entry['definition']
            cases = Body(bodies[identity]['body']).parse()
            variants = definition.get('variants', [{'name': None, 'fields': definition.get('fields', [])}])
            expected = {v['name'].removeprefix('r#') if isinstance(v['name'], str) else None: v for v in variants}
            if len(cases) != len(variants) or {c['variant'] for c in cases} != set(expected):
                raise ValueError('Incomplete compiler variant coverage: ' + symbol)
            tags = [(tuple(c['path']), c['label']) for c in cases]
            if len(tags) != len(set(tags)):
                raise ValueError('Ambiguous model variant tag: ' + symbol)
            for case in cases:
                fields = expected[case['variant']]['fields']
                names = [f['name'].removeprefix('r#') for f in fields]
                if list(case['fields'].values()) != names:
                    raise ValueError('Non-exhaustive native field pattern: ' + symbol)
                if any(row['source'] not in names for row in case['rows']):
                    raise ValueError('Projection refers to a non-source field')
            records.append({'symbol': symbol, 'source_id': identity, 'definition': definition,
                            'compiler_type_schema': type_schema, 'cases': cases})
    if profile in {'all', 'minimal'}:
        baseline = json.loads((ROOT / 'verification/state-schema.json').read_text())
        if shapes != baseline['common'] | baseline['profiles'][profile]:
            raise SystemExit('Instrumented compiler state layout differs from reviewed product inventory')
    return records, shapes


def model_source(record, index):
    """Translate only the closed pure-borrow grammar, retaining arbitrary exact field values."""
    definition, cases = record['definition'], record['cases']
    is_enum = definition['kind'] == 'enum'
    variants = definition.get('variants', [{'name': None, 'fields': definition.get('fields', [])}])
    all_types = []
    for variant in variants:
        for field in variant['fields']:
            field['_logical_type'] = 'F' + str(len(all_types))
            all_types.append(field['_logical_type'])
    case_for = {case['variant']: case for case in cases}
    named = {v['name'].removeprefix('r#') if isinstance(v['name'], str) else None: i
             for i, v in enumerate(variants)}
    observed = []
    for variant in variants:
        key = variant['name'].removeprefix('r#') if isinstance(variant['name'], str) else None
        source = {f['name'].removeprefix('r#'): f for f in variant['fields']}
        for row in case_for[key]['rows']:
            name = source[row['source']]['_logical_type']
            if name not in observed:
                observed.append(name)
    c, m, s = 'C' + str(index), 'M' + str(index), 'S' + str(index)
    def params(values):
        return '<' + ','.join(values) + '>' if values else ''
    cp = params(all_types)
    mg = params((["'a"] if observed else []) + observed)
    sp = params(observed)
    gp = params((["'a"] if observed else []) + all_types)
    # Even an empty representation still borrows its input in the native function.
    ep = params(["'a"] + all_types)
    c_arms, m_arms, forward, backward, semantic_forward, semantic_backward, normalization = [], [], [], [], [], [], []
    for i, variant in enumerate(variants):
        key = variant['name'].removeprefix('r#') if isinstance(variant['name'], str) else None
        case = case_for[key]
        source_fields = {f['name'].removeprefix('r#'): j for j, f in enumerate(variant['fields'])}
        concrete_fields = ['f' + str(j) + ':' + f['_logical_type'] for j, f in enumerate(variant['fields'])]
        model_fields = ['v' + str(j) + ":&'a " + variant['fields'][source_fields[row['source']]]['_logical_type']
                        for j, row in enumerate(case['rows'])]
        if is_enum:
            c_arms.append('V' + str(i) + '{' + ','.join(concrete_fields) + '}')
            m_arms.append('V' + str(i) + '{' + ','.join(model_fields) + '}')
            c_pattern = c + '::V' + str(i) + '{' + ','.join('f' + str(j) for j in range(len(variant['fields']))) + '}'
            m_pattern = m + '::V' + str(i) + '{' + ','.join('v' + str(j) for j in range(len(case['rows']))) + '}'
            m_constructor = m + '::V' + str(i)
            fwd = m_constructor + '{' + ','.join('v' + str(j) + ':f' + str(source_fields[row['source']])
                                                 for j, row in enumerate(case['rows'])) + '}'
        else:
            c_arms = ['pub ' + value for value in concrete_fields]
            m_arms = ['pub ' + value for value in model_fields]
            c_pattern = None
            m_pattern = m + '{' + ','.join('v' + str(j) for j in range(len(case['rows']))) + '}'
            m_constructor = m
            fwd = m + '{' + ','.join('v' + str(j) + ':&value.f' + str(source_fields[row['source']])
                                      for j, row in enumerate(case['rows'])) + '}'
        target_index = named.get(case['label']) if is_enum else i
        restore = 'None'
        if target_index is not None:
            target = variants[target_index]
            labels = {row['label']: j for j, row in enumerate(case['rows'])}
            target_names = [f['name'].removeprefix('r#') for f in target['fields']]
            if len(labels) == len(case['rows']) and set(labels) == set(target_names):
                assignments = []
                type_compatible = True
                for j, field in enumerate(target['fields']):
                    position = labels[target_names[j]]
                    actual_type = variant['fields'][source_fields[case['rows'][position]['source']]]['_logical_type']
                    if actual_type != field['_logical_type']:
                        type_compatible = False
                    assignments.append('f' + str(j) + ':*v' + str(position))
                if type_compatible:
                    restore = 'Some(' + c + ('::V' + str(target_index) if is_enum else '') + '{' + ','.join(assignments) + '})'
        bwd = m_pattern + '=>' + restore
        backward.append(bwd)
        semantic_backward.append(bwd.replace(m_pattern, m_pattern.replace(m, s, 1)).replace(':*v', ':v'))
        if is_enum:
            forward.append(c_pattern + '=>' + fwd)
            semantic_forward.append(c_pattern + '=>' + fwd.replace(m_constructor, m_constructor.replace(m, s, 1), 1))
        else:
            forward = [fwd]
            semantic_forward = [fwd.replace(m + '{', s + '{', 1).replace(':&value.', ':value.')]
        normalization.append(m_pattern + '=>' + (m_constructor.replace(m, s, 1)) + '{'
                             + ','.join('v' + str(j) + ':*v' + str(j) for j in range(len(case['rows']))) + '}')
    c_decl = 'pub ' + ('enum ' if is_enum else 'struct ') + c + cp + '{' + ','.join(c_arms) + '}'
    m_decl = 'pub ' + ('enum ' if is_enum else 'struct ') + m + mg + '{' + ','.join(m_arms) + '}'
    s_decl = m_decl.replace(m + mg, s + sp, 1).replace(":&'a ", ':')
    forward_body = 'match value {' + ','.join(forward) + '}' if is_enum else forward[0]
    semantic_body = 'match value {' + ','.join(semantic_forward) + '}' if is_enum else semantic_forward[0]
    decode_call = 'decode_' + str(index) + ('::' + cp if cp else '')
    semantic_call = 'semantic_' + str(index) + ('::' + cp if cp else '')
    restore_call = 'restore_' + str(index) + ('::' + cp if cp else '')
    normalize_call = 'normalize_' + str(index)
    semantic_gp = params(all_types)
    actual_gp = params(all_types + ['World', 'O'])
    return '\n'.join([
        '// ' + record['symbol'] + ' -- ' + record['source_id'],
        c_decl, m_decl, s_decl,
        'pub open spec fn decode_' + str(index) + gp + '(view:' + m + mg + ')->Option<' + c + cp + '>{',
        'match view {' + ','.join(backward) + '}}',
        'pub open spec fn semantic_' + str(index) + semantic_gp + '(value:' + c + cp + ')->' + s + sp + '{' + semantic_body + '}',
        'pub open spec fn restore_' + str(index) + semantic_gp + '(view:' + s + sp + ')->Option<' + c + cp + '>{',
        'match view {' + ','.join(semantic_backward) + '}}',
        'pub open spec fn normalize_' + str(index) + mg + '(view:' + m + mg + ')->' + s + sp + '{',
        'match view {' + ','.join(normalization) + '}}',
        'pub fn project_' + str(index) + ep + "(value:&'a " + c + cp + ')->(view:' + m + mg + ')',
        'ensures ' + decode_call + '(view)==Some(*value),',
        normalize_call + '(view)==' + semantic_call + '(*value),',
        '{' + forward_body + '}',
        'pub proof fn unique_model_group_' + str(index) + semantic_gp + '(view:' + s + sp + ')',
        'ensures match ' + restore_call + '(view){Some(value)=>' + semantic_call + '(value)==view,None=>false},',
        '{}',
        'pub proof fn actual_state_coverage_' + str(index) + actual_gp + '(value:' + c + cp + ',world:World,',
        'observe:spec_fn(ConcreteState<' + c + cp + ',World>)->O)',
        'ensures ' + restore_call + '(' + semantic_call + '(value))==Some(value),',
        'match ' + restore_call + '(' + semantic_call + '(value)){',
        'Some(roots)=>observe(ConcreteState{roots,world})==observe(ConcreteState{roots:value,world}),None=>false},',
        '{}', ''])


def schema_key(record):
    return json.dumps([record['source_id'], state_configuration.node_shape(record), record['cases']], sort_keys=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--skip-build', action='store_true', help='Development reuse only; never emits a proof receipt')
    args = parser.parse_args()
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise SystemExit('The reviewed native compiler binding targets Linux x86_64')
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = OUT / 'summary.json'
    receipt.unlink(missing_ok=True)
    before = fingerprints()
    check_tool_lock()
    if not args.skip_build:
        prepare()
    verus = install.tool('verus')
    frame = (ROOT / 'verification/verus/state_representation.rs').read_text()
    frame_count = len(re.findall(r'pub proof fn ', frame))
    if frame_count != 8:
        raise SystemExit('Review the independent composition theorem count')

    def write_model(path, records):
        path.write_text('mod representation {\n' + frame + '\n}\nuse vstd::prelude::*;\n'
                        'use representation::ConcreteState;\nverus! {\n'
                        + '\n'.join(model_source(copy.deepcopy(record), i) for i, record in enumerate(records)) + '\n}\n')

    proven, profiles = {}, {}
    for profile in ['all', 'minimal', 'osk-only', 'overlay-only']:
        if not args.skip_build:
            build(profile)
        records, shapes = projection_models(profile)
        (OUT / profile / 'compiled-projections.json').write_text(json.dumps(records, indent=2) + '\n')
        pending = [record for record in records if schema_key(record) not in proven]
        for start in range(0, len(pending), 40):
            chunk = pending[start:start + 40]
            name = 'correspondence-' + profile + '-' + str(start)
            source = OUT / profile / (name + '.rs')
            write_model(source, chunk)
            run(name, [verus, '--crate-type', 'lib', source], verified=3 * len(chunk) + frame_count)
            for record in chunk:
                proven[schema_key(record)] = record
        if any(schema_key(record) not in proven for record in records):
            raise SystemExit('A compiled native state has no successful inverse theorem')
        profiles[profile] = {'native_projections': len(records), 'named_types': len(shapes),
                            'transparent_aliases': {name: item['type'] for name, item in shapes.items()
                                                    if item['kind'] == 'type_alias'}}
    manifest = json.loads((OUT / 'source-manifest.json').read_text())
    terms, configurations = state_configuration.inspect(ROOT, manifest, list(proven.values()))
    (OUT / 'configuration-summary.json').write_text(json.dumps(configurations, indent=2) + '\n')
    for start in range(0, len(terms), 40):
        chunk = terms[start:start + 40]
        source = OUT / ('configurations-' + str(start) + '.rs')
        source.write_text(state_configuration.theorem_source(chunk))
        run('configurations-' + str(start), [verus, '--crate-type', 'lib', source], verified=len(chunk))
    if args.skip_build:
        print('Development reuse passed; no fresh native proof receipt was emitted')
        return
    for profile, mutation, symbol in [
        ('control-omit-field', 'omit-field', 'fairing::shell::Shell'),
        ('control-wrong-variant', 'wrong-variant', 'fairing_widgets::interaction::concrete::Lifetime')]:
        build(profile, mutation)
        candidates, _ = projection_models(profile)
        selected = [record for record in candidates if record['symbol'] == symbol]
        if len(selected) != 1:
            raise SystemExit('Native negative-control target is not unique: ' + symbol)
        source = OUT / (profile + '.rs')
        write_model(source, selected)
        run(profile, [verus, '--crate-type', 'lib', source], rejection=True)
    extrema = []
    for profile in ['all', 'minimal']:
        extrema += json.loads((OUT / profile / 'compiled-projections.json').read_text())
    incomplete_terms, _ = state_configuration.inspect(ROOT, manifest, extrema)
    builder_terms = [term for term in incomplete_terms if term[0].startswith('crates/fairing/src/shell/builder.rs#')]
    if not builder_terms:
        raise SystemExit('Missing mixed-configuration negative-control target')
    source = OUT / 'control-missing-mixed-configurations.rs'
    source.write_text(state_configuration.theorem_source(builder_terms))
    run('control-missing-mixed-configurations', [verus, '--crate-type', 'lib', source], rejection=True)
    tampered = OUT / 'tampered-expanded.rs'
    shutil.copyfile(OUT / 'all/fairing_widgets-expanded.rs', tampered)
    run('tamper-expanded', [TARGET / 'debug/instrument', tampered, '--tamper-expanded'])
    run('control-expanded-body-binding', [TARGET / 'debug/instrument', tampered, '--inspect-expanded'],
        error_marker='expanded projection differs from attestation')
    if before != fingerprints():
        raise SystemExit('Native state/proof inputs changed during verification')
    evidence = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in OUT.rglob('*') if p.is_file() and PROBE not in p.parents and p != receipt
                and p.suffix in {'.rs', '.json'}}
    report = {'state_representation': 'proved', 'native_built': True,
              'scope': 'Lossless source-derived canonical state representation; not operation semantics',
              'profiles': profiles, 'configurations': configurations,
              'verified_native_schemas': len(proven), 'configuration_theorems': len(terms),
              'operations_and_progress': 'not-claimed', 'inputs': before,
              'commands': COMMANDS, 'evidence': evidence}
    receipt.write_text(json.dumps(report, indent=2) + '\n')
    print('CONCRETE_STATE_REPRESENTATION_PROVED: ' + str(len(proven))
          + ' native schemas; ' + str(len(terms)) + ' configuration theorems', flush=True)


if __name__ == '__main__':
    main()
