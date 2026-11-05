#!/usr/bin/env python3
"""Exact namespace round trips at zero world ticks: focused103 or full106 empty diagnostic."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument("--empty-diagnostic", action="store_true", help="unchanged full106, including the separate empty-component elision residual")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--map-decoration-namespace-fixture', '--work-dir',
               str(root / '.local/map-decoration-namespace-parity')]
    if args.empty_diagnostic:
        command.append("--map-decoration-empty-diagnostic")
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    for name in ['reference_only', 'prepare']:
        if getattr(args, name):
            command.append('--' + name.replace('_', '-'))
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
