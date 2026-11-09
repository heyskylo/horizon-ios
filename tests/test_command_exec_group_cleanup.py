"""command_exec groups share one world: the harness removes what one group leaves alive before the next.

A persistent zombie the bell group left made the entities group's unbounded PersistenceRequired check
flaky: whether it was still loaded there depended on each engine's entity loading after the player had
been far away. tools/test_command_exec_parity.py puts corpus.group_cleanup() between two groups.
"""
import re
import unittest

from tools import command_exec_corpus as corpus
from tools.test_command_exec_parity import assemble


def group(name, *commands):
    return name, [corpus.cmd(c) for c in commands]


class GroupCleanupTests(unittest.TestCase):
    def test_cleanup_runs_between_groups_only(self):
        cleanup = corpus.group_cleanup()
        groups = [('setup', corpus.group_setup()), group('a', '/say a'), group('b', '/say b'), group('c', '/say c')]
        script = assemble(groups)
        setup = groups[0][1]
        # Nothing between setup and the first group (setup kills everything itself), nothing after the last.
        self.assertEqual(setup + groups[1][1] + cleanup + groups[2][1] + cleanup + groups[3][1], script)

    def test_generated_groups_are_cleaned_up_too(self):
        # tools/parity_fuzz.py's --groups-file scenarios come in as plain groups after the setup.
        script = assemble([('setup', []), group('fuzz persist 1', '/say 1'), group('fuzz persist 2', '/say 2')])
        self.assertEqual([corpus.cmd('/say 1'), *corpus.group_cleanup(), corpus.cmd('/say 2')], script)

    def test_a_group_that_continues_the_previous_one_keeps_its_scene(self):
        name = next(iter(corpus.CONTINUES_PREVIOUS))
        script = assemble([group('wolf anger live writes', '/say a'), group(name, '/say b')])
        self.assertEqual([corpus.cmd('/say a'), corpus.cmd('/say b')], script)

    def test_cleanup_records_nothing(self):
        # Records are matched to commands by position: the cleanup must not add any.
        for op in corpus.group_cleanup():
            self.assertEqual('cmd', op['op'])
            self.assertIs(False, op.get('record'))
            self.assertNotIn('expect_result', op)

    def test_each_level_shrinks_cube_mobs_then_kills_twice_above_its_floor(self):
        commands = [op['cmd'] for op in corpus.group_cleanup()]
        self.assertEqual(5 * len(corpus.CLEANUP_FLOOR), len(commands))
        for index, (level, floor) in enumerate(corpus.CLEANUP_FLOOR.items()):
            own = commands[5 * index:5 * index + 5]
            for command in own:
                # Bounded to the groups' heights: the world's generated entities below never enter it.
                self.assertIn(f' at @s if dimension minecraft:{level} positioned ~-1 {floor} ~-1 '
                              'if entity @s[dx=2,dy=4096,dz=2] run ', command)
            # A killed cube mob bigger than size 1 splits 20 ticks later, inside the next group.
            self.assertEqual(['slime', 'magma_cube', 'sulfur_cube'],
                             [re.search(r'type=minecraft:(\w+)', c).group(1) for c in own[:3]])
            self.assertTrue(all(c.endswith('run data merge entity @s {Size:0}') for c in own[:3]))
            # The second kill takes what the first one dropped.
            self.assertEqual(2, sum(c.startswith('/execute as @e[type=!player] ') and c.endswith(' run kill @s')
                                    for c in own[3:]))

    def test_the_floor_is_above_generated_terrain_and_below_every_group_summon(self):
        # The groups summon at y 100 and up in the overworld and far higher in the other levels.
        for name, operations in [g for g in corpus.groups() if g[0] not in corpus.ISOLATED_GROUPS]:
            for op in operations:
                text = op.get('cmd', '')
                found = re.search(r'summon \S+ (-?[\d.]+) (-?[\d.]+) (-?[\d.]+)', text)
                if not found:
                    continue
                level = re.search(r'execute in (?:minecraft:)?(\w+)', text)
                floor = corpus.CLEANUP_FLOOR[level.group(1) if level else 'overworld']
                with self.subTest(group=name, cmd=text[:80]):
                    self.assertGreaterEqual(float(found.group(2)), floor)

    def test_only_named_groups_read_entities_an_earlier_group_tagged(self):
        from tools.wolf_anger_corpus import groups as wolf_anger_groups
        runs = {'full run': [g for g in corpus.groups() if g[0] not in corpus.ISOLATED_GROUPS],
                'wolf anger clock diagnostic': wolf_anger_groups(corpus.cmd, corpus.setup, clock_diagnostic=True)}
        readers = set()
        for run, groups in runs.items():
            tagged = set()
            for name, operations in groups:
                own, read = set(), set()
                for op in operations:
                    text = op.get('cmd', '')
                    for tags in re.findall(r'Tags:\[([^\]]*)\]', text):
                        own |= {t.strip().strip('"') for t in tags.split(',')}
                    own |= set(re.findall(r'/tag \S+ add (\S+)', text))
                    read |= set(re.findall(r'tag=!?([\w.+-]+)', text))
                if (read - own) & tagged:
                    readers.add(name)
                    with self.subTest(run=run, group=name):
                        self.assertIn(name, corpus.CONTINUES_PREVIOUS,
                                      f'reads {sorted((read - own) & tagged)}: name it in CONTINUES_PREVIOUS')
                tagged |= own
        self.assertEqual(corpus.CONTINUES_PREVIOUS, readers)

    def test_unsynced_randoms_still_drops_sync_steps(self):
        sync = {'op': 'sync_randoms', 'uniquifier': 1, 'entity_id': 1, 'level_seed': 1}
        script = assemble([('a', [sync, corpus.cmd('/say a')]), ('b', [sync])], unsynced_randoms=True)
        self.assertEqual([corpus.cmd('/say a'), *corpus.group_cleanup()], script)


if __name__ == '__main__':
    unittest.main()
