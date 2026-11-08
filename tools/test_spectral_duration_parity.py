#!/usr/bin/env python3
"""Actual spectral-arrow save reads; wider full NBT/name/type replies stay diagnostic."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--read-existing', action='store_true', help='diagnostics only; no Game rerun')
    parser.add_argument('--strict-diagnostic', action='store_true', help='also fail on any retained full/style/type diagnostic difference')
    parser.add_argument('--work-dir', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    folder = (args.work_dir or root / '.local/spectral-duration-parity').resolve()
    command = [sys.executable, str(root / 'tools/test_command_exec_parity.py'),
               '--spectral-duration-fixture', '--work-dir', str(folder)]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    for name in ['reference_only', 'prepare']:
        if getattr(args, name):
            command.append('--' + name.replace('_', '-'))
    result = 0 if args.read_existing else subprocess.run(command, cwd=root).returncode
    if not (folder / 'vanilla.jsonl').exists():
        return result or 1
    captures = {}
    from spectral_duration_corpus import DIAGNOSTIC_RECORDS
    for side, log in [('java', 'java.log')] + ([] if args.reference_only else [('rust', 'rust.log')]):
        rows = []
        for line in (folder / log).read_text().splitlines():
            marker = 'command trace: '
            if marker in line:
                payload = line.split(marker, 1)[1]
                json.loads(payload)  # Reject malformed/incomplete diagnostics, keep original bytes.
                rows.append(payload)
        (folder / f'{side}-diagnostic.jsonl').write_text(''.join(row + '\n' for row in rows))
        if len(rows) != DIAGNOSTIC_RECORDS:
            raise ValueError(f'{side}: incomplete diagnostics {len(rows)}/{DIAGNOSTIC_RECORDS}')
        captures[side] = [json.loads(row) for row in rows]
    if args.read_existing:
        print('Diagnostic-only read; neither world was rerun.')
    if not args.reference_only:
        java, rust = captures['java'], captures['rust']
        assert [r['cmd'] for r in java] == [r['cmd'] for r in rust], 'diagnostic command sequence differs'
        mismatches = [i for i, (a, b) in enumerate(zip(java, rust)) if a != b]
        (folder / 'diagnostic-report.json').write_text(json.dumps({
            'records': DIAGNOSTIC_RECORDS, 'matching': DIAGNOSTIC_RECORDS-len(mismatches),
            'mismatch_indices': mismatches, 'strict': args.strict_diagnostic}, indent=2)+'\n')
        print(f'Full/style/type diagnostics: {DIAGNOSTIC_RECORDS-len(mismatches)}/{DIAGNOSTIC_RECORDS}; retained separately from Duration contract.')
        if args.strict_diagnostic and mismatches:
            return result or 1
    return result


if __name__ == '__main__':
    sys.exit(main())
