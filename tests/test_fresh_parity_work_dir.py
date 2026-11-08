"""Fresh capture defaults must not bypass explicit-directory safeguards."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from tools import test_command_exec_parity as driver
from tools import test_crafter_drop_name_parity as crafter
from tools import test_mob_equipment_custom_data_parity as equipment
from tools import test_pot_sherd_templates_parity as pots


class FreshWorkDirTests(unittest.TestCase):
    def test_default_captures_are_distinct_and_not_created(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(driver.lab, 'LOCAL', Path(tmp)):
            folders = []
            for _ in range(2):
                with mock.patch.object(sys, 'argv', ['harness', '--reference-only']):
                    folder = driver.fresh_work_dir('fixture')
                    self.assertEqual(['harness', '--reference-only', '--work-dir', str(folder)], sys.argv)
                    self.assertTrue(folder.parent.is_dir())
                    self.assertFalse(folder.exists())
                    folders.append(folder)
            self.assertNotEqual(*folders)

    def test_explicit_directory_accepts_both_argument_forms(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp) / 'capture'
            for arguments in [['--work-dir', str(folder)], ['--work-dir=' + str(folder)]]:
                with self.subTest(arguments=arguments), mock.patch.object(sys, 'argv', ['harness', *arguments]):
                    before = list(sys.argv)
                    self.assertEqual(folder, driver.fresh_work_dir('fixture'))
                    self.assertEqual(before, sys.argv)
                    self.assertFalse(folder.exists())

    def test_existing_captures_are_rejected_without_modification(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / 'vanilla.jsonl'
            marker.write_text('keep this reference', encoding='utf-8')
            with mock.patch.object(sys, 'argv', ['harness', '--work-dir', tmp]):
                with self.assertRaisesRegex(AssertionError, 'preserve previous capture attempts'):
                    driver.fresh_work_dir('fixture')
            self.assertEqual('keep this reference', marker.read_text(encoding='utf-8'))

    def test_three_default_entry_points_reach_the_driver(self):
        # Stop at the transport boundary: no Java server, game or world is started.
        for harness in [crafter, equipment, pots]:
            with self.subTest(harness=harness.__name__), tempfile.TemporaryDirectory() as tmp:
                with mock.patch.object(driver.lab, 'LOCAL', Path(tmp)), \
                        mock.patch.object(sys, 'argv', ['harness']), \
                        mock.patch.dict(driver.corpus_module.__dict__), \
                        mock.patch.object(driver, 'main', side_effect=RuntimeError('transport boundary')) as run:
                    with self.assertRaisesRegex(RuntimeError, 'transport boundary'):
                        harness.main()
                    run.assert_called_once_with()
                    folder = Path(sys.argv[sys.argv.index('--work-dir') + 1])
                    self.assertTrue(folder.is_relative_to(Path(tmp)))
                    self.assertFalse(folder.exists())


if __name__ == '__main__':
    unittest.main()
