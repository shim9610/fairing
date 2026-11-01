#!/usr/bin/env python3
"""Require real stock-egui OOM to kill only the supervised renderer, not its engine."""
import json
import os
from pathlib import Path
import resource
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/process'
OUT.mkdir(parents=True, exist_ok=True)
TARGET = ROOT / 'target/process-isolation'
ENV = dict(os.environ, CARGO_TARGET_DIR=str(TARGET), CARGO_INCREMENTAL='0',
           FAIRING_HEAP_BYTES=str(16 * 1024 * 1024))
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def logged(name, command, timeout=600):
    path = OUT / (name + '.log')
    with path.open('w') as output:
        result = subprocess.run(command, cwd=ROOT, env=ENV, stdout=output,
                                stderr=subprocess.STDOUT, timeout=timeout)
    if result.returncode:
        print(path.read_text()[-12000:])
        raise SystemExit(f'Process-isolation check failed: {name}')
    return path


logged('build', ['cargo', 'build', '--locked', '-p', 'fairing-fixed-heap',
                 '--example', 'isolated_renderer', '--example', 'renderer_boundary', '--features', 'global'])
logged('host-build', ['cargo', 'build', '--locked', '-p', 'fairing-process', '--examples'])
logged('native', ['cargo', 'test', '--locked', '-p', 'fairing-process'])
results = []
for stage in ['ui', 'tessellate']:
    # Negative control: unchanged egui cannot recover from real exhausted-arena allocations.
    result = subprocess.run([str(TARGET / 'debug/examples/renderer_boundary'), stage],
                            cwd=ROOT, env=ENV, capture_output=True, text=True, timeout=10)
    (OUT / ('baseline-' + stage + '.log')).write_text(result.stdout + result.stderr)
    if result.returncode != -6 or 'memory allocation' not in result.stderr or 'boundary-enter' not in result.stdout:
        raise SystemExit(f'Expected an actual egui allocation abort: {stage}')
    results.append({'negative_control': stage, 'exit_code': result.returncode})
for mode in ['normal', 'ui-oom-once', 'tessellate-oom-once', 'hang-once', 'context',
             'start-hang', 'oversize', 'partial', 'replay', 'bad-context', 'flood']:
    path = logged(mode, [str(TARGET / 'debug/examples/supervisor_probe'),
                        str(TARGET / 'debug/examples/isolated_renderer'), mode], timeout=12)
    report = json.loads(path.read_text())
    if report['parent_threads'] > 2 or report['child_threads'] > 2:
        raise SystemExit(f'Unbounded fixture thread count: {mode}')
    if report['child_virtual_kib'] > 128 * 1024:
        raise SystemExit(f'Child address-space limit exceeded: {mode}')
    results.append(report)
    print(f'{mode}: engine continued; renderer lifecycle/intent checks passed', flush=True)
(OUT / 'summary.json').write_text(json.dumps({'scope': 'Linux x86_64 headless actual egui; not board/GPU certification',
    'registry': 'unchanged egui/epaint 0.37.0', 'arena_bytes': 16 * 1024 * 1024,
    'child_address_limit': 128 * 1024 * 1024, 'results': results}, indent=2) + '\n')
