"""Exact /fill controls must work in a fresh world and after other groups."""
import unittest

from tools import command_exec_corpus as corpus
from tools.loaded_cloud_corpus import loaded_clouds


class CommandFixtureFloorTests(unittest.TestCase):
    def test_floor_control_changes_every_block_after_prior_scenarios(self):
        floor = {(x, 199, z) for x in range(16) for z in range(16)}
        fixtures = {
            'loaded clouds': loaded_clouds(corpus.cmd, corpus.setup),
            'saved Sitting': corpus.nautilus_saved_sitting(),
            'ignored Age': corpus.zombie_nautilus_ignored_age(),
        }
        # Two supports reproduce the 254 result from parity-main; a prior full
        # floor reproduces its zero/no-change failure. Fresh air must work too.
        for name, operations in fixtures.items():
            for existing in (set(), {(4, 199, 4), (6, 199, 4)}, floor):
                with self.subTest(fixture=name, existing_stone=len(existing)):
                    blocks = {pos: 'stone' for pos in existing}
                    checked = False
                    for op in operations:
                        words = op.get('cmd', '').split()
                        if not words or words[0] != '/fill':
                            continue
                        x1, y1, z1, x2, y2, z2 = map(int, words[1:7])
                        changed = 0
                        for x in range(x1, x2 + 1):
                            for y in range(y1, y2 + 1):
                                for z in range(z1, z2 + 1):
                                    pos = (x, y, z)
                                    if blocks.get(pos, 'air') != words[7]:
                                        blocks[pos] = words[7]
                                        changed += 1
                        if 'expect_result' in op:
                            self.assertTrue(op.get('record', True))
                            self.assertEqual(op['expect_result'], 256)
                            self.assertEqual(changed, 256)
                            self.assertTrue(all(blocks.get(pos) == 'stone' for pos in floor))
                            checked = True
                            break
                    self.assertTrue(checked, 'fixture must retain its exact floor control')
