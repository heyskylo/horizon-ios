#!/usr/bin/env python3
"""Generic mob home NBT: real Game and pinned Java, preserving strict feedback."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--holder-removal-diagnostic', action='store_true')
    modes.add_argument('--late-detach-control', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    folder = ('mob-home-late-detach-control' if args.late_detach_control else
              'mob-home-holder-removal-diagnostic' if args.holder_removal_diagnostic else 'mob-home-parity')
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--mob-home-fixture', '--work-dir', str(root / '.local' / folder)]
    if args.holder_removal_diagnostic:
        command.append('--mob-home-holder-removal-diagnostic')
    if args.late_detach_control:
        command.append('--mob-home-late-detach-control')
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
