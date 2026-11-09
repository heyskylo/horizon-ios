#!/usr/bin/env python3
"""Natural crafter first-match priority, in separate world-pack and vanilla worlds.

Reuse the actual Game / pinned Java command runner, but keep the overriding
recipe pack out of every other command group's registry and count controls.
"""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='use this built Game binary')
    parser.add_argument('--prepare', action='store_true')
    args = parser.parse_args()
    runner = Path(__file__).with_name('test_command_exec_parity.py')
    build = runner.resolve().parents[1] / '.local/crafting-priority-parity'
    for phase, group in [('world', 'crafting recipe priority'), ('base', 'crafting base recipe control')]:
        command = [sys.executable, str(runner), '--only', group, '--work-dir', str(build / phase)]
        if args.binary:
            command += ['--binary', str(args.binary.resolve())]
        if args.prepare:
            command.append('--prepare')
        result = subprocess.run(command, check=False)
        if result.returncode:
            return result.returncode
    return 0


if __name__ == '__main__':
    sys.exit(main())
