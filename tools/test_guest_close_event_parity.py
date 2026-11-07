#!/usr/bin/env python3
"""Replay the real guest chest-close route against one immutable Java reference.

Complete raw records remain on disk. Absolute scene epochs and whole listener
compounds are declared diagnostics in the corpus; relative clocks and all
measured event/position/callback/store records compare without normalization.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def recorded(ops):
    return [op for op in ops if op['op'] == 'menu_state'
            or op['op'] == 'cmd' and op.get('record', True)
            or op['op'] == 'tick' and op.get('record')]


def compare(ops, reference, native):
    assert len(ops) == len(reference) == len(native), 'incomplete capture'
    differences = [{'row': i, 'operation': op, 'java': a, 'native': b}
                   for i, (op, a, b) in enumerate(zip(ops, reference, native)) if a != b]
    return {
        'records': len(ops), 'matches': len(ops) - len(differences),
        'strict_records': sum(not op.get('diagnostic') for op in ops),
        'strict_matches': sum(a == b for op, a, b in zip(ops, reference, native)
                              if not op.get('diagnostic')),
        'admissions': sum(bool(op.get('h112_admission')) for op in ops),
        'failed_admissions': [d for d in differences if d['operation'].get('h112_admission')],
        'differences': differences,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--side', choices=['java', 'native'], required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference', type=Path, help='retained vanilla.jsonl with sibling script.jsonl')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    command = [sys.executable, str(ROOT / 'tools/test_command_exec_parity.py'),
               '--only', 'guest container close event', '--work-dir', str(work)]
    if args.side == 'java':
        command.append('--reference-only')
        subprocess.run(command, cwd=ROOT, check=True)
        print('Fresh Java admission complete:', work / 'vanilla.jsonl')
        return
    if not args.binary or not args.reference:
        parser.error('native requires --binary and --reference')
    reference_path = args.reference.resolve()
    frozen = reference_path.read_bytes()
    digest = hashlib.sha256(frozen).hexdigest()
    command += ['--binary', str(args.binary.resolve())]
    # The underlying runner keeps its additional fresh Java capture untouched.
    # Its full comparison can fail on declared diagnostics; the strict check
    # below uses the same retained reference for both native binaries.
    result = subprocess.run(command, cwd=ROOT)
    assert reference_path.read_bytes() == frozen, 'reference changed during capture'
    assert (work / 'script.jsonl').read_bytes() == reference_path.with_name('script.jsonl').read_bytes(), 'script changed'
    ops = recorded([json.loads(x) for x in (work / 'script.jsonl').read_text().splitlines()])
    native = [json.loads(x) for x in (work / 'rust.jsonl').read_text().splitlines()]
    reference = [json.loads(x) for x in frozen.decode().splitlines()]
    report = compare(ops, reference, native)
    report.update({'reference_sha256': digest, 'underlying_full_comparison_exit': result.returncode,
                   'additional_fresh_java': str(work / 'vanilla.jsonl')})
    (work / 'guest-close-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f"strict {report['strict_matches']}/{report['strict_records']}; "
          f"full raw {report['matches']}/{report['records']}")
    assert not report['failed_admissions'], 'native setup failed; invalid pair'
    assert report['strict_matches'] == report['strict_records'], 'guest close parity failure'


if __name__ == '__main__':
    main()
