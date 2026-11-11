"""The automatic parity runner must be able to invoke the special-attack harness."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools import test_special_attack_parity as harness


class SpecialAttackInvocationTests(unittest.TestCase):
    def test_no_arguments_reaches_resource_validation(self):
        with patch('sys.argv', ['test_special_attack_parity.py']), \
                patch.object(harness.base, 'verify_artifacts', side_effect=RuntimeError('resources checked')):
            with self.assertRaisesRegex(RuntimeError, 'resources checked'):
                harness.main()

    def test_selects_gameplay_library_test_not_other_cargo_artifacts(self):
        artifact = dict(reason='compiler-artifact', executable='/scratch/game-tests',
                        profile=dict(test=True), target=dict(name='horizon_client', kind=['lib']))
        messages = [
            'Compiling dependencies',
            json.dumps({**artifact, 'target': dict(name='horizon_client', kind=['bin'])}),
            json.dumps({**artifact, 'profile': dict(test=False)}),
            json.dumps({**artifact, 'target': dict(name='horizon_client_lib', kind=['lib'])}),
            json.dumps(artifact),
            json.dumps(dict(reason='build-finished', success=True)),
        ]
        with tempfile.TemporaryDirectory() as scratch:
            log = Path(scratch) / 'cargo.log'

            def compile_fixture(*args, **kwargs):
                kwargs['stdout'].write('\n'.join(messages))

            with patch.object(harness.subprocess, 'run', side_effect=compile_fixture):
                self.assertEqual(harness.build_test_binary(log), Path('/scratch/game-tests').resolve())

    def test_missing_gameplay_artifact_fails_instead_of_running_unrelated_binary(self):
        with tempfile.TemporaryDirectory() as scratch, patch.object(harness.subprocess, 'run'):
            with self.assertRaisesRegex(RuntimeError, 'exactly one gameplay test executable'):
                harness.build_test_binary(Path(scratch) / 'cargo.log')
