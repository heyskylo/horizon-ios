"""Machine-consumed domain and deterministic observation contracts for summoned zombie villagers."""
import re
import unittest

from tools.command_exec_corpus import zombie_villager_summoned_data


class ZombieVillagerCorpusTests(unittest.TestCase):
    def test_random_subjects_have_recorded_registry_domain_guards(self):
        ops = zombie_villager_summoned_data()
        expected = {'none', 'armorer', 'butcher', 'cartographer', 'cleric', 'farmer', 'fisherman',
                    'fletcher', 'leatherworker', 'librarian', 'mason', 'nitwit', 'shepherd',
                    'toolsmith', 'weaponsmith'}
        for n in (1, 2):
            uuid = f'000033ce-0000-0000-0000-{n:04x}00000001'
            probes = [op for op in ops if op.get('cmd', '').endswith(
                f'run data get entity {uuid} VillagerData.profession')]
            self.assertEqual(len(probes), 1)
            probe = probes[0]
            clauses = re.findall(r'unless data entity ([\w-]+) VillagerData\{profession:"minecraft:(\w+)"\}',
                                 probe['cmd'])
            self.assertEqual({target for target, _ in clauses}, {uuid})
            self.assertEqual({profession for _, profession in clauses}, expected)
            self.assertTrue(probe.get('record', True))
            self.assertNotIn('comparison', probe)

    def test_finalized_and_saved_data_reads_remain_recorded_and_exact(self):
        ops = zombie_villager_summoned_data()
        for n in (1, 2, 3):
            uuid = f'000033ce-0000-0000-0000-{n:04x}00000001'
            paths = ['VillagerDataFinalized', 'VillagerData'] if n == 3 else ['VillagerDataFinalized']
            for path in paths:
                reads = [op for op in ops if op.get('cmd') == f'/data get entity {uuid} {path}']
                self.assertEqual(len(reads), 1)
                self.assertTrue(reads[0].get('record', True))
                self.assertNotIn('comparison', reads[0])
