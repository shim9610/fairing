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
    'crates/fairing/src/access/protected_dispatch.rs',
    'verification/central_proofs.py',
)
PROOF_INPUTS = {
    'shared-production-admission-kernel-verified': KERNEL_INPUTS,
    'unbounded-production-path-verified': PATH_INPUTS,
}
