"""Negative evidence controls, run both normally and with python -O."""
import ast
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import mediation as m
import protected_contracts as p
from validation import PROOF_INPUTS


class EvidenceValidation(unittest.TestCase):
    def test_proof_receipts_reject_invalid_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(m, 'ROOT', root):
                for status, names in PROOF_INPUTS.items():
                    for name in names:
                        source = root / name
                        source.parent.mkdir(parents=True, exist_ok=True)
                        source.write_text('current source')
                    inputs = {name: m.digest(root / name) for name in names}
                    receipt = root / 'receipt.json'
                    receipt.write_text(json.dumps({'status': status, 'inputs': inputs}))
                    self.assertEqual(p.receipt(receipt, status)['status'], status)
                    for label, data in (
                        ('failed proof', {'status': 'rejected', 'inputs': inputs}),
                        ('stale source', {'status': status, 'inputs': dict.fromkeys(names, '0' * 64)}),
                        ('empty inputs', {'status': status, 'inputs': {}}),
                        ('partial inputs', {'status': status, 'inputs': {names[0]: inputs[names[0]]}}),
                        ('unexpected input', {'status': status, 'inputs': inputs | {'extra': 'x'}}),
                    ):
                        with self.subTest(status=status, case=label):
                            receipt.write_text(json.dumps(data))
                            with self.assertRaises(AssertionError):
                                p.receipt(receipt, status)

    def test_empty_or_unknown_profile_cannot_certify_coverage(self):
        for profiles in ((), ('unknown',), ('all', 'all')):
            with self.subTest(profiles=profiles), self.assertRaises(AssertionError):
                p.verify(profiles=profiles)

    def test_acceptance_checks_cannot_be_removed_by_optimization(self):
        # Rust assertions inside source strings are proof obligations, not Python AST assertions.
        for name in ('mediation.py', 'central_mediation.py', 'central_proofs.py',
                     'protected_contracts.py', 'mediation_controls.py',
                     'contract_controls.py', 'metadata_controls.py', 'verus/mediation_path.py'):
            with self.subTest(module=name):
                tree = ast.parse((Path(__file__).parent / name).read_text())
                self.assertFalse(any(isinstance(node, ast.Assert) for node in ast.walk(tree)))


if __name__ == '__main__':
    unittest.main()
