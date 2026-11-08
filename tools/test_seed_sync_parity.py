#!/usr/bin/env python3
"""Fields an unseeded random decides, synced and compared exactly (tools/seed_sync_corpus.py, docs/parity-workflow.md)."""
from pathlib import Path
import subprocess
import sys


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    raise SystemExit(subprocess.run([
        sys.executable, str(root / 'tools/test_command_exec_parity.py'),
        '--only', 'seed sync proof', '--work-dir', str(root / '.local/seed-sync-parity'),
        *sys.argv[1:],
    ], cwd=root).returncode)
