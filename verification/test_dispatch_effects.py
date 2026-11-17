"""Check admission consumption through helpers, nested dispatch and recursion."""
import unittest
from dispatch_effects import check


def body(*blocks):
    return {'blocks': [{'block': i, 'kind': kind, 'successors': targets}
                       for i, (kind, targets) in enumerate(blocks)]}


class DispatchEffects(unittest.TestCase):
    def test_normal_helper_preserves_two_entries(self):
        bodies = {'bridge': body(('call', [1]), ('return', [])),
                  'helper': body(('call', [1]), ('call', [2]), ('return', []))}
        self.assertTrue(check(bodies, {('bridge', 0): {('helper', 0)}},
                              {('helper', 0), ('helper', 1)}, {}, {'bridge'}))

    def test_nested_dispatch_does_not_restore_callers_old_authority(self):
        bodies = {'bridge': body(('call', [1]), ('call', [2]), ('return', [])),
                  'operation': body(('call', [1]), ('return', []))}
        self.assertTrue(check(bodies, {}, {('operation', 0), ('bridge', 1)},
                              {('bridge', 0): {('operation', 0)}}, {'bridge'}))

    def test_fresh_nested_dispatch_after_an_entry_is_valid(self):
        bodies = {'bridge': body(('call', [1]), ('call', [2]), ('return', [])),
                  'operation': body(('call', [1]), ('return', []))}
        self.assertFalse(check(bodies, {}, {('operation', 0), ('bridge', 0)},
                               {('bridge', 1): {('operation', 0)}}, {'bridge'}))

    def test_recursion_retains_a_nonreturning_violation(self):
        bodies = {'bridge': body(('call', [1]), ('return', [])),
                  'helper': body(('call', [1]), ('call', [2]), ('return', []))}
        calls = {('bridge', 0): {('helper', 0)}, ('helper', 1): {('helper', 0)}}
        self.assertTrue(check(bodies, calls, {('helper', 0)}, {}, {'bridge'}))

    def test_pure_helper_is_not_a_payload(self):
        bodies = {'bridge': body(('call', [1]), ('call', [2]), ('return', [])),
                  'helper': body(('return', []))}
        self.assertFalse(check(bodies, {('bridge', 0): {('helper', 0)}},
                               {('bridge', 1)}, {}, {'bridge'}))

    def test_policy_callback_requires_fresh_admission_after_it(self):
        bodies = {'bridge': body(('call', [1]), ('call', [2]), ('return', []))}
        self.assertTrue(check(bodies, {}, {('bridge', 1)}, {}, {'bridge'}, {('bridge', 0)}))


if __name__ == '__main__':
    unittest.main()
