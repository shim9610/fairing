#!/usr/bin/env python3
"""Review tripwires for production session proof inputs; not a Rust refinement proof."""
from pathlib import Path
import re
ROOT = Path(__file__).resolve().parents[1]

def inputs():
    return [ROOT / path for path in [
        'crates/fairing/src/access/session_rules.rs', 'crates/fairing/src/access/session_shared.rs',
        'crates/fairing/src/access/session.rs', 'crates/fairing/src/access/mod.rs',
        'crates/fairing/src/access/auth.rs', 'crates/fairing/src/config.rs',
        'crates/fairing/src/shell/authentication.rs', 'crates/fairing/src/shell/navigation.rs',
        'crates/fairing/src/shell/settings_dispatch.rs', 'crates/fairing/src/shell/background.rs', 'crates/fairing/src/shell/update.rs',
        'crates/fairing/src/shell/mod.rs', 'crates/fairing/src/shell/builder.rs',
        'verification/kani_session.rs', 'verification/verus/session.py', 'verification/proofs.py', 'verification/proof_obligations.py',
        'verification/session_bindings.py', 'verification/src/session_publication.rs',
        'verification/src/sessions.rs', 'crates/fairing/tests/session_tokens.rs',
    ]]

def audit():
    texts = {str(path.relative_to(ROOT)): path.read_text() for path in inputs()}
    reader = texts['crates/fairing/src/access/session.rs']
    shared = texts['crates/fairing/src/access/session_shared.rs']
    access = texts['crates/fairing/src/access/mod.rs']
    auth = texts['crates/fairing/src/shell/authentication.rs']
    checks = {
        'opaque-reader': 'pub struct SessionToken(Arc<SessionInner>);' in reader,
        'private-writer-storage': 'pub struct SessionController {\n    inner: Arc<SessionInner>,\n    waker: Waker,\n}' in reader,
        'single-writer-capability': not re.search(r'#\[derive\([^\]]*Clone[^\]]*\)\]\s*pub struct SessionController', reader),
        'controller-disposal': 'impl Drop for SessionController' in reader and reader.count('self.inner.shared.revoke();') == 2,
        'source-bound-identity': 'super::session_rules::next_identity' in reader,
        'source-bound-transition': shared.count('session_rules::transition(') == 2,
        'revoke-event-binding': 'session_rules::transition(state, session_rules::ControlEvent::Revoke)' in shared,
        'actual-control-fields': 'revision: before,' in shared and 'revoked: self.revoked.load(Ordering::SeqCst)' in shared and 'until: self.until.load(Ordering::SeqCst)' in shared,
        'publication-epoch-stores': 'self.revision.store(before + 1, Ordering::SeqCst)' in shared and 'self.revision.store(after, Ordering::SeqCst)' in shared,
        'monotone-revocation-storage': shared.count('self.revoked.store(') == 1 and 'self.revoked.store(next.revoked, Ordering::SeqCst)' in shared,
        'sequential-consistency': set(re.findall(r'Ordering::(\w+)', shared)) == {'SeqCst'},
        'stable-reader-binding': 'before == expected' in shared and 'session_rules::observe(revoked, before, after, now, until)' in shared,
        'actual-permission-cells': 'cell.base.load(Ordering::SeqCst)' in shared and 'cell.temporary.load(Ordering::SeqCst)' in shared and 'session_rules::permission_allows(live, now, base, temporary)' in shared,
        'authority-recheck-after-policy': access.count('token_permission(') == 7,
        'local-policy-intersection': access.count('session_rules::authorize(') == 3,
        'no-one-level-bypass': '!self.token_sessions && (self.table.len() <= 1 || self.mode == AccessMode::Off)' in access,
        'token-terminal-detach': 'Err(SessionReadError::Revoked | SessionReadError::Expired)' in access and 'self.token.take()' in access,
        'request-correlation': auth.count('response.request.is_active(self.auth_request)') == 1,
        'legacy-grant-denial': 'self.access.uses_session_tokens()\n                    || !crate::access::rules::grant_allowed' in auth,
    }
    failed = [name for name, passed in checks.items() if not passed]
    if failed: raise SystemExit('Review session security wiring: ' + ', '.join(failed))
    return checks

if __name__ == '__main__':
    print(audit())
