"""Per-operation presence gates keep raw random counts and strict defaults."""
import copy
import unittest

from tools.test_command_exec_parity import compare, unordered_attributes


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
        speed_first = self.body(('movement_speed', '0.25d'), ('max_health', '20.0d'))
        health_first = self.body(('max_health', '20.0d'), ('movement_speed', '0.25d'))
        other_base = self.body(('max_health', '10.0d'), ('movement_speed', '0.25d'))
        self.assertNotEqual(speed_first, health_first)
        self.assertEqual(unordered_attributes(speed_first), unordered_attributes(health_first))
        self.assertNotEqual(unordered_attributes(other_base), unordered_attributes(health_first))
        plain = {'cmd': '/say', 'messages': [[['attributes', self.PLAIN]]], 'results': [[True, 1]]}
        self.assertEqual(unordered_attributes(plain), plain)

    def raw_body(self, body):
        return {'cmd': '/data get entity x',
                'messages': [[['Pig has the following entity data: ' + body, self.PLAIN]]],
                'results': [[True, 1]]}

    def test_only_the_root_entity_attribute_list_is_unordered(self):
        for body in ('{CustomName: "attributes: [{id: b}, {id: a}]"}',
                     "{CustomName: 'attributes: [{id: b}, {id: a}]'}",
                     '{otherattributes: [{id: b}, {id: a}]}',
                     '{custom: {attributes: [{id: b}, {id: a}]}}'):
            with self.subTest(body=body):
                record = self.raw_body(body)
                other = self.raw_body(body.replace('{id: b}, {id: a}', '{id: a}, {id: b}'))
                self.assertEqual(unordered_attributes(record), record)
                self.assertNotEqual(unordered_attributes(record), unordered_attributes(other))
        record = self.raw_body('{attributes: [{id: b}, {id: a}]}')
        record['cmd'] = '/say attributes'
        self.assertEqual(unordered_attributes(record), record)

    def test_entries_duplicates_modifiers_and_other_state_stay_significant(self):
        first = '{id: "minecraft:max_health", base: 20.0d, modifiers: [{id: "x", amount: 1.0d}, {id: "y", amount: 2.0d}]}'
        second = '{id: "minecraft:movement_speed", base: 0.25d}'
        body = '{attributes: [' + first + ', ' + second + ', ' + first + '], Motion: [1.0d, 2.0d, 3.0d], invulnerable_time: 30}'
        record = self.raw_body(body)
        original = copy.deepcopy(record)
        reordered = self.raw_body(body.replace(first + ', ' + second + ', ' + first,
                                               second + ', ' + first + ', ' + first))
        self.assertEqual(unordered_attributes(record), unordered_attributes(reordered))
        for changed in (body.replace(', ' + first + '], Motion', '], Motion'),
                        body.replace('amount: 1.0d', 'amount: 3.0d'),
                        body.replace('id: "x"', 'id: "z"'),
                        body.replace('{id: "x", amount: 1.0d}, {id: "y", amount: 2.0d}',
                                     '{id: "y", amount: 2.0d}, {id: "x", amount: 1.0d}'),
                        body.replace('[1.0d, 2.0d, 3.0d]', '[2.0d, 1.0d, 3.0d]'),
                        body.replace('invulnerable_time: 30', 'invulnerable_time: 29')):
            with self.subTest(body=changed):
                self.assertNotEqual(unordered_attributes(record), unordered_attributes(self.raw_body(changed)))
        self.assertEqual(record, original)
        for field, value in (('results', [[False, 0]]), ('state', {'health': 19})):
            changed = {**record, field: value}
            self.assertNotEqual(unordered_attributes(record), unordered_attributes(changed))

    def test_quotes_nested_lists_and_malformed_input(self):
        first = r'''{id: "b", note: 'comma, bracket ] and "quote"', nested: [1, 2]}'''
        second = r'''{id: "a", note: "escaped \" and comma, bracket ]"}'''
        left = self.raw_body('{attributes: [' + first + ', ' + second + ']}')
        right = self.raw_body('{attributes: [' + second + ', ' + first + ']}')
        self.assertEqual(unordered_attributes(left), unordered_attributes(right))
        for body in ('{attributes: [{id: b}, {id: a}',
                     '{attributes: [{id: b}, {id: a}}]}',
                     '{attributes: [{id: b}, {id: a}], CustomName: "unterminated}'):
            with self.subTest(body=body):
                record = self.raw_body(body)
                self.assertEqual(unordered_attributes(record), record)

    def test_styles_of_entries_and_all_separators_are_retained(self):
        record = self.body(('movement_speed', '0.25d'), ('max_health', '20.0d'), ('max_health', '20.0d'))
        normalized = unordered_attributes(record)
        styled = copy.deepcopy(record)
        styled['messages'][0][4][1] = [123] + [False] * 5
        self.assertNotEqual(unordered_attributes(styled), normalized)
        # The second separator's style must not be replaced by a copy of the first.
        styled = copy.deepcopy(record)
        styled['messages'][0][19] = [', ', [123] + [False] * 5]
        styled['messages'][0].insert(20, ['{', self.PLAIN])
        self.assertNotEqual(unordered_attributes(styled), normalized)
