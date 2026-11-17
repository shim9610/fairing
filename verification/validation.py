"""Assurance acceptance checks that remain active under Python optimization."""


def require(condition, message):
    """Reject invalid evidence even when Python removes assert statements."""
    if not condition:
        raise AssertionError(message)


PATH_INPUTS = (
    'crates/fairing/src/access/mediation.rs',
    'verification/verus/mediation_path.py',
    'verification/validation.py',
    'verification/tools.json',
)
KERNEL_INPUTS = PATH_INPUTS + (
    'crates/fairing/src/access/mediation/contract.rs',
    'crates/fairing/src/access/mediation/execution.rs',
    'crates/fairing/src/access/protected_dispatch.rs',
    'verification/central_proofs.py',
)
LAYOUT_INPUTS = (
    'crates/fairing/src/elements/layout_token.rs',
    'crates/fairing/src/elements/dispatch.rs',
    'crates/fairing/src/elements.rs',
    'crates/fairing/src/access/protected.rs',
    'crates/fairing/src/access/mod.rs',
    'crates/fairing/src/access/session.rs',
    'crates/fairing/src/access/session_rules.rs',
    'crates/fairing/src/screen/cx.rs',
    'verification/layout_proofs.py',
    'verification/verus/mediation_path.py',
    'verification/validation.py',
    'verification/tools.json',
)
PROOF_INPUTS = {
    'production-layout-token-verified': LAYOUT_INPUTS,
    'shared-production-admission-kernel-verified': KERNEL_INPUTS,
    'unbounded-production-path-verified': PATH_INPUTS,
}
