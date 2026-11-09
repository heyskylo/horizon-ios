#!/usr/bin/env python3
"""Crafting-result map post-processing through actual connected-owner menu packets."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--work-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--only', 'craft map takes', '--work-dir',
               str(args.work_dir.resolve() if args.work_dir else root / '.local/craft-map-take-parity')]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    for name in ['reference_only', 'prepare']:
        if getattr(args, name):
            command.append('--' + name.replace('_', '-'))
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
