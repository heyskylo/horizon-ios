"""Wrappers around the command_exec driver each run in a folder of their own.

In the driver's default folder (command_exec's own), two harnesses running at once read each other's scripts (#14855).
"""
import importlib
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

WRAPPERS = ['test_frame_map_writes_parity', 'test_wolf_anger_writes_parity']


def work_dir(name, arguments):
    harness = importlib.import_module('tools.' + name)
    with patch.object(sys, 'argv', [name + '.py', *arguments]), patch.object(sys, 'exit'), \
         patch.object(harness.subprocess, 'run') as run:
        harness.main()
    command = run.call_args.args[0]
    return Path(command[command.index('--work-dir') + 1])


class WrapperWorkDirTest(unittest.TestCase):
    def test_each_wrapper_has_its_own_default_folder(self):
        folders = [work_dir(name, []) for name in WRAPPERS]
        self.assertEqual(len(set(folders)), len(WRAPPERS))
        for folder in folders:
            self.assertEqual(folder.parent.name, '.local')
            self.assertNotEqual(folder.name, 'command-exec-parity')

    def test_an_explicit_folder_reaches_the_driver(self):
        requested = Path('requested').resolve()
        for name in WRAPPERS:
            with self.subTest(harness=name):
                self.assertEqual(work_dir(name, ['--work-dir', str(requested)]), requested)


if __name__ == '__main__':
    unittest.main()
