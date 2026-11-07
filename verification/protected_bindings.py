#!/usr/bin/env python3
"""Source wiring tripwires supporting component receipts; compiler confinement is checked separately."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def inputs():
    return [ROOT / path for path in [
        'crates/fairing/src/access/protected.rs',
        'crates/fairing/src/access/protected_dispatch.rs',
        'crates/fairing/src/access/protected_tests.rs',
        'crates/fairing/src/access/mod.rs',
        'crates/fairing/src/access/session.rs',
        'crates/fairing/src/screen/cx.rs',
        'crates/fairing/src/shell/protected.rs',
        'crates/fairing/src/shell/builder.rs',
        'crates/fairing/src/overlay/tiles.rs',
        'crates/fairing/src/elements.rs', 'crates/fairing/src/elements/dispatch.rs',
        'crates/fairing/src/elements/tests.rs', 'crates/fairing/src/access/mediation.rs', 'crates/fairing/src/access/mediation/contract.rs',
        'crates/fairing/src/access/mediation/declared.rs', 'crates/fairing/src/access/mediation/element.rs',
        'crates/fairing/tests/permission_values.rs',
        'verification/kani_protected.rs', 'verification/verus/protected.py',
        'verification/protected_bindings.py', 'verification/proofs.py',
        'verification/src/typed_callbacks.rs',
        'verification/proof_obligations.py',
    ]]

def audit():
    p = (ROOT / 'crates/fairing/src/access/protected.rs').read_text()
    mediation = (ROOT / 'crates/fairing/src/access/mediation.rs').read_text()
    shell = (ROOT / 'crates/fairing/src/shell/protected.rs').read_text()
    builder = (ROOT / 'crates/fairing/src/shell/builder.rs').read_text()
    cx = (ROOT / 'crates/fairing/src/screen/cx.rs').read_text()
    checks = {
        'private-binding-and-body': 'binding: PermissionBinding<P>,' in p and 'body: T,' in p,
        'fresh-calling-context': 'cx.token_mode() && cx.allows_gate(&self.gate)' in p,
        'actual-dispatch': 'protected_dispatch::admitted(cx.token_mode(), permission)' in mediation and 'let allowed = admission.allows(cx);' in mediation and 'protected_dispatch::invoke(allowed, || {' in mediation,
        'no-stored-admission': 'required: P,' in p and 'gate: Gate,' in p and 'allowed: bool,' not in p,
        'factory-recheck': 'body.as_mut()' in ''.join(mediation.split()) and mediation[mediation.index('impl ScreenHandle {'):].count('invoke(requirement.admission(), cx, |cx|') == 2,
        'application-owned-value': 'fn gate(self) -> Gate;' in p and 'fn required_permission(&self) -> Self::Required;' in p and 'const GATE:' not in p,
        'automatic-element-registration': 'pub fn add_elements<P: Permission>' in shell and 'self.add_screen(id, tree);' in shell,
        'mandatory-registration': 'element: Protected<P, D>' in shell and 'self.inner.add(element.into_decl());' in shell,
        'no-mutable-shell-escape': 'inner: Shell,' in shell and '-> &mut Shell' not in shell and 'Deref' not in shell,
        'protected-builder-rejects-hooks': 'self.validate_protected_hooks()?;' in builder and 'build_protected rejects untyped hook' in builder,
        'callback-local-extension-stores': all(s in cx for s in ['fn with_protected_resources', 'let mut icons = IconSet::new();', 'icons: &mut icons,', 'widget_painters: &mut painters,']),
        'recursive-wallpaper-review': 'wallpaper_has_callback(dark) || wallpaper_has_callback(light)' in builder,
        'token-mode-build': 'if !self.config.access.session_tokens {' in builder and 'self.build(ctx).map(super::ProtectedShell::new)' in builder,
        'tile-panel-guard': 'body.into_inner().required(binding)' in (ROOT / 'crates/fairing/src/overlay/tiles.rs').read_text(),
    }
    failed = [name for name, passed in checks.items() if not passed]
    if failed:
        raise SystemExit('Review typed callback wiring: ' + ', '.join(failed))
    return checks

if __name__ == '__main__':
    print(audit())
