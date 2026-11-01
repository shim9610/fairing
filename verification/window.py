#!/usr/bin/env python3
"""Smoke-test a real eframe window across private process IPC; run inside xvfb-run."""
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/process'
OUT.mkdir(parents=True, exist_ok=True)
TARGET = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
if not TARGET.is_absolute(): TARGET = ROOT / TARGET
ENV = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(TARGET))
with (OUT / 'window-build.log').open('w') as log:
    for command in [
        ['cargo', 'build', '--locked', '-p', 'fairing', '--example', 'process_renderer', '--features', 'runner-x11,process'],
        ['cargo', 'build', '--locked', '-p', 'fairing-process', '--example', 'work_engine'],
    ]:
        subprocess.run(command, cwd=ROOT, env=ENV, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)


def memory(pid):
    try:
        status = (Path('/proc') / str(pid) / 'status').read_text()
        def value(key):
            return next((int(line.split()[1]) for line in status.splitlines() if line.startswith(key)), 0)
        return [value('VmRSS:'), value('VmSize:'), value('Threads:')]
    except FileNotFoundError:
        return [0, 0, 0]


with (OUT / 'window-stderr.log').open('w') as errors:
    engine = subprocess.Popen([str(TARGET / 'debug/examples/work_engine'), str(TARGET / 'debug/examples/process_renderer'), '3'],
                              cwd=ROOT, env=ENV, stdout=subprocess.PIPE, stderr=errors, text=True)
    try:
        # The fixture prints its child's PID once, before ordinary runtime sampling.
        first = engine.stdout.readline()
        match = re.fullmatch(r'renderer_pid=(\d+)\n', first)
        if not match:
            raise SystemExit('Window fixture did not start a renderer')
        child_pid = int(match[1])
        peaks = {'engine': [0, 0, 0], 'renderer': [0, 0, 0]}
        deadline = time.monotonic() + 10
        while engine.poll() is None:
            if time.monotonic() >= deadline:
                raise SystemExit('Window fixture timed out')
            for name, pid in [('engine', engine.pid), ('renderer', child_pid)]:
                peaks[name] = [max(a, b) for a, b in zip(peaks[name], memory(pid))]
            time.sleep(0.005)
        output = first + engine.stdout.read()
        if engine.returncode or 'state: Running' not in output or 'failures: 0' not in output:
            raise SystemExit('Window IPC/model readiness failed: ' + output)
        if not re.search(r'last_ready_sequence: [1-9]\d*', output) or not peaks['renderer'][0]:
            raise SystemExit('No applied model/renderer memory observation')
        report = {'scope': 'Linux x86_64 debug, Xvfb/Mesa, LP_NUM_THREADS=1; X server memory excluded; not a device benchmark',
                  'units': ['rss_kib', 'virtual_kib', 'threads'], 'peaks': peaks,
                  'output': output, 'exit_code': engine.returncode}
        (OUT / 'window.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report))
    finally:
        if engine.poll() is None:
            engine.kill()
            engine.wait(timeout=5)
