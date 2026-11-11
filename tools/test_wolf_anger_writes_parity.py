#!/usr/bin/env python3
"""Wolf live anger writes in a fresh world with a guarded, real-tick setup clock.

--clock-diagnostic retains the separate loaded-NoAI clock-drift tail, with its
raw absolute ends. It is not part of the default zero-tick live-write gate.
"""
import argparse
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--clock-diagnostic', action='store_true')
    parser.add_argument('--allow-gaps', action='store_true')
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--work-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--only', 'wolf anger', '--work-dir',
               str(args.work_dir.resolve() if args.work_dir else root / '.local/wolf-anger-writes-parity')]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    for enabled, flag in [(args.prepare, '--prepare'), (args.allow_gaps, '--allow-gaps'),
                          (args.reference_only, '--reference-only'),
                          (args.clock_diagnostic, '--wolf-anger-clock-diagnostic')]:
        if enabled:
            command.append(flag)
    sys.exit(subprocess.run(command, cwd=root).returncode)


if __name__ == '__main__':
    main()
