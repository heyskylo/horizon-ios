#!/usr/bin/env python3
"""Real connected-player save fixture; see docs/parity/connected-statistics-save.md.

    python3 tools/test_connected_statistics_save_parity.py vanilla SERVER_JAR NEW_OUT
    python3 tools/test_connected_statistics_save_parity.py horizon GAME NEW_OUT SUPERVISOR
    python3 tools/test_connected_statistics_save_parity.py --verify EXISTING_OUT

Uses an existing paper-pumpkin-parity/framework checkout, selected by
HORIZON_STATISTIC_FRAMEWORK (default: the sibling paper-pumpkin-parity checkout).
No framework or game files are vendored here. The fixture retains every failed
record before this wrapper returns a nonzero status.
"""
import json
from pathlib import Path
import runpy
import sys


def verify(out):
    checks = json.loads((out / 'strict-checks.json').read_text())
    clocks = json.loads((out / 'clocks.json').read_text())
    cleanup = json.loads((out / 'cleanup.json').read_text())
    assert len(checks) == 87, 'incomplete fixture'
    assert len(clocks) == 4 and all(c['delta'] == 2 for c in clocks), 'clock admission'
    assert cleanup['exit_code'] == 0, 'unclean server shutdown'
    failed = [c for c in checks if not c['passed']]
    print(json.dumps({'passed': len(checks) - len(failed), 'total': len(checks),
                      'world_ticks': sum(c['delta'] for c in clocks), 'failures': failed}, indent=2))
    return bool(failed)


def main():
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == '--verify':
        return verify(Path(args[1]))
    if args in (['--help'], ['-h']):
        print(__doc__)
        return 0
    if not args or not ((args[0] == 'vanilla' and len(args) == 3)
                        or (args[0] == 'horizon' and len(args) == 4)):
        raise SystemExit(__doc__)
    fixture = Path(__file__).parent / 'probes/connected_statistics_save/fixture.py'
    sys.argv = [str(fixture), *args]
    runpy.run_path(str(fixture), run_name='__main__')
    return verify(Path(args[2]))


if __name__ == '__main__':
    raise SystemExit(main())
