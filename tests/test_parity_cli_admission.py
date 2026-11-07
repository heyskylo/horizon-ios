"""CLI admission/routing contracts; every Java, Cargo and native launch is stubbed."""
from contextlib import ExitStack, redirect_stdout
import copy
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
from types import SimpleNamespace
import unittest
from unittest import mock

from tools import test_command_exec_parity as command_runner
from tools import test_loaded_pig_tags_parity, test_loot_mine_pot_components_parity
from tools import test_mob_discard_parity


ROOT = Path(__file__).resolve().parents[1]


class RunnerAdmissionTests(unittest.TestCase):
    def run_fixture(self, *, reject=False, reference_only=False, binary=False):
        with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
            folder = Path(directory)
            runner = command_runner
            events = []
            row = {'cmd': '/say cli', 'messages': [], 'results': [[True, 1]]}
            argv = ['parity.py', '--work-dir', str(folder)]
            if reference_only:
                argv.append('--reference-only')
            if binary:
                executable = folder / 'archived-game'
                executable.touch()
                argv += ['--binary', str(executable)]
            stack.enter_context(mock.patch.object(sys, 'argv', argv))
            stack.enter_context(mock.patch.object(runner.base, 'verify_artifacts', return_value=(
                {'minecraft': {'java_major': '25', 'version': '26.3'}},
                {'server': folder / 'server.jar'}, [])))
            stack.enter_context(mock.patch.object(runner.corpus_module, 'groups', return_value=[
                ('cli admission', [{'op': 'cmd', 'cmd': '/say cli', 'expect_result': 1}])]))
            stack.enter_context(mock.patch.object(runner, 'install_datapack_fixtures'))
            stack.enter_context(mock.patch.object(runner.platform, 'platform', return_value='unit'))

            def subprocess_run(command, **kwargs):
                if command[0] == 'java':
                    events.append('java')
                    (folder / 'vanilla.jsonl').write_text(json.dumps(row) + '\n')
                elif command[0] == 'cargo':
                    events.append('native-build')
                else:
                    self.assertEqual(command[0], 'javac')
                return SimpleNamespace(returncode=0)

            def native_run(command, **kwargs):
                events.append('native-run')
                Path(kwargs['env']['HORIZON_COMMAND_OUTPUT']).write_text(json.dumps(row) + '\n')
                return SimpleNamespace(returncode=0)

            def admit(rows):
                events.append('admit')
                self.assertEqual(rows, [row])
                if reject:
                    raise ValueError('reference rejected')

            stack.enter_context(mock.patch.object(runner.subprocess, 'run', side_effect=subprocess_run))
            stack.enter_context(mock.patch.object(runner.subprocess, 'check_output', return_value=json.dumps(
                {'target_directory': str(folder / 'target')})))
            stack.enter_context(mock.patch.object(runner, 'run_game', side_effect=native_run))
            with redirect_stdout(io.StringIO()):
                if reject:
                    with self.assertRaisesRegex(ValueError, 'reference rejected'):
                        runner.main(reference_admission=admit)
                else:
                    runner.main(reference_admission=admit)
            return events

    def test_normal_run_admits_before_native_build_and_execution(self):
        self.assertEqual(self.run_fixture(), ['java', 'admit', 'native-build', 'native-run'])

    def test_rejected_reference_never_builds_or_runs_native(self):
        self.assertEqual(self.run_fixture(reject=True), ['java', 'admit'])

    def test_reference_only_still_admits_without_native(self):
        self.assertEqual(self.run_fixture(reference_only=True), ['java', 'admit'])

    def test_archived_binary_keeps_admission_and_skips_build(self):
        self.assertEqual(self.run_fixture(binary=True), ['java', 'admit', 'native-run'])


