"""Reject compiled layouts that ignore, invert or misbind the proved verdict."""
import copy
import unittest
from layout_bindings import check_plan_entries as check


def place(local, projections=None):
    return {'kind':'place','local':local,'projections':projections or []}


def plan(method):
    context=2 if method=='validate' else 3
    field={'kind':'field','id':'fairing::elements::Plan::layout','index':0}
    def fact(local, kind, operand, block=0):
        return {'block':block,'destination_value':place(local),'kind':kind,'operand_facts':[operand]}
    return {'id':'elements::Plan::'+method,'canonical_id':'fairing::elements::dispatch::Impl::'+method,
            'impl_owner':'fairing::elements::Plan','argument_count':context,
            'constructions':[
                fact(8,'reference',place(1,[{'kind':'deref'},field])),
                fact(9,'reference',place(context)),
                fact(0,'use',{'kind':'constant','literal':'const false'},2),
            ],
            'blocks':[
                {'block':0,'kind':'call','canonical_callee':'fairing::elements::layout_token::Impl::valid',
                 'argument_values':[place(8),place(9)],'destination_value':place(7),'successors':[1]},
                {'block':1,'kind':'switch','statements':[],'discriminant':'move _7',
                 'branches':[{'value':'0','target':2}],'otherwise':4,'successors':[2,4]},
                {'block':2,'kind':'goto','successors':[3]},
                {'block':3,'kind':'return','successors':[]},
                {'block':4,'kind':'return','successors':[]},
            ]}


class LayoutBindings(unittest.TestCase):
    def setUp(self):
        self.bodies={method:plan(method) for method in ['validate','paint']}

    def rejected(self, mutate, reason):
        bodies=copy.deepcopy(self.bodies)
        for method in ['validate','paint']:
            with self.subTest(method=method):
                variant=copy.deepcopy(bodies);mutate(variant[method])
                self.assertIn(reason,{e['kind'] for e in check(variant)[0]})

    def test_actual_shape_of_both_callers_is_accepted(self):
        errors,checked=check(self.bodies)
        self.assertEqual(errors,[])
        self.assertEqual(len(checked),2)

    def test_missing_guard_is_rejected(self):
        self.rejected(lambda b:b['blocks'][0].update(canonical_callee='other'),'layout-entry-guard')

    def test_entry_projected_write_is_rejected(self):
        bodies=copy.deepcopy(self.bodies)
        bodies['paint']['constructions'].append({
            'block':0,'kind':'aggregate','operand_facts':[],
            'destination_value':place(17,[{'kind':'deref'},
                {'kind':'field','id':'fairing::screen::cx::Cx::event','index':11}]),
        })
        self.assertIn('layout-entry-mutation',{e['kind'] for e in check(bodies)[0]})

    def test_wrong_token_is_rejected(self):
        self.rejected(lambda b:b['constructions'][0]['operand_facts'][0]['projections'][1].update(id='other::layout'),'layout-entry-guard')

    def test_wrong_context_is_rejected(self):
        self.rejected(lambda b:b['constructions'][1]['operand_facts'][0].update(local=1),'layout-entry-guard')

    def test_ignored_verdict_is_rejected(self):
        self.rejected(lambda b:b['blocks'][1].update(discriminant='move _6'),'layout-verdict-not-used')

    def test_inverted_verdict_is_rejected(self):
        self.rejected(lambda b:b['blocks'][1]['branches'][0].update(target=4),'layout-refusal-not-closed')

    def test_true_refusal_result_is_rejected(self):
        self.rejected(lambda b:b['constructions'][2]['operand_facts'][0].update(literal='const true'),'layout-refusal-not-closed')

    def test_effect_on_refusal_is_rejected(self):
        self.rejected(lambda b:b['blocks'][2].update(kind='call'),'layout-refusal-not-closed')

    def test_refusal_cycle_is_rejected(self):
        self.rejected(lambda b:b['blocks'][2].update(successors=[2]),'layout-refusal-not-closed')

    def test_missing_method_is_rejected(self):
        self.assertIn('layout-guard-inventory',{e['kind'] for e in check({'validate':self.bodies['validate']})[0]})



