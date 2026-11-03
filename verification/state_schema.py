#!/usr/bin/env python3
"""Compiler-derived inventory of product types; never equates field coverage with a behavioral proof."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/state-schema'
BASELINE = ROOT / 'verification/state-schema.json'
NIGHTLY = json.loads((ROOT / 'verification/tools.json').read_text())['nightly']
PACKAGES = ['fairing', 'fairing-widgets']
PROFILES = ['all', 'minimal']


def normalize(value, doc):
    if isinstance(value, list):
        return [normalize(item, doc) for item in value]
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if key == 'id':
                target = doc['paths'].get(str(item))
                if target is None:
                    raise SystemExit('Unresolved compiler type id; review schema extraction: ' + str(item))
                result['symbol'] = '::'.join(target['path'])
            else:
                result[key] = normalize(item, doc)
        return result
    return value


def extract(path):
    doc = json.loads(path.read_text())
    if doc['format_version'] != 61:
        raise SystemExit('Review the pinned rustdoc JSON schema version')
    index = doc['index']

    def field(key):
        item = index[str(key)]
        return {'name': item['name'], 'type': normalize(item['inner']['struct_field'], doc)}

    def fields(kind):
        if kind == 'unit' or kind == 'plain':
            return []
        if 'plain' in kind or 'struct' in kind:
            body = kind.get('plain', kind.get('struct'))
            if body['has_stripped_fields']:
                raise SystemExit('Compiler omitted private fields; inventory is incomplete')
            return [field(key) for key in body['fields']]
        if 'tuple' in kind:
            if any(key is None for key in kind['tuple']):
                raise SystemExit('Compiler omitted a tuple field')
            return [field(key) for key in kind['tuple']]
        raise SystemExit('Unknown compiler field layout: ' + repr(kind))

    result = {}
    for key, item in index.items():
        if item['crate_id'] != 0 or item['name'] is None:
            continue
        kinds = set(item['inner']) & {'struct', 'enum', 'union', 'type_alias'}
        if not kinds:
            continue
        kind = next(iter(kinds))
        location = doc['paths'].get(key)
        if location is None:
            raise SystemExit('A product type has no compiler path: ' + item['name'])
        name = '::'.join(location['path'])
        body = item['inner'][kind]
        value = {'kind': kind, 'generics': normalize(body['generics'], doc)}
        if kind == 'struct':
            value['fields'] = fields(body['kind'])
        elif kind == 'enum':
            if body['has_stripped_variants']:
                raise SystemExit('Compiler omitted enum variants')
            value['variants'] = [
                {'name': index[str(v)]['name'], 'fields': fields(index[str(v)]['inner']['variant']['kind'])}
                for v in body['variants']
            ]
        elif kind == 'union':
            if body['has_stripped_fields']:
                raise SystemExit('Compiler omitted union fields')
            value['fields'] = [field(v) for v in body['fields']]
        else:
            value['type'] = normalize(body['type'], doc)
        # This label only binds a type to proof inputs. It is not a claim that all
        # methods/field effects or the enclosing application have been verified.
        if name.startswith('fairing_widgets::interaction::concrete::'):
            value['proof_input'] = 'concrete-carriers'
        elif name.startswith('fairing_widgets::interaction::rules::'):
            value['proof_input'] = 'lifecycle-kernel'
        else:
            value['proof_input'] = None
        result[name] = value
    return dict(sorted(result.items()))


def build(profile):
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(ROOT / 'target/state-schema'),
               RUSTDOCFLAGS='-Z unstable-options --output-format json --document-private-items --document-hidden-items')
    env.pop('RUSTFLAGS', None)
    command = ['cargo', '+' + NIGHTLY, 'doc', '--locked', '--no-deps']
    for package in PACKAGES:
        command += ['-p', package]
    command += ['--all-features'] if profile == 'all' else ['--no-default-features']
    dest = OUT / profile
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / 'compiler.log').open('w') as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        raise SystemExit((dest / 'compiler.log').read_text()[-10000:])
    for package in PACKAGES:
        name = package.replace('-', '_') + '.json'
        shutil.copyfile(ROOT / 'target/state-schema/doc' / name, dest / name)


def source_inputs():
    paths = list((ROOT / 'crates').rglob('*.rs')) + list((ROOT / 'crates').rglob('Cargo.toml'))
    paths += [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'verification/tools.json', Path(__file__)]
    return {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(paths)}


def main():
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise SystemExit('This reviewed compiler inventory targets Linux x86_64')
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--write', action='store_true', help='Explicitly update the reviewed type inventory')
    p.add_argument('--skip-build', action='store_true', help='Read existing compiler JSON during local inventory review')
    args = p.parse_args()
    before = source_inputs()
    profiles = {}
    for profile in PROFILES:
        if not args.skip_build:
            build(profile)
        types = {}
        for package in PACKAGES:
            path = OUT / profile / (package.replace('-', '_') + '.json')
            types.update(extract(path))
        profiles[profile] = dict(sorted(types.items()))
    common = {name: value for name, value in profiles['all'].items()
              if profiles['minimal'].get(name) == value}
    differences = {profile: {name: value for name, value in types.items() if name not in common}
                   for profile, types in profiles.items()}
    actual = {'compiler': NIGHTLY, 'format_version': 61, 'target': 'x86_64-unknown-linux-gnu',
              'common': common, 'profiles': differences}
    lines = ['{', '  "compiler": ' + json.dumps(NIGHTLY) + ',', '  "format_version": 61,',
             '  "target": "x86_64-unknown-linux-gnu",', '  "common": {']
    def declarations(types, indent):
        return [indent + json.dumps(name) + ': ' + json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
                + (',' if j + 1 < len(types) else '') for j, (name, value) in enumerate(types.items())]
    lines += declarations(common, '    ')
    lines += ['  },', '  "profiles": {']
    for i, (profile, types) in enumerate(differences.items()):
        lines.append('    ' + json.dumps(profile) + ': {')
        lines += declarations(types, '      ')
        lines.append('    }' + (',' if i + 1 < len(differences) else ''))
    encoded = '\n'.join(lines + ['  }', '}']) + '\n'
    if args.write:
        BASELINE.write_text(encoded)
    if not BASELINE.exists() or json.loads(BASELINE.read_text()) != actual:
        (OUT / 'unreviewed-schema.json').write_text(encoded)
        raise SystemExit('STATE_SCHEMA_CHANGED: product fields/variants/types changed; review state correspondence before updating the inventory')
    if before != source_inputs():
        raise SystemExit('Compiler inventory inputs changed while building metadata')
    report = {'status': 'matched', 'compiler': NIGHTLY, 'profiles': {}, 'built': not args.skip_build, 'source_inputs': before,
              'scope': 'Compiler type/field inventory for Linux x86_64, minimal and all features; not a state/transition refinement theorem',
              'baseline_sha256': hashlib.sha256(BASELINE.read_bytes()).hexdigest()}
    for profile, types in profiles.items():
        mapped = [name for name, item in types.items() if item['proof_input'] is not None]
        unmapped = [name for name, item in types.items() if item['proof_input'] is None]
        report['profiles'][profile] = {'types': len(types), 'proof_input_types': mapped,
                                       'unmapped_types': unmapped, 'unmapped_count': len(unmapped)}
    (OUT / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({name: {'types': value['types'], 'unmapped': value['unmapped_count']}
                      for name, value in report['profiles'].items()}, indent=2))


if __name__ == '__main__':
    main()
