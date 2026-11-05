#!/usr/bin/env python3
"""Exercise frozen /damage reinforcement dispatch and a separate later-tick control.

--dump adds complete, unnormalized random callee NBT snapshots to each capture.
Those additive records are diagnostic and are deliberately not the typed gate.
The default immediate gate is208 records; --target-diagnostic retains full209,
including the separate creative-target relation lead. The once gate is135.
"""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--dump', action='store_true')
    parser.add_argument('--target-diagnostic', action='store_true')
    parser.add_argument('--original-diagnostic', action='store_true', help='retain the original174/108 unisolated fixture')
    parser.add_argument('--work-dir', type=Path, help='isolated scratch output root')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    build = args.work_dir.resolve() if args.work_dir else root / '.local/command-reinforcements-parity'
    prefix = ('original-' if args.original_diagnostic else '') + ('raw-' if args.dump else '')
    status = 0
    for phase, group in [('immediate', 'command zombie reinforcements'),
                         ('once', 'command reinforcement once')]:
        command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
                   '--only', group, '--work-dir',
                   str(build / (prefix + phase))]
        if args.binary:
            command += ['--binary', str(args.binary.resolve())]
        if args.prepare:
            command.append('--prepare')
        if args.dump:
            command.append('--reinforcement-raw-diagnostic')
        if args.target_diagnostic:
            command.append('--reinforcement-target-diagnostic')
        if args.original_diagnostic:
            command.append('--reinforcement-original-diagnostic')
        result = subprocess.run(command, cwd=root)
        status = max(status, result.returncode)
    return status


if __name__ == '__main__':
    sys.exit(main())
