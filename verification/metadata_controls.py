#!/usr/bin/env python3
"""Compile real downstream implementations; reject metadata effects and divergence."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

import check_extension as checker

ROOT = checker.ROOT
OUT = ROOT / 'target/metadata-controls'

HEADER = '''use fairing::access::{Permission, PermissionBound};
#[derive(Clone, Copy)]
enum Rights { Read, Edit }
impl Permission for Rights {
    fn gate(self) -> fairing::Gate {
        fairing::Gate::borrowed(match self { Self::Read => "read", Self::Edit => "edit" })
    }
}
struct Customer(Rights);
'''
PURE = '''impl PermissionBound for Customer {
    type Required = Rights;
    fn required_permission(&self) -> Rights { self.0 }
}
'''


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    results = []
    with tempfile.TemporaryDirectory(prefix='fairing-metadata-client-') as temporary:
        project = Path(temporary)
        (project / 'src').mkdir()
        (project / 'Cargo.toml').write_text('[package]\nname="metadata-client"\nversion="0.0.0"\nedition="2021"\n[workspace]\n[dependencies]\nfairing={path=' + json.dumps(str(ROOT / 'crates/fairing')) + ',default-features=false}\n')
        shutil.copyfile(ROOT / 'Cargo.lock', project / 'Cargo.lock')
        (project / 'src/lib.rs').write_text(HEADER + PURE)
        channel = json.loads((ROOT / 'verification/tools.json').read_text())['kani_toolchain']
        version = subprocess.check_output(['rustup', 'run', channel, 'rustc', '-vV'], text=True)
        host = next(line.removeprefix('host: ') for line in version.splitlines() if line.startswith('host: '))
        with (OUT / 'resolve.log').open('w') as log:
            # Resolve the seeded lock without fetching unused Redox/Windows/etc.
            # sources absent from a clean Linux runner's registry cache.
            resolved = subprocess.run(['cargo', '+' + channel, 'metadata', '--offline', '--filter-platform', host,
                                       '--format-version', '1', '--manifest-path', str(project / 'Cargo.toml')],
                                      stdout=log, stderr=subprocess.STDOUT)
        if resolved.returncode:
            raise SystemExit((OUT / 'resolve.log').read_text()[-12000:])
        cases = [
            ('pure-enum-and-field', HEADER + PURE, True),
            ('metadata-io', HEADER + PURE.replace('self.0 }', 'let _ = std::fs::write("metadata-must-not-run", b"x"); self.0 }'), False),
            ('transitive-io', HEADER + 'fn hidden() { let _ = std::fs::write("metadata-must-not-run", b"x"); }\n' + PURE.replace('self.0 }', 'hidden(); self.0 }'), False),
            ('metadata-loop', HEADER + PURE.replace('self.0 }', 'loop {} }'), False),
            ('metadata-static-write', HEADER + 'static mut CHANGED: u8 = 0;\n' + PURE.replace('self.0 }', 'unsafe { CHANGED = 1; } self.0 }'), False),
            ('mapper-effect', HEADER.replace('fairing::Gate::borrowed(match self', 'let _ = std::fs::write("metadata-must-not-run", b"x"); fairing::Gate::borrowed(match self') + PURE, False),
            ('missing-declaration', HEADER + 'impl PermissionBound for Customer { type Required = Rights; }\n', False),
        ]
        for name, source, accepted in cases:
            (project / 'src/lib.rs').write_text(source)
            report = checker.check(project / 'Cargo.toml', OUT / 'work')
            log = (OUT / 'work/compiler.log').read_text()
            actual = report['status'] == 'metadata-contracts-checked'
            assert actual == accepted, (name, report)
            if accepted:
                assert report['checked_methods'] == 2, report
            elif name != 'missing-declaration':
                assert 'error: FAIRING_METADATA_EFFECT:' in log, (name, log[-4000:])
            else:
                assert 'required_permission' in log and 'not all trait items implemented' in log
            assert not (ROOT / 'metadata-must-not-run').exists()
            shutil.copyfile(OUT / 'work/compiler.log', OUT / (name + '.log'))
            (OUT / (name + '.json')).write_text(json.dumps(report, indent=2) + '\n')
            results.append({'case': name, 'expected': 'accepted' if accepted else 'rejected', 'actual': report['status'], 'checked_methods': report['checked_methods']})
            print(name + (': accepted' if accepted else ': rejected'), flush=True)
    (OUT / 'summary.json').write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
