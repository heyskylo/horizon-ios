#!/usr/bin/env python3
"""Pufferfish player contact with frozen world time and actual player-loop witnesses."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path)
    parser.add_argument('--prepare',action='store_true')
    parser.add_argument('--reference-only',action='store_true')
    parser.add_argument('--body-diagnostic',action='store_true')
    args=parser.parse_args()
    root=Path(__file__).resolve().parents[1]
    folder='pufferfish-contact-bodies' if args.body_diagnostic else 'pufferfish-contact-parity'
    command=[sys.executable,str(root/'tools/test_command_exec_parity.py'),
             '--pufferfish-contact-fixture','--work-dir',str(root/'.local'/folder)]
    if args.binary: command+=['--binary',str(args.binary.resolve())]
    if args.prepare: command.append('--prepare')
    if args.reference_only: command.append('--reference-only')
    if args.body_diagnostic: command.append('--pufferfish-body-diagnostic')
    return subprocess.run(command,cwd=root).returncode


if __name__=='__main__':
    sys.exit(main())
