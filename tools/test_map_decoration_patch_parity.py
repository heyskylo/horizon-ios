#!/usr/bin/env python3
"""Exact item-aware empty decoration saved patches through actual integrated Game commands."""
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
    parser.add_argument('--removal-diagnostic', action='store_true', help='strict original full147, retaining seven existing item-command removal-patch differences')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--map-decoration-patch-fixture', '--work-dir',
               str(args.work_dir.resolve() if args.work_dir else root / '.local/map-decoration-patch-parity')]
    if args.removal_diagnostic:
        command.append('--map-decoration-removal-diagnostic')
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    for name in ['reference_only', 'prepare']:
        if getattr(args, name):
            command.append('--' + name.replace('_', '-'))
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
