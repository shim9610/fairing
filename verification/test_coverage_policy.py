"""Negative coverage controls require actual execution, not a nonzero line count."""
from copy import deepcopy
import unittest
from coverage_policy import validate


class CoveragePolicy(unittest.TestCase):
    def test_missing_one_line_and_function_regressions_fail(self):
        floors = {'kernel.rs': {'lines': 80, 'functions': 100}}
        valid = {'kernel.rs': {'lines': {'count': 10, 'covered': 8},
                               'functions': {'count': 4, 'covered': 4}}}
        validate(valid, floors)
        for metric, covered in [('lines', 1), ('lines', 7), ('functions', 3)]:
            bad = deepcopy(valid)
            bad['kernel.rs'][metric]['covered'] = covered
            bad['kernel.rs'][metric]['percent'] = 100
            with self.subTest(metric=metric, covered=covered), self.assertRaises(ValueError):
                validate(bad, floors)
        with self.assertRaises(ValueError):
            validate({}, floors)
        with self.assertRaises(ValueError):
            validate(valid, {})
        with self.assertRaises(ValueError):
            validate(valid, floors, ['new-mediation-module.rs'])


if __name__ == '__main__':
    unittest.main()
