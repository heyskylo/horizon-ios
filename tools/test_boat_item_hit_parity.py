#!/usr/bin/env python3
"""BoatItem server UseItem: exact placement, consumption and one-tick identity.

Seven independently reset stone/turned/water/slab/entity/wall cases. The
optional Motion observations are additive raw diagnostics for boat physics.
"""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--motion-diagnostic', action='store_true')
    parser.add_argument('--reference-only', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    folder = 'boat-item-hit-motion' if args.motion_diagnostic else 'boat-item-hit-parity'
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--boat-item-hit-fixture', '--work-dir', str(root / '.local' / folder)]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    if args.motion_diagnostic:
        command.append('--boat-motion-diagnostic')
    if args.reference_only:
        command.append('--reference-only')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
