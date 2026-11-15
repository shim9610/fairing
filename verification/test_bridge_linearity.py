"""Interprocedural bridge controls, including mutually exclusive payload calls."""
from collections import defaultdict
import unittest

from central_mediation import KERNEL, bridge_linearity


class BridgeLinearity(unittest.TestCase):
    def check_graph(self, successors, calls, sinks, forwarding=None):
        bodies = {}
        adjacency, targets = defaultdict(set), defaultdict(set)
        for name, blocks in successors.items():
            bodies[name] = {'canonical_id': KERNEL + name, 'closure': True,
                            'blocks': [{'block': i, 'successors': dest} for i, dest in enumerate(blocks)]}
            for i, dest in enumerate(blocks):
                adjacency[name, i].update((name, j) for j in dest)
        for node, callees in calls.items():
            targets[node].update(callees)
            adjacency[node].update(callees)
        return {e['kind'] for e in bridge_linearity(bodies, adjacency, targets, sinks, forwarding or {})}

    def test_once_forwarding_is_counted_across_nested_closures(self):
        graph = {'outer': [[1], [2], []], 'first': [[]], 'second': [[]]}
        calls = {('outer', 0): {('first', 0)}, ('outer', 1): {('second', 0)}}
        self.assertIn('multiple-protected-entries', self.check_graph(
            graph, calls, {('first', 0), ('second', 0)}))

    def test_exclusive_branches_are_not_duplicate_execution(self):
        self.assertEqual(self.check_graph({'outer': [[1, 2], [3], [3], []]}, {},
                                         {('outer', 1), ('outer', 2)}), set())

    def test_loop_around_forwarded_closure_is_rejected(self):
        self.assertIn('kernel-adapter-cycle', self.check_graph(
            {'outer': [[1], [0, 2], []], 'inner': [[]]},
            {('outer', 0): {('inner', 0)}}, {('inner', 0)}))

    def test_unknown_external_consumer_cannot_hide_repetition(self):
        for target in ('core::iter::Iterator::for_each', 'third_party::consume', None):
            with self.subTest(target=target):
                self.assertIn('unknown-bridge-forwarding', self.check_graph(
                    {'outer': [[]], 'inner': [[]]}, {('outer', 0): {('inner', 0)}},
                    {('inner', 0)}, {('outer', 0): target}))


if __name__ == '__main__':
    unittest.main()
