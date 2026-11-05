#!/usr/bin/env python3
"""Exact real Game/Java lava-orb health/absence, with an unchanged air control."""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys


def check_raw_vectors(folder):
    """Keep raw comparison failures; independently reject invalid impulses."""
    facts = []
    for implementation in ['vanilla', 'rust']:
        rows = [json.loads(line) for line in (folder / (implementation + '.jsonl')).read_text().splitlines()]
        first_tick = {}
        for field in ['Pos', 'Motion']:
            command = f'/data get entity 00000131-0000-0000-0000-000000000001 {field}'
            observed = [row for row in rows if row['cmd'] == command]
            row = observed[1]  # Initial and then exactly one authoritative tick.
            text = ''.join(part[0] for part in row['messages'][0])
            values = [float(value.strip().removesuffix('d'))
                      for value in text.partition(': [')[2].removesuffix(']').split(',')]
            assert len(values) == 3 and all(math.isfinite(v) for v in values), row
            first_tick[field] = values
        x, y, z = first_tick['Pos']
        assert 9.3 <= x <= 9.7 and 201.0 <= y <= 202.0 and 3.3 <= z <= 3.7, first_tick
        x, y, z = first_tick['Motion']
        assert abs(x) <= 0.2 and 0.0 < y <= 0.21 and abs(z) <= 0.2, first_tick
        facts.append({'implementation': implementation, 'first_tick': first_tick})
    (folder / 'raw-vector-guards.json').write_text(json.dumps(facts, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--raw-diagnostic', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    folder = 'orb-lava-raw-diagnostic' if args.raw_diagnostic else 'orb-lava-parity'
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--orb-lava-fixture', '--work-dir', str(root / '.local' / folder)]
    if args.raw_diagnostic:
        command.append('--orb-lava-raw-diagnostic')
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.prepare:
        command.append('--prepare')
    result = subprocess.run(command, cwd=root).returncode
    if args.raw_diagnostic:
        check_raw_vectors(root / '.local' / folder)
    return result


if __name__ == '__main__':
    sys.exit(main())
