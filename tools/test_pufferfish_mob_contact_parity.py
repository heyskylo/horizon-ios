#!/usr/bin/env python3
"""Pufferfish mob contacts through actual world ticks, including NoAI.

The default218-record gate omits exactly two additive poison-duration reads.
--duration-diagnostic keeps the unchanged strict220-record capture, including
the independent ordinary-AI119 versus Java118 event-order seam.
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
    parser.add_argument("--duration-diagnostic", action="store_true",
                        help="retain both duration reads and their independent event-order residual")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--pufferfish-mob-fixture', '--work-dir', str(root / '.local/pufferfish-mob-contact-parity')]
    if args.duration_diagnostic:
        command.append("--pufferfish-mob-duration-diagnostic")
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    if args.reference_only:
        command.append('--reference-only')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
