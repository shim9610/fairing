"""Controls for API decision completeness, aliases and mutable-loan rejection."""
import copy
import unittest
from api_decisions import build, check, compact, contracts, encode, expanded
import json


def report(items):
    return {'head': 'fixture', 'compiler': 'fixture compiler', 'extractor_sha256': 'fixture extractor',
            'profiles': {'fairing/minimal': {'items': items}}}


class DecisionControls(unittest.TestCase):
    def test_aliases_share_a_decision_but_retain_exported_paths(self):
        data = report({'fairing::Shell#use': {'kind': 'use', 'target': 'fairing::shell::Shell', 'glob': False},
                       'fairing::Shell#struct': {'kind': 'struct'},
                       'fairing::shell::Shell#struct': {'kind': 'struct'},
                       'fairing::Shell::observe#function': {'kind': 'function', 'declaration': 'observer'},
                       'fairing::shell::Shell::observe#function': {'kind': 'function', 'declaration': 'observer'}})
        folded = contracts(data)
        self.assertEqual(len(folded['fairing::shell::Shell::observe#function']['exports']['fairing/minimal']), 2)

    def test_function_reexport_does_not_rewrite_same_named_module_children(self):
        data = report({'fairing::screen#use': {'kind': 'use', 'target': 'fairing::screen::screen', 'glob': False},
                       'fairing::screen#module': {'kind': 'module'},
                       'fairing::screen#function': {'kind': 'function'},
                       'fairing::screen::screen#function': {'kind': 'function'},
                       'fairing::screen::Screen#trait': {'kind': 'trait'}})
        folded = contracts(data)
        self.assertIn('fairing::screen::Screen#trait', folded)
        self.assertIn('fairing::screen#module', folded)

    def test_recursive_namespace_alias_rejects_without_unbounded_expansion(self):
        data = report({'fixture::nested#use': {'kind': 'use', 'target': 'fixture::nested::nested', 'glob': False},
                       'fixture::nested#module': {'kind': 'module'}})
        with self.assertRaisesRegex(ValueError, 'Cyclic API alias'):
            contracts(data)

    def test_unreviewed_signature_change_is_rejected(self):
        data = report({'fairing::shell::Shell::observe#function': {'kind': 'function', 'declaration': 'observer'}})
        ledger = build(data, data)
        changed = copy.deepcopy(data)
        changed['profiles']['fairing/minimal']['items']['fairing::shell::Shell::observe#function']['declaration'] = 'mutable shell'
        with self.assertRaisesRegex(ValueError, 'lack a current decision'):
            check(ledger, changed)

    def test_normalized_storage_preserves_each_complete_decision(self):
        data = report({'fairing::shell::Shell::observe#function': {'kind': 'function', 'declaration': 'observer'}})
        ledger = build(data, data)
        self.assertEqual(expanded(compact(ledger)), ledger)
        self.assertEqual(expanded(json.loads(encode(compact(ledger)))), ledger)
        self.assertEqual(check(compact(ledger), data)['status'], 'matched')

    def test_even_a_recorded_mutable_loan_cannot_pass(self):
        data = report({'fairing::shell::Shell::desktop_mut#function': {'kind': 'function', 'declaration': 'mutable component'}})
        ledger = build(data, data)
        with self.assertRaisesRegex(ValueError, 'loan reintroduced'):
            check(ledger, data)

    def test_removed_api_requires_a_reviewed_migration(self):
        data = report({'fairing::shell::Shell::observe#function': {'kind': 'function', 'declaration': 'observer'}})
        with self.assertRaisesRegex(ValueError, 'no reviewed migration'):
            build(data, report({}))


if __name__ == '__main__':
    unittest.main()
