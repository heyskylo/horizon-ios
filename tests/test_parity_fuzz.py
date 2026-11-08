import unittest

from tools import parity_fuzz as fuzz


def record(text):
    return {'cmd': '/data get entity @s', 'messages': [[[text, None]]], 'results': [[True, 1]]}


class SnbtDiffTests(unittest.TestCase):
    def test_a_difference_names_its_field_and_key_order_is_not_one(self):
        vanilla = record('Pig has the following entity data: {Motion: [0.0d, 0.1d, 0.0d], Tags: ["a b"], '
                         'UUID: [I; 1, 2, 3, 4], Brain: {memories: {}}, OnGround: 1b}')
        horizon = record('Pig has the following entity data: {OnGround: 0b, Brain: {memories: {}}, '
                         'UUID: [I; 1, 2, 3, 4], Tags: ["a b"], Motion: [0.0d, 0.1d, 0.0d]}')
        self.assertEqual(fuzz.compare(vanilla, vanilla), [])
        self.assertEqual(fuzz.compare(vanilla, horizon), [('OnGround', '1b', '0b')])

    def test_list_lengths_names_and_missing_data_differ(self):
        self.assertEqual(fuzz.diff({'Items': ['1', '2']}, {'Items': ['1']}), [('Items[#]', 2, 1)])
        self.assertEqual(fuzz.diff({'a': {'b': '1s'}}, {'a': {}}), [('a.b', '1s', None)])
        renamed = record('Arrow has the following entity data: {}')
        self.assertEqual(fuzz.compare(record('Spectral Arrow has the following entity data: {}'), renamed),
                         [('<name>', 'Spectral Arrow', 'Arrow')])
        missing = {'cmd': '/data get entity @s', 'messages': [[['No entity was found', None]]], 'results': [[False, 0]]}
        self.assertEqual(fuzz.compare(renamed, missing)[0][0], '<result>')
        self.assertEqual(fuzz.normalise('Items[3].components[0].id'), 'Items[].components[].id')

    def test_attributes_compare_in_any_order_but_by_value(self):
        a = {'attributes': [{'id': 'b', 'base': '1.0d'}, {'id': 'a', 'base': '2.0d'}]}
        self.assertEqual(fuzz.diff(a, {'attributes': a['attributes'][::-1]}), [])
        changed = {'attributes': [{'id': 'a', 'base': '2.0d'}, {'id': 'b', 'base': '3.0d'}]}
        self.assertEqual(fuzz.diff(a, changed), [('attributes[1].base', '1.0d', '3.0d')])


class ShrinkTests(unittest.TestCase):
    scenario = {'family': 'persist', 'seed': 1, 'steps': [
        {'k': 'summon', 'type': 'pig', 'at': [7.5, 0, 7.5], 'n': 0, 'nbt': {'NoAI': '1b', 'Fire': '20s'}},
        {'k': 'tick', 'n': 4}, {'k': 'read_entity', 'n': 0}]}

    def test_edits_drop_steps_and_keys_and_halve_ticks(self):
        edits = list(fuzz.edits(self.scenario))
        self.assertIn(('nbt', 0, 'Fire'), edits)
        self.assertIn(('half', 1), edits)
        reduced = fuzz.reduce(self.scenario, {('nbt', 0, 'Fire'), ('half', 1)})
        self.assertEqual(reduced['steps'][0]['nbt'], {'NoAI': '1b'})
        self.assertEqual(reduced['steps'][1]['n'], 2)
        self.assertEqual(len(fuzz.reduce(self.scenario, {('drop', 1)})['steps']), 2)
        self.assertEqual(self.scenario['steps'][0]['nbt'], {'NoAI': '1b', 'Fire': '20s'})

    def test_each_read_divergence_counts_not_only_the_first(self):
        result = {'invalid': None, 'failures': [('persist|pig|live|Air', '/a', []), ('persist|pig|live|Air', '/b', []),
                                                ('persist|pig|reloaded|Fire', '/c', [])]}
        self.assertEqual(fuzz.signatures(result), ['persist|pig|live|Air', 'persist|pig|reloaded|Fire'])
        self.assertEqual(fuzz.signatures({**result, 'invalid': 'kept loaded'}), [])
        self.assertEqual(fuzz.signatures({'crash': 'log'}), ['crash'])

    def test_rendered_scenarios_sync_randoms_and_give_their_entity_a_fixed_uuid(self):
        ops = fuzz.render(self.scenario, 2)
        self.assertEqual([op['op'] for op in ops[:4]], ['cmd', 'wait_chunks', 'cmd', 'sync_randoms'])
        summon = next(op['cmd'] for op in ops if op['op'] == 'cmd' and op['cmd'].startswith('/summon'))
        self.assertIn('UUID:[I;1179998810,2,0,1]', summon)
        read = next(op for op in ops if op.get('read'))
        self.assertEqual(read['cmd'], '/data get entity 46555a5a-0000-0002-0000-000000000001')


if __name__ == '__main__':
    unittest.main()
