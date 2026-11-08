"""Command harnesses accept the arguments parity.py actually supplies."""
import importlib
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

class ReachedDriver(Exception):
    pass


class CommandHarnessInvocationTest(unittest.TestCase):
    def test_default_and_explicit_scratch_paths_reach_the_shared_driver(self):
        for name, default in [('test_noai_dolphin_air_parity', 'noai-dolphin-air-parity'),
                              ('test_spell_ticks_parity', 'spell-ticks-parity')]:
            harness = importlib.import_module('tools.' + name)
            with tempfile.TemporaryDirectory() as temporary:
                local = Path(temporary)
                requested = local / 'requested'
                for arguments, expected in [([], local / default),
                                            (['--reference-only'], local / default),
                                            (['--work-dir', str(requested)], requested),
                                            ([f'--work-dir={requested}', '--binary=game'], requested)]:
                    with self.subTest(harness=name, arguments=arguments):
                        argv = [name + '.py', *arguments]
                        with patch.object(sys, 'argv', argv), \
                             patch.object(harness.driver.lab, 'LOCAL', local), \
                             patch.object(harness.driver.corpus_module, 'groups'), \
                             patch.object(harness.driver, 'main', side_effect=ReachedDriver) as run:
                            with self.assertRaises(ReachedDriver):
                                harness.main()
                            run.assert_called_once_with()
                            self.assertEqual(harness.harness_args.ensure_work_dir(local / 'unused'), expected)
                            self.assertIn('--allow-gaps', argv)


if __name__ == '__main__':
    unittest.main()
