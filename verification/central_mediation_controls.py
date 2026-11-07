#!/usr/bin/env python3
"""Compile production mutations in an isolated checkout; require checker rejection."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

import central_mediation as c
import global_mediation as g
import mediation as m

ROOT = m.ROOT
OUT = ROOT / 'target/assurance/central-mediation/controls'


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    report = OUT / 'summary.json'; report.unlink(missing_ok=True)
    before = m.source_hashes()
    selected = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT).decode().split('\0')
    cases = [
        ('direct-screen-entry', 'crates/fairing/src/screen/mod.rs', None,
         '\npub(crate) fn proof_bypass(body: &mut dyn Screen, ui: &mut egui::Ui, cx: &mut Cx<\'_>) { body.ui(ui, cx); }\n', 'raw-protocol-reference-outside-kernel'),
        ('raw-screen-store', 'crates/fairing/src/screen/mod.rs', None,
         '\nstruct ProofBypassStore { body: Box<dyn Screen> }\n', 'raw-protocol-storage'),
        ('raw-screen-extraction', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl ScreenHandle { pub(crate) fn proof_extract(self) -> Option<Box<dyn Screen>> { self.body } }\n', 'raw-payload-return'),
        ('generic-payload-extraction', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl<T: ?Sized> Callback<T> { pub(crate) fn proof_extract(self) -> Box<T> { self.body } }\n', 'generic-payload-return'),
        ('public-body-field', 'crates/fairing/src/access/mediation.rs',
         '    body: Option<Box<dyn Screen>>,', '    pub(crate) body: Option<Box<dyn Screen>>,', 'payload-field-visibility'),
        ('unguarded-screen-adapter', 'crates/fairing/src/access/mediation.rs',
         'self.invoke(cx, |body, cx| body.$name($($arg,)* cx));',
         'if let Some(body) = &mut self.body { body.$name($($arg,)* cx); }', 'unguarded-payload-entry'),
        ('discard-inherited-scope', 'crates/fairing/src/screen/cx.rs',
         'authority_path: Some(path),', 'authority_path: None,', 'scope-context-contract'),
        ('overwrite-inherited-scope', 'crates/fairing/src/screen/cx.rs',
         '        callback(&mut scoped)', '        scoped.authority_path = None;\n        callback(&mut scoped)', 'authority-path-write'),
        ('external-erased-callback', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl ActionCallback { pub(crate) fn proof_external_bypass(&mut self, cx: &mut Cx<\'_>) { let _ = Some(cx).map(&mut *self.body); } }\n', 'unguarded-payload-entry'),
        ('external-wrapped-callback', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl ActionCallback { pub(crate) fn proof_external_bypass(&mut self, cx: &mut Cx<\'_>) { let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.body)(cx))); } }\n', 'unguarded-payload-entry'),
        ('nested-payload-extraction', 'crates/fairing/src/access/mediation.rs', None,
         '\nstruct ProofPayload { callback: Box<dyn FnMut(&mut Cx<\'_>)> }\nimpl ActionCallback { pub(crate) fn proof_extract(self) -> ProofPayload { ProofPayload { callback: self.body } } }\n', 'executable-kernel-export'),
        ('write-payload-through-app-borrow', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl ActionCallback { pub(crate) fn proof_park(self, cx: &mut Cx<\'_>) { if let Some(slot) = cx.app_mut::<Option<Box<dyn FnMut(&mut Cx<\'_>)>>>() { *slot = Some(self.body); } } }\n', 'executable-payload-write-through-borrow'),
        ('generic-helper-payload-export', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl<T: ?Sized> Callback<T> { pub(crate) fn proof_park(self, slot: &mut Option<Box<T>>) { proof_sink(self.body, slot); } }\nfn proof_sink<T: ?Sized>(body: Box<T>, slot: &mut Option<Box<T>>) { *slot = Some(body); }\n', 'executable-payload-write-through-borrow'),
        ('unclassified-generic-owner', 'crates/fairing/src/access/mediation.rs', None,
         '\nstruct ProofOpaque<T> { payload: T }\n', 'unclassified-executable-kernel-storage'),
        ('captured-payload-export', 'crates/fairing/src/access/mediation.rs', None,
         '\nimpl<T: ?Sized> Callback<T> { pub(crate) fn proof_park(self, slot: &mut Option<Box<T>>) { let _ = Some(self.body).map(|body| { *slot = Some(body); }); } }\n', 'executable-payload-write-through-borrow'),
        ('retain-untyped-global-hook', 'crates/fairing/src/shell/builder/protected.rs',
         'status_bar_painter: None, status_bar_layout: None,',
         'status_bar_painter: Some(Box::new(|_, _| {})), status_bar_layout: None,', 'untyped-hook-not-cleared'),
        ('expose-construction-capability', 'crates/fairing/src/shell/builder/protected.rs',
         'struct CallbackFreeWallpaper(Wallpaper);', 'struct CallbackFreeWallpaper(pub(crate) Wallpaper);', 'construction-capability-field-visible'),
        ('restore-global-icon-alias', 'crates/fairing/src/screen/cx.rs',
         'icons: &mut icons,', 'icons: &mut *self.icons,', 'scoped-resource-aliases-global'),
        ('wrong-context-grant', 'crates/fairing/src/screen/cx.rs',
         'self.access.allows(gate)', 'self.access.allows(&Gate::borrowed("public"))', 'context-observer-binding'),
        ('hide-protected-mode', 'crates/fairing/src/access/mod.rs',
         '        self.mediation_required\n', '        false\n', 'authority-boolean-observer-binding'),
        ('scope-constructor-installs-callback', 'crates/fairing-widgets/src/widgets/painters.rs',
         '    pub fn new() -> Self {\n        Self::default()\n    }',
         '    pub fn new() -> Self {\n        Self::default().button(|_, _| {})\n    }', 'scoped-resource-aliases-global'),
    ]
    results = []
    with tempfile.TemporaryDirectory(prefix='central-controls-', dir=ROOT / 'target') as temporary:
        checkout = Path(temporary)
        for name in selected:
            if not name or not (name.startswith(('crates/', 'verification/', 'xtask/', '.cargo/')) or name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')):
                continue
            source = ROOT / name
            if source.is_file():
                destination = checkout / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, destination)
        m.ROOT = checkout
        m.OUT = checkout / 'target/mediation'
        m.OUT.mkdir(parents=True)
        # Reuse compiled dependencies, never mutate the user's source checkout.
        (m.OUT / 'build').symlink_to(ROOT / 'target/mediation/build', target_is_directory=True)
        for label, file, old, new, expected in cases:
            path = checkout / file
            original = path.read_text()
            if old is None:
                changed = original + new
            else:
                assert original.count(old) == 1, 'Production mutation changed shape: ' + label
                changed = original.replace(old, new, 1)
            path.write_text(changed)
            try:
                try:
                    m.build('all')
                except subprocess.CalledProcessError:
                    compiler_log = m.OUT / 'all/compiler.log'
                    if compiler_log.exists():
                        shutil.copy2(compiler_log, OUT / (label + '-compiler.log'))
                    raise
                m.graph('all')
                data = [json.loads((m.OUT / 'all' / (crate + '.json')).read_text()) for crate in m.CRATES]
                outcome = c.analyze(data)
                capabilities = g.capability_check(data)
                exports, _ = g.payload_boundaries(data, outcome)
                outcome['connected_errors'] = capabilities['errors'] + exports
                kinds = {error['kind'] for error in outcome['errors']}
                kinds.update(error['kind'] for error in outcome['connected_errors'])
                if outcome['unguarded_payload_entries']:
                    kinds.add('unguarded-payload-entry')
                assert (outcome['status'] == 'rejected' or outcome['connected_errors']) and expected in kinds, (label, outcome)
                (OUT / (label + '.json')).write_text(json.dumps(outcome, indent=2) + '\n')
                shutil.copy2(m.OUT / 'all/compiler.log', OUT / (label + '-compiler.log'))
                results.append({'case': label, 'status': 'compiled-and-rejected', 'reason': expected})
                print(label + ': compiled and rejected', flush=True)
            finally:
                path.write_text(original)
    m.ROOT = ROOT; m.OUT = ROOT / 'target/mediation'
    assert before == m.source_hashes(), 'User source changed while checking mutations'
    report.write_text(json.dumps({'inputs': before, 'controls': results}, indent=2) + '\n')


if __name__ == '__main__': main()
