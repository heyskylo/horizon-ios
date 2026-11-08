#!/usr/bin/env python3
"""Armor stands stopped by a shulker and a boat (Entity.collide, #11877).

Stands fall onto a NoAI shulker, onto a boat and onto the stone, one slides
into a shulker's side and one rises with a shulker riding it, in a frozen level
stepped tick by tick; then the first shulker's lid opens and shuts under its
stand (Shulker.onPeekAmountChange). Lids on a wall and a ceiling push stands
sideways and down into the floor, and a floor lid lifts a stand into a
ceiling, each with the stand joined after and before its shulker; a boat runs
into a wall shulker's open lid and picks it up. Their Pos, Motion, OnGround and
fall_distance are compared with vanilla's.
"""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--reference-only', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--stand-entity-collision-fixture',
               '--work-dir', str(root / '.local/stand-entity-collision-parity')]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    if args.reference_only:
        command.append('--reference-only')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
