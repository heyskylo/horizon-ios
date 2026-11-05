#!/usr/bin/env python3
"""Box selectors through actual Game entity bounds; point distance and sort controls."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--work-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'), '--selector-bounds-fixture',
               '--work-dir', str(args.work_dir.resolve() if args.work_dir else root / '.local/selector-bounds-parity')]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.reference_only:
        command.append('--reference-only')
    return subprocess.run(command, cwd=root).returncode


if __name__ == '__main__':
    sys.exit(main())
