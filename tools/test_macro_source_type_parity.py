#!/usr/bin/env python3
"""Macro argument types through actual storage, functions and result callbacks."""
from pathlib import Path
import subprocess
import sys


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    raise SystemExit(subprocess.run([
        sys.executable, str(root / 'tools/test_command_exec_parity.py'),
        '--macro-source-type-fixture', '--work-dir', str(root / '.local/macro-source-type-parity'),
        *sys.argv[1:],
    ], cwd=root).returncode)
