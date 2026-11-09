#!/usr/bin/env python3
"""Distance-two /execute loaded after forced-ticket removal.

The default90-record gate is strict. --block-diagnostic also retains30 FULL
block-access observations, whose separate failures are outside this task.
"""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--block-diagnostic', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    folder = 'loaded-block-diagnostic' if args.block_diagnostic else 'loaded-command-parity'
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--loaded-fixture', '--only', 'loaded ticket removal',
               '--work-dir', str(root / '.local' / folder)]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    if args.block_diagnostic:
        command.append('--loaded-block-diagnostic')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
