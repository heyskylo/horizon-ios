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
