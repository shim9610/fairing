"""Regression controls for the compiler API inventory's namespace and attribute handling."""
import copy
import unittest
from api_surface import inventory


class InventoryControls(unittest.TestCase):
    def fixture(self):
        def item(name, inner, visibility='public'):
            return {'name': name, 'inner': inner, 'crate_id': 0, 'attrs': [], 'visibility': visibility}
        return {'root': 0, 'paths': {}, 'index': {
            '0': item('fixture', {'module': {'items': [1]}}),
            '1': item('Grant', {'struct': {'kind': {'plain': {'fields': [2]}}, 'impls': [3], 'generics': {}}}),
            '2': item('base', {'struct_field': {'primitive': 'bool'}}),
            '3': item(None, {'impl': {'trait': None, 'items': [4]}}),
            '4': item('base', {'function': {'sig': {'output': {'generic': 'Self'}}}}),
        }}

    def test_field_and_factory_with_the_same_name_are_both_retained(self):
        records = inventory(self.fixture())
        self.assertIn('fixture::Grant::base#struct_field', records)
        self.assertIn('fixture::Grant::base#function', records)
        self.assertEqual(len(records), 4)

    def test_line_only_cfg_traces_do_not_hide_real_attribute_changes(self):
        original = self.fixture()
        changed = copy.deepcopy(original)
        changed['index']['1']['attrs'] = [{'other': '#[attr = CfgTrace(span: new.rs:42:1)]'}]
        self.assertEqual(inventory(original), inventory(changed))
        changed['index']['1']['attrs'].append({'other': '#[non_exhaustive]'})
        self.assertNotEqual(inventory(original), inventory(changed))


if __name__ == '__main__':
    unittest.main()
