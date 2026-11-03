#!/usr/bin/env python3
"""Export compiler-resolved state roots, closures and private data dependencies.

The export is an audit input, never a proof of whole-UI-state completeness.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

import install
import state_schema

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/state-types'
TOOL = ROOT / 'verification/type-importer'


def run(name, command, env=None):
    log = OUT / (name + '.log')
    with log.open('w') as output:
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env,
                                stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        raise SystemExit(name + ' failed:\n' + log.read_text()[-12000:])
    print(name + ': passed', flush=True)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    channel = state_schema.NIGHTLY
    run('install-compiler-interface', ['rustup', 'toolchain', 'install', channel,
        '--profile', 'minimal', '--component', 'rustc-dev'])
    run('compiler-version', ['rustup', 'run', channel, 'rustc', '--version', '--verbose'])
    run('build-importer', ['cargo', '+' + channel, 'build', '--locked',
        '--manifest-path', TOOL / 'Cargo.toml', '--target-dir', ROOT / 'target/type-importer'])
    sysroot = subprocess.check_output(['rustup', 'run', channel, 'rustc', '--print', 'sysroot'],
                                     cwd=ROOT, text=True).strip()
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(ROOT / 'target/state-type-check'),
        RUSTC_WRAPPER=str(ROOT / 'target/type-importer/debug/state-types'),
        FAIRING_STATE_TYPES_OUT=str(OUT),
        LD_LIBRARY_PATH=str(Path(sysroot) / 'lib') + os.pathsep + os.environ.get('LD_LIBRARY_PATH', ''))
    run('import-native-state-roots', ['cargo', '+' + channel, 'check', '--locked',
        '-p', 'fairing', '-p', 'fairing-widgets', '--all-features'], env)
    summary = {'whole_ui_state_status': 'unproved', 'purpose': 'Native state-domain dependency audit', 'crates': {}}
    for name in ['fairing', 'fairing_widgets', 'egui']:
        path = OUT / (name + '.jsonl')
        if not path.is_file():
            raise SystemExit('Compiler did not export actual state roots for ' + name)
        rows = [json.loads(line) for line in path.read_text().splitlines()]
        types = {r['id']: r for r in rows if r['record'] == 'type'}
        for row in rows:
            children = []
            if row['record'] == 'adt_schema':
                children = [f['type'] for v in row['variants'] for f in v['fields']]
            elif row['record'] == 'body':
                children = [local['type'] for local in row['locals']]
            elif row['record'] == 'type':
                children = row.get('elements', []) + row.get('arguments', []) + row.get('captures', [])
                if 'element' in row:
                    children.append(row['element'])
            if any(child not in types for child in children):
                raise SystemExit('A state type reference was omitted from ' + name)
        counts = {}
        for row in types.values():
            counts[row['kind']] = counts.get(row['kind'], 0) + 1
        summary['crates'][name] = {'types': len(types), 'kinds': counts,
            'declarations': sum(r['record'] == 'declaration' for r in rows),
            'bodies': sum(r['record'] == 'body' for r in rows),
            'adt_schemas': sum(r['record'] == 'adt_schema' for r in rows)}
    (OUT / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
