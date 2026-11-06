"""A command-oracle harness keeps the `--work-dir` its caller gave.

The harnesses append a default scratch directory when the caller named none.
`tools/test_command_exec_parity.py` reads that argument with argparse, where the
last `--work-dir` wins, so an extra pair appended after `--work-dir=/tmp/x`
would send the run somewhere the caller did not ask for.
"""
import importlib.util
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / 'tools'
_spec = importlib.util.spec_from_file_location('harness_args', TOOLS / 'harness_args.py')
harness_args = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(harness_args)

DEFAULT = Path('/scratch/default')


class EnsureWorkDirTest(unittest.TestCase):
    def test_one_argument_spelling_is_kept(self):
        argv = ['harness.py', '--work-dir=/tmp/asked-for', '--full-diagnostic']
        harness_args.ensure_work_dir(DEFAULT, argv)
        self.assertEqual(argv, ['harness.py', '--work-dir=/tmp/asked-for', '--full-diagnostic'])

    def test_separate_spelling_is_kept(self):
        argv = ['harness.py', '--work-dir', '/tmp/asked-for']
        harness_args.ensure_work_dir(DEFAULT, argv)
        self.assertEqual(argv, ['harness.py', '--work-dir', '/tmp/asked-for'])

    def test_default_is_appended_when_none_is_given(self):
        argv = ['harness.py', '--reference', '/tmp/vanilla.jsonl']
        harness_args.ensure_work_dir(DEFAULT, argv)
        self.assertEqual(
            argv,
            ['harness.py', '--reference', '/tmp/vanilla.jsonl', '--work-dir', '/scratch/default'],
        )

    def test_script_path_is_not_an_argument(self):
        argv = ['--work-dir/not-an-argument.py']
        harness_args.ensure_work_dir(DEFAULT, argv)
        self.assertEqual(argv, ['--work-dir/not-an-argument.py', '--work-dir', '/scratch/default'])


if __name__ == '__main__':
    unittest.main()
