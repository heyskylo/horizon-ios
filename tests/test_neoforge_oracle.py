"""tools/neoforge_oracle.py: the argument file and the links the server folder is built from (no server is started)."""

from pathlib import Path
import os
import tempfile
import unittest
from unittest import mock

from tools import neoforge_oracle as oracle


class ArgsFileTests(unittest.TestCase):
    def test_windows_uses_win_args(self):
        with mock.patch.object(oracle, 'WINDOWS', True):
            self.assertEqual(oracle.args_file(Path('home')).name, 'win_args.txt')

    def test_elsewhere_uses_unix_args(self):
        with mock.patch.object(oracle, 'WINDOWS', False):
            path = oracle.args_file(Path('home'))
        self.assertEqual(path.name, 'unix_args.txt')
        self.assertEqual(path.parent, Path('home') / 'libraries/net/neoforged/neoforge' / oracle.NEOFORGE)


class LinkTests(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.TemporaryDirectory()
        self.addCleanup(self._dir.cleanup)
        self.root = Path(self._dir.name)
        (self.root / 'libs/sub').mkdir(parents=True)
        (self.root / 'libs/sub/a.jar').write_bytes(b'lib')
        (self.root / 'mod.jar').write_bytes(b'mod')

    def test_folder_shows_the_target_files(self):
        oracle.link(self.root / 'libraries', self.root / 'libs')
        self.assertEqual((self.root / 'libraries/sub/a.jar').read_bytes(), b'lib')

    def test_file_shows_the_target_bytes(self):
        oracle.link(self.root / 'linked.jar', self.root / 'mod.jar')
        self.assertEqual((self.root / 'linked.jar').read_bytes(), b'mod')

    @unittest.skipUnless(os.name == 'nt', 'Windows only')
    def test_windows_needs_no_symlink_privilege(self):
        with mock.patch.object(Path, 'symlink_to', side_effect=OSError(1314, 'privilege')):
            oracle.link(self.root / 'libraries', self.root / 'libs')
            oracle.link(self.root / 'linked.jar', self.root / 'mod.jar')
        self.assertEqual((self.root / 'libraries/sub/a.jar').read_bytes(), b'lib')
        self.assertEqual((self.root / 'linked.jar').read_bytes(), b'mod')

    @unittest.skipUnless(os.name == 'nt', 'Windows only')
    def test_windows_copies_when_linking_fails(self):
        with mock.patch.object(oracle.subprocess, 'run', return_value=mock.Mock(returncode=1)), \
                mock.patch.object(oracle.os, 'link', side_effect=OSError('cross-device')):
            oracle.link(self.root / 'libraries', self.root / 'libs')
            oracle.link(self.root / 'linked.jar', self.root / 'mod.jar')
        self.assertEqual((self.root / 'libraries/sub/a.jar').read_bytes(), b'lib')
        self.assertEqual((self.root / 'linked.jar').read_bytes(), b'mod')


if __name__ == '__main__':
    unittest.main()
