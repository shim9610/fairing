#!/usr/bin/env python3
"""Verify actual element traversal, with explicit UI/authority contract adapters."""
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import install

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/elements'

def inputs():
    return [ROOT / p for p in ['crates/fairing/src/elements.rs', 'crates/fairing/src/elements/dispatch.rs',
        'crates/fairing/src/access/protected.rs', 'crates/fairing/src/access/protected_dispatch.rs',
        'crates/fairing/src/access/session_rules.rs', 'verification/kani_elements.rs',
        'verification/element_proofs.py', 'verification/element_proof_controls.py', 'verification/tools.json']]

def hashes():
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs()}

def generate(output=OUT, root=ROOT):
    output.mkdir(parents=True, exist_ok=True)
    # Preserve every production body; explicitly bind Vec operations to the bounded
    # sequence contract instead of expanding std allocator/recursive drop glue.
    tree = output / 'elements'; tree.mkdir(exist_ok=True)
    for original, name, marker in [('elements.rs', 'mod.rs', 'mod dispatch;'),
                                   ('elements/dispatch.rs', 'dispatch.rs', 'use super::')]:
        text = (root / 'crates/fairing/src' / original).read_text()
        assert text.count(marker) == 1
        adapted = text.replace(marker, 'use crate::ModelVec as Vec;\n' + marker, 1)
        assert adapted.replace('use crate::ModelVec as Vec;\n', '', 1) == text
        (tree / name).write_text(adapted)
    source = (root / 'crates/fairing/src/access/protected.rs').read_text()
    # Keep the complete public metadata/binding definitions and executable methods.
    # No body rewriting or simplified authorization function is permitted.
    start = source.index('pub trait Permission:')
    end = source.index('/// An element and its mandatory permission binding.')
    binding = output / 'binding.rs'
    binding.write_text('use super::{Gate, protected_dispatch};\nuse crate::screen::Cx;\n' + source[start:end])
    harness = (root / 'verification/kani_elements.rs').read_text()
    for name, path in {
        'SESSION_RULES': root / 'crates/fairing/src/access/session_rules.rs',
        'DISPATCH': root / 'crates/fairing/src/access/protected_dispatch.rs',
        'BINDING': binding,
        'ELEMENTS': tree / 'mod.rs',
    }.items():
        assert harness.count('@' + name + '@') == 1
        harness = harness.replace('@' + name + '@', str(path))
    path = output / 'harness.rs'; path.write_text(harness)
    return path

def loop_arguments(harness, kani, env, selected=()):
    """Bound width-two iteration separately from recursive call depth.

    Discover compiler loop IDs rather than pinning mangled names. Kani may
    renumber loops when instrumenting checks; include preceding indices too.
    Unwinding assertions remain enabled for both loops and recursion.
    """
    with (harness.parent / 'codegen.log').open('w') as log:
        subprocess.run([str(kani), str(harness), '--only-codegen', '--keep-temps', *selected],
                       cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    models = [p for p in harness.parent.glob(harness.stem + '__*.out') if not p.name.endswith('.symtab.out')]
    assert models, 'Kani emitted no harness models for loop discovery'
    bounds = set()
    for model in models:
        text = subprocess.check_output([str(kani.parent / 'cbmc'), '--show-loops', str(model)], env=env, text=True)
        for identifier, location in re.findall(r'Loop ([^\n]+):\n([^\n]*)', text):
            if 'function elements::dispatch::' not in location and 'ModelIter' not in location:
                continue
            function, index = identifier.rsplit('.', 1)
            for n in range(int(index) + 1): bounds.add(function + '.' + str(n) + ':3')
    assert bounds, 'Production traversal loops were not discovered'
    return ['-Z', 'unstable-options', '--cbmc-args', '--unwindset', ','.join(sorted(bounds))]

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = OUT / 'receipt.json'; receipt.unlink(missing_ok=True)
    before = hashes(); harness = generate()
    generated = [harness, OUT / 'binding.rs', OUT / 'elements/mod.rs', OUT / 'elements/dispatch.rs']
    generated_before = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in generated}
    channel = install.PINS['kani_toolchain']
    sysroot = Path(subprocess.check_output(['rustup', 'run', channel, 'rustc', '--print', 'sysroot'], text=True).strip())
    kani = install.tool('kani')
    env = dict(os.environ, RUSTUP_TOOLCHAIN=channel)
    env['PATH'] = str(kani.parent) + os.pathsep + env.get('PATH', '')
    env['LD_LIBRARY_PATH'] = os.pathsep.join([str(sysroot / 'lib'), env.get('LD_LIBRARY_PATH', '')])
    command = [str(kani), str(harness), '-j', '2', '--output-format=terse', *loop_arguments(harness, kani, env)]
    with (OUT / 'kani.log').open('w') as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    text = (OUT / 'kani.log').read_text()
    if result.returncode or '6 successfully verified harnesses, 0 failures, 6 total' not in text:
        raise SystemExit(text[-16000:])
    assert hashes() == before, 'Element proof inputs changed during checking'
    assert generated_before == {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in generated}, 'Generated proof sources changed during checking'
    report = {'status': 'bounded-production-traversal-verified', 'inputs': before, 'command': command,
        'harnesses': 6, 'generated_inputs': generated_before, 'topology_bound': 'Complete symbolic leaf traversal and retained-tree frames; fixed nested denial/publication/requirement-drift cases use at most two leaves and two row containers. Width-two sequence adapter; unwinding assertions enabled.',
        'production': 'Unmodified production function bodies for ElementTree, dispatch traversal, PermissionBinding methods and session transition decisions, under the external contracts below.',
        'contracts': 'Sequential modeled Cx authority/grant/policy observation and callback-local resource wrapper; width-two arena sequence contract (separately checked); egui geometry allocation observer. Allocation failure and input destruction are excluded. No concrete atomics, UI hashing/clipping/rasterization, concurrency or unrestricted application code refinement.',
        'global_mediation': 'not-proved'}
    receipt.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))

if __name__ == '__main__': main()
