#!/usr/bin/env python3
"""Compile workspace production targets and reject effectful permission metadata MIR.

This checks declaration types and a conservative metadata purity/termination subset.
It does not certify callback mediation or arbitrary application implementation bodies.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def check(manifest, output, extra=()):
    manifest = manifest.resolve(strict=True)
    output.mkdir(parents=True, exist_ok=True)
    (output / 'report.json').unlink(missing_ok=True)
    driver_paths = [ROOT / 'verification/mediation_driver.rs', ROOT / 'verification/metadata_driver.rs', ROOT / 'verification/mediation_schema.rs', Path(__file__), ROOT / 'verification/tools.json']
    def hashes(paths):
        return {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(set(paths))}
    driver_before = hashes(driver_paths)
    channel = json.loads((ROOT / 'verification/tools.json').read_text())['kani_toolchain']
    sysroot = Path(subprocess.check_output(['rustup', 'run', channel, 'rustc', '--print', 'sysroot'], text=True).strip())
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(output / 'build'), CARGO_TERM_COLOR='never')
    if env.get('RUSTC_WORKSPACE_WRAPPER'):
        raise SystemExit('An existing RUSTC_WORKSPACE_WRAPPER must be composed explicitly; it cannot be silently replaced')
    env['LD_LIBRARY_PATH'] = os.pathsep.join([str(sysroot / 'lib'), str(sysroot / 'lib/rustlib/x86_64-unknown-linux-gnu/lib'), env.get('LD_LIBRARY_PATH', '')])
    driver = output / 'metadata-driver'
    subprocess.run(['rustup', 'run', channel, 'rustc', str(ROOT / 'verification/mediation_driver.rs'), '-o', str(driver), '-C', 'prefer-dynamic', '-C', 'rpath', '-L', f'native={sysroot / "lib"}'], env=env, check=True)
    command = ['cargo', '+' + channel]
    metadata = json.loads(subprocess.check_output(command + ['metadata', '--locked', '--no-deps', '--format-version', '1', '--manifest-path', str(manifest)], env=env, text=True))
    members = set(metadata['workspace_members'])
    workspace = [p for p in metadata['packages'] if p['id'] in members]
    packages = [p['name'] for p in workspace]
    paths = [Path(metadata['workspace_root']) / 'Cargo.lock']
    for package in workspace:
        package_manifest = Path(package['manifest_path'])
        paths.append(package_manifest)
        for directory, dirs, files in os.walk(package_manifest.parent):
            dirs[:] = [name for name in dirs if name not in {'target', '.git'}]
            paths.extend(Path(directory) / name for name in files if name.endswith('.rs'))
    source_before = hashes(path for path in paths if path.exists())
    clean = command + ['clean', '--manifest-path', str(manifest)]
    for package in packages:
        clean += ['-p', package]
    subprocess.run(clean, env=env, check=True, stdout=subprocess.DEVNULL)
    env.update(RUSTC_WORKSPACE_WRAPPER=str(driver), FAIRING_CHECK_METADATA='1')
    env.pop('FAIRING_MEDIATION_OUTPUT', None)
    invocation = command + ['check', '--locked', '--workspace', '--manifest-path', str(manifest), *extra]
    with (output / 'compiler.log').open('w') as log:
        result = subprocess.run(invocation, env=env, stdout=log, stderr=subprocess.STDOUT)
    log = (output / 'compiler.log').read_text()
    prefix = 'FAIRING_METADATA_CHECKED '
    records = [json.loads(line.split(prefix, 1)[1]) for line in log.splitlines() if line.startswith(prefix)]
    count = sum(len(record['methods']) for record in records)
    status = 'metadata-contracts-checked' if result.returncode == 0 and count else 'rejected' if result.returncode else 'no-implementations'
    report = {'status': status, 'command': invocation, 'compiler_exit_code': result.returncode, 'checked_methods': count, 'crates': records,
              'driver_inputs': driver_before, 'workspace_inputs': source_before,
              'scope': 'Compiled production-target workspace Permission/PermissionBound implementations and statically available helper MIR. Restricted purity/termination and ordinary Rust type conformance only. Dependency implementations outside the workspace, callback coverage, business semantics and global mediation are not certified.'}
    if hashes(driver_paths) != driver_before or hashes(Path(path) for path in source_before) != source_before:
        raise SystemExit('Metadata checker inputs changed during compilation; rebuild before using the result')
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    if status != 'metadata-contracts-checked':
        print(log[-16000:])
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest-path', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/metadata-check')
    parser.add_argument('--all-features', action='store_true')
    parser.add_argument('--no-default-features', action='store_true')
    parser.add_argument('--features')
    args = parser.parse_args()
    extra = (['--all-features'] if args.all_features else []) + (['--no-default-features'] if args.no_default_features else []) + (['--features', args.features] if args.features else [])
    report = check(args.manifest_path, args.output.resolve(), extra)
    print(json.dumps({k: report[k] for k in ['status', 'checked_methods', 'scope']}, indent=2))
    raise SystemExit(0 if report['status'] == 'metadata-contracts-checked' else 1)


if __name__ == '__main__':
    main()
