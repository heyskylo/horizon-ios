#!/usr/bin/env python3
"""Live contents SlotSource over three supported components, with strict output."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path)
    parser.add_argument('--prepare',action='store_true')
    parser.add_argument('--provider-empty-control',action='store_true')
    args=parser.parse_args()
    root=Path(__file__).resolve().parents[1]
    folder = 'item-contents-provider-empty-control' if args.provider_empty_control else 'item-contents-parity'
    command=[sys.executable,str(root/'tools/test_command_exec_parity.py'),
             '--item-contents-fixture','--work-dir',str(root/'.local'/folder)]
    if args.provider_empty_control:
        command.append('--item-contents-provider-empty-control')
    if args.binary:
        command+=['--binary',str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    return subprocess.run(command,cwd=root).returncode


if __name__=='__main__':
    sys.exit(main())