class SpecializedHarnessRoutingTests(unittest.TestCase):
    def run_fixture(self, name, mode, invalid=False):
        with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
            folder = Path(directory)
            module = {'loaded_pig_tags': test_loaded_pig_tags_parity,
                      'loot_mine_pot_components': test_loot_mine_pot_components_parity}[name]
            runner = module.runner
            # Pot admission independently requires two unchanged before/after sources.
            program = [dict(op='cmd', cmd='/source ' + source, expect_result=1,
                            source_snapshot=source, snapshot_phase=phase)
                       for source in ['a', 'b'] for phase in ['before', 'after']]
            rows = [dict(cmd=op['cmd'], messages=[], results=[[True, 1]]) for op in program]
            if invalid:
                rows[0]['results'] = [[True, True]]  # bool is not an integer callback.
            original = copy.deepcopy(rows)
            argv = ['harness.py', '--work-dir', str(folder)]
            if mode == 'reference-only':
                argv.append('--reference-only')
            if mode == 'reuse':
                argv += ['--reference-dir', str(folder), '--binary', 'archived-game']
            stack.enter_context(mock.patch.object(sys, 'argv', argv))
            stack.enter_context(mock.patch.object(module, 'cases', return_value=program))
            stack.enter_context(mock.patch.object(runner.corpus_module, 'group_setup', return_value=[]))
            stack.enter_context(mock.patch.object(runner.corpus_module, 'groups'))
            events = []

            def outputs():
                text = ''.join(json.dumps(row) + '\n' for row in rows)
                (folder / 'vanilla.jsonl').write_text(text)
                if mode != 'reference-only':
                    (folder / 'rust.jsonl').write_text(text)

            def fresh(*, reference_admission):
                events.append('fresh')
                reference_admission(rows)
                events.append('admitted')
                outputs()

            def reuse(reference, output, actual_program):
                self.assertEqual(reference, folder.resolve())
                self.assertEqual(output, folder.resolve())
                self.assertEqual(actual_program, program)
                events.append('reuse')
                outputs()

            stack.enter_context(mock.patch.object(runner, 'main', side_effect=fresh))
            stack.enter_context(mock.patch.object(module, 'native_from_reference', side_effect=reuse))
            with redirect_stdout(io.StringIO()):
                if invalid:
                    with self.assertRaises(AssertionError):
                        module.main()
                else:
                    self.assertFalse(module.main())
            self.assertEqual(rows, original, 'admission must not normalize or alter raw reference rows')
            return events

    def test_both_harnesses_support_normal_reference_only_and_frozen_routes(self):
        for name in ['loaded_pig_tags', 'loot_mine_pot_components']:
            for mode in ['normal', 'reference-only', 'reuse']:
                with self.subTest(harness=name, mode=mode):
                    self.assertEqual(self.run_fixture(name, mode),
                                     ['reuse'] if mode == 'reuse' else ['fresh', 'admitted'])

    def test_both_normal_routes_reject_typed_admission_before_native(self):
        for name in ['loaded_pig_tags', 'loot_mine_pot_components']:
            with self.subTest(harness=name):
                self.assertEqual(self.run_fixture(name, 'normal', invalid=True), ['fresh'])


class MobDiscardArgumentsTests(unittest.TestCase):
    def parse(self, argv, environment):
        with mock.patch.dict(os.environ, environment, clear=True):
            return test_mob_discard_parity, test_mob_discard_parity.parse_args(argv)

    def test_normal_cli_uses_prepared_resources_without_environment(self):
        module, args = self.parse([], {})
        self.assertEqual(args.resources, module.lab.RESOURCES)

    def test_environment_selects_explicit_resource_directory(self):
        _, args = self.parse([], {'HORIZON_TEST_RESOURCES': '/fixture/environment'})
        self.assertEqual(args.resources, Path('/fixture/environment'))

    def test_empty_environment_uses_prepared_resources(self):
        module, args = self.parse([], {'HORIZON_TEST_RESOURCES': ''})
        self.assertEqual(args.resources, module.lab.RESOURCES)

    def test_cli_resource_directory_overrides_environment(self):
        _, args = self.parse(['--resources', '/fixture/argument'], {'HORIZON_TEST_RESOURCES': '/fixture/environment'})
        self.assertEqual(args.resources, Path('/fixture/argument'))


class ImportOrderRegressionTests(unittest.TestCase):
    def test_full_discovery_keeps_test_module_and_tools_helper_distinct(self):
        program = textwrap.dedent('''
            import importlib
            import unittest
            loader = unittest.TestLoader()
            # Populate the real colliding discovery name first, with no cache edits.
            loader.discover('tests', pattern='test_rust_parity.py')
            loader.discover('tests')
            discovered_test = importlib.import_module('test_rust_parity')
            from tools import test_rust_parity as helper
            from tools import test_command_exec_parity as runner
            from tools import test_loaded_pig_tags_parity as pigs
            from tools import test_loot_mine_pot_components_parity as pots
            from tools import test_mob_discard_parity as discard
            assert discovered_test is not helper
            assert runner.base is helper and callable(helper.verify_artifacts)
            assert pigs.runner is runner and pots.runner is runner
            assert discard.lab is runner.lab
            tests = loader.loadTestsFromName('test_parity_cli_admission.RunnerAdmissionTests')
            assert tests.countTestCases() == 4
            result = unittest.TextTestRunner(verbosity=2).run(tests)
            raise SystemExit(not result.wasSuccessful())
        ''')
        result = subprocess.run([sys.executable, '-c', program], cwd=ROOT,
                                env=dict(os.environ, PYTHONNOUSERSITE='1'),
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
