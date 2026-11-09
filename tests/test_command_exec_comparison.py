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


class UnorderedAttributesTests(unittest.TestCase):
    KEY, PLAIN, ID = [5636095] + [False] * 5, [None] + [False] * 5, [5635925] + [False] * 5

    def body(self, *entries):
        """`/data get entity` feedback holding an `attributes` list of (id, base) entries, as the printer styles it."""
        spans = [['Pig has the following entity data: {', self.PLAIN], ['attributes', self.KEY], [': [', self.PLAIN]]
        for n, (name, base) in enumerate(entries):
            spans += [['{' if n == 0 else ', {', self.PLAIN], ['id', self.KEY], [': "', self.PLAIN],
                      [f'minecraft:{name}', self.ID], ['", ', self.PLAIN], ['base', self.KEY], [': ' + base, self.PLAIN],
                      ['}', self.PLAIN]]
        spans += [[']}', self.PLAIN]]
        return {'cmd': '/data get entity x', 'messages': [spans], 'results': [[True, 1]]}

    def test_the_lists_order_is_ignored_and_its_entries_are_not(self):
        from tools.test_command_exec_parity import unordered_attributes
        speed_first = self.body(('movement_speed', '0.25d'), ('max_health', '20.0d'))
        health_first = self.body(('max_health', '20.0d'), ('movement_speed', '0.25d'))
        other_base = self.body(('max_health', '10.0d'), ('movement_speed', '0.25d'))
        self.assertNotEqual(speed_first, health_first)
        self.assertEqual(unordered_attributes(speed_first), unordered_attributes(health_first))
        self.assertNotEqual(unordered_attributes(other_base), unordered_attributes(health_first))
        plain = {'cmd': '/say', 'messages': [[['attributes', self.PLAIN]]], 'results': [[True, 1]]}
        self.assertEqual(unordered_attributes(plain), plain)
