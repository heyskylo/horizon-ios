#!/usr/bin/env python3
"""Actual command saves of modeled loot-pickup flags, including species overrides."""
from pathlib import Path
import subprocess
import sys


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    raise SystemExit(subprocess.run([
        sys.executable, str(root / 'tools/test_command_exec_parity.py'),
        '--mob-pickup-save-fixture', '--work-dir', str(root / '.local/mob-pickup-save-parity'),
        *sys.argv[1:],
    ], cwd=root).returncode)
