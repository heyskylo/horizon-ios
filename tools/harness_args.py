"""Scratch directories for the command-oracle harnesses.

The harnesses run `tools/test_command_exec_parity.py`, which reads `--work-dir`
with argparse: a value may be separate (`--work-dir PATH`) or one argument
(`--work-dir=PATH`), and the last one wins. A harness that appends its own
default after the caller's therefore replaces the directory they asked for.
"""
import sys
from pathlib import Path


def ensure_work_dir(default: Path, argv=None) -> None:
    """Append `--work-dir default` unless the caller named one, either way."""
    argv = sys.argv if argv is None else argv
    if not any(
        argument == '--work-dir' or argument.startswith('--work-dir=')
        for argument in argv[1:]
    ):
        argv += ['--work-dir', str(default)]