def root_body():
    def const(value):return {'kind':'constant','literal':value}
    def fact(local,kind,operand):return {'block':0,'destination_value':place(local),'kind':kind,'operand_facts':[operand]}
    return {'id':'elements::ElementTree::ui','canonical_id':'fairing::elements::Impl::ui',
            'impl_owner':'fairing::elements::ElementTree','impl_protocol':'fairing::screen::Screen','argument_count':3,
            'constructions':[fact(4,'use',const('plan')),fact(8,'reference',place(4)),
                             fact(9,'reference',place(3)),fact(10,'use',const('stamp'))],
            'blocks':[
                {'block':0,'kind':'call','canonical_callee':'fairing::elements::dispatch::Impl::validate',
                 'argument_values':[place(8),place(9),place(10)],'destination_value':place(7),'successors':[1]},
                {'block':1,'kind':'switch','statements':[],'discriminant':'move _7',
                 'branches':[{'value':'0','target':2}],'otherwise':3,'successors':[2,3]},
                {'block':2,'kind':'return','successors':[]},
                {'block':3,'kind':'call','canonical_callee':'egui::ui::Impl::allocate_exact_size',
                 'argument_values':[place(2),place(4,[{'kind':'field','id':'fairing::elements::Plan::size','index':1}]),const('sense')],
                 'destination_value':place(11),'successors':[4]},
                {'block':4,'kind':'call','canonical_callee':'fairing::elements::dispatch::Impl::paint',
                 'argument_values':[place(8),place(2),place(3),const('rect'),place(10),const('id')],
                 'destination_value':place(12),'successors':[5]},
                {'block':5,'kind':'return','successors':[]},
            ]}


class RootLayoutBindings(unittest.TestCase):
    def run_check(self, body):
        from layout_bindings import check_root
        return check_root({'root':body})

    def test_data_use_requires_true_validation(self):
        self.assertEqual(self.run_check(root_body())[0],[])

    def test_ignored_root_validation_is_rejected(self):
        body=root_body();body['blocks'][1]['branches'][0]['target']=3
        self.assertIn('layout-root-guard',{e['kind'] for e in self.run_check(body)[0]})

    def test_wrong_root_context_is_rejected(self):
        body=root_body();body['blocks'][0]['argument_values'][1]=place(2)
        self.assertIn('layout-root-context',{e['kind'] for e in self.run_check(body)[0]})

    def test_painting_another_plan_is_rejected(self):
        body=root_body();body['blocks'][4]['argument_values'][0]=place(6)
        self.assertIn('layout-root-plan-binding',{e['kind'] for e in self.run_check(body)[0]})

    def test_painting_with_another_stamp_is_rejected(self):
        body=root_body();body['blocks'][4]['argument_values'][4]={'kind':'constant','literal':'other stamp'}
        self.assertIn('layout-root-plan-binding',{e['kind'] for e in self.run_check(body)[0]})

    def test_use_cycle_requires_revalidation(self):
        body=root_body();body['blocks'][5].update(kind='goto',successors=[3])
        self.assertIn('layout-root-guard',{e['kind'] for e in self.run_check(body)[0]})

    def test_use_cycle_with_fresh_validation_is_accepted(self):
        body=root_body();body['blocks'][5].update(kind='goto',successors=[0])
        self.assertEqual(self.run_check(body)[0],[])

    def test_nested_ambiguous_plan_provenance_is_rejected(self):
        body=root_body();body['constructions'][0].update(kind='aggregate',owner='model',variant=None,operand_facts=[place(6)])
        self.assertIn('layout-root-plan-binding',{e['kind'] for e in self.run_check(body)[0]})

    def test_root_cannot_hide_data_use_in_a_new_helper(self):
        body=root_body();body['blocks'][4]['canonical_callee']='other::helper'
        self.assertIn('layout-root-unclassified-call',{e['kind'] for e in self.run_check(body)[0]})

    def test_root_cannot_mutate_fields_around_validation(self):
        body=root_body();body['constructions'][0]['destination_value']['projections']=[{'kind':'field','id':'model','index':0}]
        self.assertIn('layout-root-write-through',{e['kind'] for e in self.run_check(body)[0]})

    def test_root_verdict_cannot_be_discarded(self):
        body=root_body();body['blocks'][1]['discriminant']='move _99'
        self.assertIn('layout-root-verdict',{e['kind'] for e in self.run_check(body)[0]})


if __name__=='__main__':unittest.main()
