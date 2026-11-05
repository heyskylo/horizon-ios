import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools import test_management_protocol_parity as management


class KeystoreTests(unittest.TestCase):
    def test_cached_java_uses_keytool_from_path(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            shim = root / 'cache' / f'java{management.EXE}'
            keytool = root / 'jdk' / 'bin' / f'keytool{management.EXE}'
            context = mock.Mock(options=0)
            def which(name):
                return str(keytool if name == 'keytool' else shim)
            with (mock.patch.dict(os.environ, {'JAVA_HOME': ''}),
                  mock.patch.object(management.shutil, 'which', side_effect=which),
                  mock.patch.object(management.subprocess, 'run') as run,
                  mock.patch.object(management.ssl, 'create_default_context', return_value=context)):
                management.make_keystore(root)
            self.assertEqual([Path(call.args[0][0]) for call in run.call_args_list], [keytool, keytool])

    def test_keytool_from_java_home(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            keytool = root / 'jdk' / 'bin' / f'keytool{management.EXE}'
            keytool.parent.mkdir(parents=True)
            keytool.touch()
            context = mock.Mock(options=0)
            with (mock.patch.dict(os.environ, {'JAVA_HOME': str(root / 'jdk')}),
                  mock.patch.object(management.shutil, 'which', return_value=None),
                  mock.patch.object(management.subprocess, 'run') as run,
                  mock.patch.object(management.ssl, 'create_default_context', return_value=context)):
                management.make_keystore(root)
            self.assertEqual([Path(call.args[0][0]) for call in run.call_args_list], [keytool, keytool])

    def test_missing_keytool_reports_requirement(self):
        with tempfile.TemporaryDirectory() as temp:
            with (mock.patch.dict(os.environ, {'JAVA_HOME': ''}),
                  mock.patch.object(management.shutil, 'which', return_value=None),
                  self.assertRaisesRegex(RuntimeError, 'keytool')):
                management.make_keystore(Path(temp))
