#!/usr/bin/env python3
"""Repeated actual orb UUID views; whole-body traces stay raw diagnostics."""
from pathlib import Path
import subprocess
import sys


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    raise SystemExit(subprocess.run([
        sys.executable, str(root / 'tools/test_command_exec_parity.py'),
        '--orb-uuid-fixture', '--work-dir', str(root / '.local/orb-saved-uuid-parity'),
        *sys.argv[1:],
    ], cwd=root).returncode)
