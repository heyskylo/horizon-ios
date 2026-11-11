"""Per-operation presence gates keep raw random counts and strict defaults."""
import copy
import unittest

from tools.test_command_exec_parity import compare


class CommandComparisonTests(unittest.TestCase):
    @staticmethod
    def record(count, accepted=True):
        return {'cmd': '/count', 'messages': [[[str(count), [None, False, False, False, False, False]]]],
                'results': [[accepted, count]]}

    def test_only_explicit_presence_accepts_different_positive_counts(self):
        left, right = self.record(7), self.record(19)
        originals = copy.deepcopy((left, right))
        self.assertIsNotNone(compare(left, right))
        self.assertIsNone(compare(left, right, 'positive_result'))
        self.assertEqual((left, right), originals)

    def test_presence_detects_missing_fire_and_accepts_empty_controls(self):
        positive, empty = self.record(7), self.record(0, False)
        self.assertIsNotNone(compare(positive, empty, 'positive_result'))
        self.assertIsNotNone(compare(empty, positive, 'positive_result'))
        self.assertIsNone(compare(empty, empty, 'positive_result'))

    def test_malformed_callbacks_cannot_pass_as_empty_counts(self):
        empty = self.record(0, False)
        for results in ([], [[False, 7]], [[True, -1]], [[True, True]], [[True, 1], [True, 2]]):
            bad = {**empty, 'results': results}
            self.assertIsNotNone(compare(bad, bad, 'positive_result'))

    def test_unknown_comparison_fails_loudly(self):
        with self.assertRaises(ValueError):
            compare(self.record(1), self.record(1), 'typo')

    def test_domain_probe_keeps_failure_feedback_and_callbacks_exact(self):
        valid = {'cmd': '/execute unless data entity UUID VillagerData run data get entity UUID VillagerData.profession',
                 'messages': [], 'results': []}
        self.assertIsNone(compare(valid, copy.deepcopy(valid)))
        for invalid in ({**valid, 'results': [[True, 7]]},
                        {**valid, 'results': [[False, 0]]},
                        {**valid, 'messages': [[['missing data', [None, False, False, False, False, False]]]]}):
            with self.subTest(invalid=invalid):
                self.assertIsNotNone(compare(valid, invalid))

    def test_saved_data_feedback_styles_and_finalized_state_stay_exact(self):
        saved = self.record(3)
        saved['cmd'] = '/data get entity UUID VillagerData'
        saved['messages'][0][0][0] = '{profession:"minecraft:cleric",type:"minecraft:taiga",level:2}'
        finalized = self.record(1)
        finalized['cmd'] = '/data get entity UUID VillagerDataFinalized'
        for changed in ('profession:"minecraft:farmer"', 'type:"minecraft:plains"', 'level:1'):
            wrong = copy.deepcopy(saved)
            wrong['messages'][0][0][0] = changed
            self.assertEqual(compare(saved, wrong), 'messages')
        wrong_style = copy.deepcopy(saved)
        wrong_style['messages'][0][0][1][1] = True
        self.assertEqual(compare(saved, wrong_style), 'messages')
        self.assertEqual(compare(finalized, {**finalized, 'results': [[True, 0]]}), 'results')
