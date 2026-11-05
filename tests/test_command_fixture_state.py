"""command_exec groups run in one world, one after another: what a group leaves
behind changes the counts a later group's Java fixture guard expects.

Vanilla's `/scoreboard objectives add` returns the number of objectives after
the add (ScoreboardCommand.addObjective), and `/forceload add` the number of
chunks it newly forces (ForceLoadCommand.changeForceLoad). A group that keeps
an objective or a forced chunk moves those numbers for every later group, and
parity-main then stops on "Java fixture control failed" (#9587).
"""
import unittest

from tools import command_exec_corpus as corpus


def forced_chunks(words):
    # /forceload add|remove <from x z> [<to x z>], block coordinates.
    coords = [int(float(word)) // 16 for word in words[2:6]]
    if len(coords) == 2:
        coords *= 2
    x1, z1, x2, z2 = coords
    return {(x, z) for x in range(min(x1, x2), max(x1, x2) + 1)
            for z in range(min(z1, z2), max(z1, z2) + 1)}


class CommandFixtureStateTests(unittest.TestCase):
    def test_count_guards_match_what_earlier_groups_leave(self):
        objectives, forced = set(), set()
        for group, operations in corpus.groups():
            for op in operations:
                words = op.get('cmd', '').split() if op['op'] == 'cmd' else []
                expected = op.get('expect_result')
                if words[:3] == ['/scoreboard', 'objectives', 'add']:
                    objectives.add(words[3])
                    if expected is not None:
                        with self.subTest(group=group, cmd=op['cmd']):
                            self.assertEqual(expected, len(objectives),
                                             f'objectives alive: {sorted(objectives)}')
                elif words[:3] == ['/scoreboard', 'objectives', 'remove']:
                    objectives.discard(words[3])
                elif words[:2] == ['/forceload', 'add']:
                    new = forced_chunks(words) - forced
                    forced |= new
                    if expected is not None:
                        with self.subTest(group=group, cmd=op['cmd']):
                            self.assertEqual(expected, len(new))
                elif words[:3] == ['/forceload', 'remove', 'all']:
                    forced.clear()
                elif words[:2] == ['/forceload', 'remove']:
                    forced -= forced_chunks(words)
            with self.subTest(group=group):
                self.assertEqual(set(), objectives, 'the group must remove its scoreboard objectives')


if __name__ == '__main__':
    unittest.main()
