#!/usr/bin/env python3
"""Inventory compiler-exported public contracts; report changes without a SemVer claim."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def inventory(data):
    index, paths = data['index'], data['paths']
    def item(identity):
        return index.get(str(identity))
    def identity(value):
        path = paths.get(str(value))
        if path:
            return '::'.join(path['path'])
        target = item(value)
        return (target or {}).get('name') or '<anonymous>'
    def normalize(value):
        if isinstance(value, list):
            return [normalize(v) for v in value]
        if not isinstance(value, dict):
            return value
        return {k: identity(v) if k == 'id' else normalize(v) for k, v in value.items()
                if k not in ('span', 'docs', 'links', 'deprecation')}
    result = {}
    def visit(key, exported, active=frozenset()):
        target = item(key)
        if not target or key in active:
            return
        inner = target['inner']
        kind, content = next(iter(inner.items()))
        if kind == 'use':
            result[exported + '#use'] = {'kind': kind, 'target': identity(content['id']), 'glob': content['is_glob']}
            if not content['is_glob']:
                visit(content['id'], exported, active | {key})
            # Globs remain explicit reexports in the inventory; foreign bodies are not expanded.
            return
        if target['crate_id'] != 0:
            return
        record = {'kind': kind, 'attrs': [attr for attr in target['attrs'] if 'CfgTrace(' not in str(attr) and 'CfgAttrTrace(' not in str(attr)]}
        record_key = exported + '#' + kind
        if record_key in result and result[record_key] != record:
            raise SystemExit('Ambiguous exported contract; extend the extractor: ' + record_key)
        result[record_key] = record
        children = []
        if kind == 'module':
            children = content['items']
        elif kind in ('struct', 'union', 'enum', 'trait'):
            for impl_id in content.get('impls', []):
                impl = item(impl_id)['inner']['impl']
                if impl['trait'] is None:
                    children += impl['items']
            if kind == 'enum':
                children += content['variants']
            if kind == 'trait':
                children += content['items']
            if kind == 'union':
                children += content['fields']
            if kind == 'struct':
                shape = content['kind']
                if isinstance(shape, dict):
                    if 'plain' in shape:
                        children += shape['plain']['fields']
                    elif 'tuple' in shape:
                        children += [field for field in shape['tuple'] if field is not None]
            record['declaration'] = normalize({k: v for k, v in content.items()
                                              if k not in ('impls', 'items', 'variants', 'kind', 'fields', 'implementations')})
            if kind == 'struct':
                record['shape'] = next(iter(content['kind'])) if isinstance(content['kind'], dict) else content['kind']
        elif kind == 'variant':
            shape = content['kind']
            record['discriminant'] = content['discriminant']
            if isinstance(shape, dict):
                variant_kind, fields = next(iter(shape.items()))
                children = fields['fields'] if variant_kind == 'struct' else fields
                children = [field for field in children if field is not None]
                record['shape'] = variant_kind
            else:
                record['shape'] = shape
        else:
            record['declaration'] = normalize(content)
        for child_id in children:
            child = item(child_id)
            if not child:
                continue
            if child['visibility'] != 'public' and kind not in ('enum', 'trait', 'variant'):
                continue
            name = child['name']
            if name is None and 'use' in child['inner']:
                name = child['inner']['use']['name']
            if name:
                visit(child_id, exported + '::' + name, active | {key})
    visit(data['root'], item(data['root'])['name'])
    return dict(sorted(result.items()))


def main():
    global ROOT
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/api-surface/summary.json')
    parser.add_argument('--compare', type=Path)
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    ROOT = args.root.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    compiler = subprocess.check_output(['rustc', '-vV'], cwd=ROOT, text=True)
    args.output.unlink(missing_ok=True)
    def inputs():
        paths = sorted([*(ROOT / 'crates').rglob('*.rs'), *(ROOT / 'crates').rglob('Cargo.toml'), ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'rust-toolchain.toml'])
        return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    before = inputs()
    extractor = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    profiles = {}
    for package in ('fairing-widgets', 'fairing'):
        for profile in ('minimal', 'all'):
            command = ['cargo', 'rustdoc', '--locked', '-p', package, '--lib',
                       '--all-features' if profile == 'all' else '--no-default-features',
                       '--', '--document-hidden-items', '-Z', 'unstable-options', '--output-format', 'json']
            # rustdoc JSON is unstable; the repository toolchain is pinned and recorded.
            env = dict(os.environ, RUSTC_BOOTSTRAP='1', CARGO_INCREMENTAL='0')
            env.pop('RUSTDOCFLAGS', None)
            with (args.output.parent / f'{package}-{profile}.log').open('w') as log:
                subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            path = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')) / 'doc' / (package.replace('-', '_') + '.json')
            data = json.loads(path.read_text())
            profiles[f'{package}/{profile}'] = {'format_version': data['format_version'], 'items': inventory(data), 'command': command}
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if before != inputs() or extractor != hashlib.sha256(Path(__file__).read_bytes()).hexdigest():
        raise SystemExit('Source or extractor changed while building the API inventory')
    report = {'extractor_sha256': extractor, 'compiler': compiler, 'head': source, 'inputs': before,
              'profiles': profiles,
              'scope': 'Compiler-exported paths, inherent public members, fields, variants and trait items, including hidden public items. Glob reexports are recorded without expanding foreign bodies. Structural comparison is not a complete semantic compatibility proof; private representation, behavior, layout, auto traits and downstream macro expansion require separate review.'}
    if args.compare:
        old = json.loads(args.compare.read_text())
        if old['extractor_sha256'] != extractor:
            raise SystemExit('Use the same extractor for baseline and candidate inventory')
        if old['compiler'] != compiler:
            raise SystemExit('Use the same compiler for the baseline and candidate inventory')
        changes = {}
        for profile, current in profiles.items():
            previous = old['profiles'][profile]
            if previous['format_version'] != current['format_version']:
                raise SystemExit('rustdoc JSON format changed; review the extractor')
            before, after = previous['items'], current['items']
            changes[profile] = {'removed': sorted(before.keys() - after.keys()),
                                'added': sorted(after.keys() - before.keys()),
                                'changed': sorted(k for k in before.keys() & after.keys() if before[k] != after[k])}
        report['changes'] = changes
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({p: len(v['items']) for p, v in profiles.items()}, indent=2))


if __name__ == '__main__':
    main()
