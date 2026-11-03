#!/usr/bin/env python3
"""Import real egui state and bind its native focus query to an independent domain.

This component command cannot establish whole-product state completeness.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.request

import install
import state_correspondence as structure

ROOT = structure.ROOT
OUT = ROOT / 'target/assurance/native-ui-state'
MODEL = ROOT / 'verification/verus/egui_focus_state.rs'
FOCUS_FIELDS = {
    'focused_widget': 'Option < FocusWidget >',
    'id_previous_frame': 'Option < Id >',
    'id_two_frames_ago': 'Option < Id >',
    'id_next_frame': 'Option < Id >',
    'give_to_next': 'bool',
    'last_interested': 'Option < Id >',
    'focus_direction': 'FocusDirection',
    'top_modal_layer': 'Option < LayerId >',
    'top_modal_layer_current_frame': 'Option < LayerId >',
    'focus_widgets_cache': 'IdMap < Rect >',
}
OWNER_FIELDS = {'id': 'Id', 'filter': 'EventFilter'}
DIRECTIONS = {'None', 'Up', 'Right', 'Down', 'Left', 'Next', 'Previous'}


def run(name, command, env=None, *, verified=None, negative=False):
    log = OUT / (name + '.log')
    with log.open('w') as output:
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env=env,
                                stdout=output, stderr=subprocess.STDOUT)
    text = log.read_text()
    valid = result.returncode == 0
    if verified is not None:
        valid = valid and f'verification results:: {verified} verified, 0 errors' in text
    if negative:
        valid = (result.returncode != 0 and ('postcondition not satisfied' in text or 'assertion failed' in text)
                 and re.search(r'verification results:: \d+ verified, [1-9]\d* errors', text)
                 and not re.search(r'resource limit|timed out|timeout', text, re.I))
    if not valid:
        raise SystemExit(name + ' failed:\n' + text[-10000:])
    print(name + ': ' + ('semantic control rejected' if negative else 'passed'), flush=True)
    return {'name': name, 'exit_code': result.returncode, 'negative_control': negative}


def tokens(text):
    return re.findall(r'::|=>|[^\W\d]\w*|\d+|[^\s]', text)


def parse_expansion(path):
    raw = subprocess.check_output([str(structure.TARGET / 'debug/instrument'), str(path),
                                   '--inspect-egui'], text=True)
    rows = [json.loads(line) for line in raw.splitlines() if line]
    definitions = {r['id']: r for r in rows if r['record'] == 'definition'}
    bodies = {r['id']: r for r in rows if r['record'] == 'projection'}
    if len(definitions) != sum(r['record'] == 'definition' for r in rows) or definitions.keys() != bodies.keys():
        raise SystemExit('Compiled egui datatype/projection identities are not bijective')
    records = {}
    for identity, definition in definitions.items():
        records[identity] = {'source_id': identity, 'symbol': 'egui::' + definition['name'],
            'definition': definition['definition'], 'compiler_type_schema': None,
            'cases': structure.Body(bodies[identity]['body']).parse()}
    queries = {}
    for row in rows:
        if row['record'] == 'query':
            key = row['owner'] + '::' + row['method']
            if key in queries:
                raise SystemExit('Ambiguous actual query identity: ' + key)
            queries[key] = row['body']
    return records, queries


def native_focus_source(records, queries):
    selected = {}
    for name in ['Focus', 'FocusWidget', 'FocusDirection']:
        found = [r for r in records.values() if r['symbol'] == 'egui::' + name
                 and '/src/memory/mod.rs#' in r['source_id']]
        if len(found) != 1:
            raise SystemExit('Native memory state is not uniquely bound: ' + name)
        selected[name] = found[0]
    for name, expected in [('Focus', FOCUS_FIELDS), ('FocusWidget', OWNER_FIELDS)]:
        fields = selected[name]['definition']['fields']
        actual = {f['name']: f['type']['compiler_tokens'] for f in fields}
        if list(actual) != list(expected) or any(tokens(actual[k]) != tokens(v) for k, v in expected.items()):
            raise SystemExit('FOCUS_DOMAIN_INCOMPLETE: independently specified fields/types changed: ' + name)
        rows = selected[name]['cases']
        if len(rows) != 1 or [(r['label'], r['source']) for r in rows[0]['rows']] != [(n, n) for n in expected]:
            raise SystemExit('FOCUS_BINDING_INCOMPLETE: native projection dropped or relabelled a field')
    variants = selected['FocusDirection']['definition']['variants']
    if {v['name'] for v in variants} != DIRECTIONS or any(v['fields'] for v in variants):
        raise SystemExit('FOCUS_DIRECTION_DOMAIN_INCOMPLETE')
    getter = tokens(queries['Focus::focused'])
    regular = tokens('{ self.focused_widget.as_ref().map(|w| w.id) }')
    if getter == regular:
        native_getter = 'match s.focused_widget {None=>None,Some(w)=>Some(w.id)}'
    elif getter == tokens('{ None }'):
        # The supported native semantic mutation must reach the verifier, not fail parsing.
        native_getter = 'None'
    else:
        raise SystemExit('Untranslated native focus query; no observation-preservation claim')
    if tokens(queries['Memory::focus']) != tokens('{ self.focus.get(&self.viewport_id) }'):
        raise SystemExit('Untranslated actual viewport selector')
    if tokens(queries['Memory::focused']) != tokens('{ self.focus()?.focused() }'):
        raise SystemExit('Untranslated actual public memory query')
    model = MODEL.read_text()
    original = 'match s.focused_widget {None=>None,Some(owner)=>Some(owner.id)}'
    if model.count(original) != 1:
        raise SystemExit('Ambiguous independent native-query insertion site')
    return model.replace(original, native_getter), selected


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = OUT / 'focus-receipt.json'
    receipt.unlink(missing_ok=True)
    before = structure.fingerprints()
    extra_inputs = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in [Path(__file__), MODEL, ROOT / 'verification/ui_state_rustc.py']}
    structure.OUT.mkdir(parents=True, exist_ok=True)
    structure.check_tool_lock()
    structure.prepare()
    locked = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
    matches = [p for p in locked if p['name'] == 'egui' and p.get('source', '').startswith('registry+')]
    if len(matches) != 1:
        raise SystemExit('Need one reviewed upstream egui version')
    pin = matches[0]
    archive = OUT / ('egui-' + pin['version'] + '.crate')
    if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest() != pin['checksum']:
        with urllib.request.urlopen('https://static.crates.io/crates/egui/' + archive.name, timeout=60) as source:
            archive.write_bytes(source.read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != pin['checksum']:
        raise SystemExit('Locked egui archive checksum mismatch')
    vendor = OUT / 'upstream'
    if vendor.exists():
        shutil.rmtree(vendor)
    vendor.mkdir()
    with tarfile.open(archive) as source:
        source.extractall(vendor, filter='data')
    crate = vendor / ('egui-' + pin['version'])
    instrument = structure.TARGET / 'debug/instrument'
    manifest = []
    for source in sorted((crate / 'src').rglob('*.rs')):
        identity = 'egui/' + pin['version'] + '/' + str(source.relative_to(crate))
        text = subprocess.check_output([str(instrument), str(source), identity], text=True)
        manifest.extend(json.loads(line) for line in text.splitlines() if line)
    (OUT / 'egui-source-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    with (crate / 'Cargo.toml').open('a') as output:
        output.write('\n[dependencies.fairing-state-projection]\npath = ' + json.dumps(str(structure.TOOLS)) + '\n')
    root_manifest = structure.PROBE / 'Cargo.toml'
    text = root_manifest.read_text()
    if '[patch.crates-io]' in text:
        raise SystemExit('Review existing dependency patches before instrumentation')
    root_manifest.write_text(text + '\n[patch.crates-io]\negui = { path = ' + json.dumps(str(crate)) + ' }\n')
    wrapper = OUT / 'rustc-wrapper'
    wrapper.write_text('#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' '
                       + shlex.quote(str(ROOT / 'verification/ui_state_rustc.py')) + ' "$@"\n')
    wrapper.chmod(0o755)
    expansion = OUT / 'egui-expanded.rs'
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / 'target/native-ui-state'),
               CARGO_INCREMENTAL='0', RUSTC_WRAPPER=str(wrapper),
               FAIRING_UI_STATE_EXPANSION=str(expansion))
    for name in ['RUSTDOC', 'RUSTDOCFLAGS', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS',
                 'FAIRING_STATE_PROJECTION_MUTATION']:
        env.pop(name, None)
    command = ['cargo', '+' + structure.state_schema.NIGHTLY, 'check', '--manifest-path', root_manifest,
               '-p', 'fairing', '-p', 'fairing-widgets', '--all-features']
    commands = [run('native-egui', command, env)]
    approved = {(p['name'], p['version'], p.get('source'), p.get('checksum')) for p in locked}
    for package in tomllib.loads((structure.PROBE / 'Cargo.lock').read_text())['package']:
        if package.get('source') and (package['name'], package['version'], package.get('source'),
                                      package.get('checksum')) not in approved:
            raise SystemExit('Instrumented native binding resolved an unreviewed dependency')
    if not expansion.is_file():
        raise SystemExit('No production egui compiler expansion was captured')
    records, queries = parse_expansion(expansion)
    (OUT / 'compiled-egui-projections.json').write_text(json.dumps(list(records.values()), indent=2) + '\n')
    (OUT / 'native-focus-queries.json').write_text(json.dumps(queries, indent=2) + '\n')
    source, selected = native_focus_source(records, queries)
    proof = OUT / 'focus-domain.rs'
    proof.write_text(source)
    verus = install.tool('verus')
    commands.append(run('focus-domain', [verus, '--crate-type=lib', proof], verified=4))
    # Verify the compiler-derived native field projections used by the semantic adapter.
    frame = (ROOT / 'verification/verus/state_representation.rs').read_text()
    binding = OUT / 'focus-native-fields.rs'
    binding.write_text('mod representation {\n' + frame + '\n}\nuse vstd::prelude::*;\n'
        'use representation::ConcreteState;\nverus! {\n'
        + '\n'.join(structure.model_source(__import__('copy').deepcopy(r), i)
                    for i, r in enumerate(selected.values())) + '\n}\n')
    commands.append(run('focus-native-fields', [verus, '--crate-type=lib', binding], verified=17))
    # Change the actual upstream method, recompile it, then lower that compiled body.
    memory_source = crate / 'src/memory/mod.rs'
    commands.append(run('mutate-native-focus', [instrument, memory_source, '--mutate-focus-query']))
    changed = OUT / 'egui-mutant-expanded.rs'
    mutated_env = dict(env, FAIRING_UI_STATE_EXPANSION=str(changed))
    commands.append(run('compile-native-focus-control', command, mutated_env))
    mutant_records, mutant_queries = parse_expansion(changed)
    mutant_source, _ = native_focus_source(mutant_records, mutant_queries)
    mutant_proof = OUT / 'focus-native-control.rs'
    mutant_proof.write_text(mutant_source)
    commands.append(run('focus-native-control', [verus, '--crate-type=lib', mutant_proof], negative=True))
    if before != structure.fingerprints() or extra_inputs != {
        str((ROOT / p).relative_to(ROOT)): hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
        for p in extra_inputs}:
        raise SystemExit('UI state proof inputs changed during native compilation')
    report = {'component': 'egui-focus-state-domain', 'status': 'proved',
              'whole_ui_state_status': 'unproved', 'native_source_archive': pin,
              'scope': 'All Focus fields, all FocusDirection variants, all viewport-map sizes and exact native focused-query composition',
              'inputs': before | extra_inputs, 'commands': commands,
              'native_fields': FOCUS_FIELDS, 'native_queries': queries,
              'other_egui_state_domains': 'not-claimed',
              'native_expansion_sha256': hashlib.sha256(expansion.read_bytes()).hexdigest()}
    receipt.write_text(json.dumps(report, indent=2) + '\n')
    print('NATIVE_FOCUS_STATE_DOMAIN_PROVED; WHOLE_UI_STATE_UNPROVED', flush=True)


if __name__ == '__main__':
    main()
