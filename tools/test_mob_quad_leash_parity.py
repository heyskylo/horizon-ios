#!/usr/bin/env python3
"""Compare live NoAI horse leash dispatch against pinned Java26.3."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--raw-diagnostic', action='store_true', help='add full-precision yaw/motion records')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    folder = 'mob-quad-leash-raw-diagnostic' if args.raw_diagnostic else 'mob-quad-leash-parity'
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--mob-quad-fixture', '--work-dir', str(root / '.local' / folder)]
    if args.raw_diagnostic:
        command.append('--mob-quad-raw-diagnostic')
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
