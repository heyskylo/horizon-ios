#!/usr/bin/env python3
"""Actual armor-stand tick/save state and a stationary NoGravity control."""
from pathlib import Path
import subprocess
import sys


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    raise SystemExit(subprocess.run([
        sys.executable, str(root / 'tools/test_command_exec_parity.py'),
        '--stand-fall-fixture', '--work-dir', str(root / '.local/stand-fall-save-parity'),
        *sys.argv[1:],
    ], cwd=root).returncode)
