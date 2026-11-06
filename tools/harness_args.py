"""Scratch directories for the command-oracle harnesses.

The harnesses run `tools/test_command_exec_parity.py`, which reads `--work-dir`
with argparse: a value may be separate (`--work-dir PATH`) or one argument
(`--work-dir=PATH`), and the last one wins. A harness that appends its own
default after the caller's therefore replaces the directory they asked for.
"""
import sys
from pathlib import Path


def ensure_work_dir(default: Path, argv=None) -> Path:
    """Append `--work-dir default` unless the caller named one, either way, and return the directory the
    run will use (argparse's: the last one given)."""
    argv = sys.argv if argv is None else argv
    chosen = None
    for index, argument in enumerate(argv[1:], start=1):
        if argument == '--work-dir' and index + 1 < len(argv):
            chosen = argv[index + 1]
        elif argument.startswith('--work-dir='):
            chosen = argument[len('--work-dir='):]
    if chosen is None:
        argv += ['--work-dir', str(default)]
        chosen = str(default)
    return Path(chosen)
