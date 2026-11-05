#!/usr/bin/env python3
"""Live guarded-block piglin anger UUID/remaining TTL through actual Game owner actions."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--work-dir', type=Path)
    parser.add_argument('--whole-memory-diagnostic', action='store_true', help='strict original394, retaining six H191 compound-order observations')
    parser.add_argument('--raw-diagnostic', action='store_true', help='retain full random AI vectors and combat amounts')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'), '--piglin-anger-memory-fixture',
               '--work-dir', str(args.work_dir.resolve() if args.work_dir else root / '.local/piglin-anger-memory-parity')]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.reference_only:
        command.append('--reference-only')
    if args.whole_memory_diagnostic:
        command.append('--piglin-anger-memory-diagnostic')
    if args.raw_diagnostic:
        command.append('--piglin-anger-raw-diagnostic')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
