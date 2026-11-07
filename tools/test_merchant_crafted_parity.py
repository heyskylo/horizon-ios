#!/usr/bin/env python3
"""Compare accepted merchant result takes and crafted statistics through real owner clicks."""
import sys
from pathlib import Path
import test_command_exec_parity

if __name__ == '__main__':
    sys.argv += ['--merchant-crafted-fixture']
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(Path('.local/merchant-crafted-parity').resolve())]
    test_command_exec_parity.main()
