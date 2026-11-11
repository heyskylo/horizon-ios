#!/usr/bin/env python3
"""Real owner map allocation and frame-map detachment through live Game commands."""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--allow-gaps', action='store_true')
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--work-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--only', 'frame live map detachment', '--work-dir',
               str(args.work_dir.resolve() if args.work_dir else root / '.local/frame-map-writes-parity')]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    for enabled, flag in [(args.prepare, '--prepare'), (args.allow_gaps, '--allow-gaps'),
                          (args.reference_only, '--reference-only')]:
        if enabled:
            command.append(flag)
    sys.exit(subprocess.run(command, cwd=root).returncode)


if __name__ == '__main__':
    main()
