#!/usr/bin/env python3
"""Pufferfish mob contacts through actual world ticks, including NoAI.

The default gate has 218 records and omits exactly two additive duration reads.
--duration-diagnostic keeps the unchanged strict capture of 220 records, including
the independent ordinary-AI duration of 119 versus Java's 118. A separate
same-position pig/cod control also runs by default, or alone with --overlap-control.
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
    parser.add_argument("--overlap-control", action="store_true",
                        help="separate same-position cod/pig contact control")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    phases = [('pufferfish-mob-contact-parity', False)]
    if args.overlap_control:
        phases = [('pufferfish-mob-overlap-control', True)]
    elif not args.duration_diagnostic:
        phases.append(('pufferfish-mob-overlap-control', True))
    status = 0
    for folder, overlap in phases:
        command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
                   '--pufferfish-mob-fixture', '--work-dir', str(root / '.local' / folder)]
        if overlap:
            command.append('--pufferfish-mob-overlap-control')
        if args.duration_diagnostic:
            command.append('--pufferfish-mob-duration-diagnostic')
        if args.binary:
            command += ['--binary', str(args.binary.resolve())]
        if args.prepare:
            command.append('--prepare')
        if args.reference_only:
            command.append('--reference-only')
        status = max(status, subprocess.run(command, cwd=root).returncode)
    return status


if __name__ == '__main__':
    sys.exit(main())
