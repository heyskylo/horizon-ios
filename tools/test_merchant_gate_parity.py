#!/usr/bin/env python3
"""Real owner trades: strict command/menu state plus complete raw diagnostics."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

import lab
import test_rust_parity as base


def validate_stream(rows):
    if not rows or rows[0].get('event') != 'start' or rows[-1].get('event') != 'end':
        raise ValueError('missing bounded stream start/end')
    header, footer = rows[0], rows[-1]
    if not isinstance(header.get('uuid'), str) or len(header['uuid']) != 32:
        raise ValueError('missing login UUID')
    if footer.get('channel_open') is False or footer.get('flushed') is False:
        raise ValueError('closed/unflushed embedded channel')
    packets = 0
    elapsed = -1
    for row in rows[1:-1]:
        if row.get('event') == 'control':
            continue
        if row.get('event') != 'packet' or row.get('connection') != header['connection']:
            raise ValueError('unexpected/mixed connection record')
        if row.get('sequence') != packets or int(row['elapsed_ns']) < elapsed:
            raise ValueError('packet sequence/time gap')
        if row.get('dir') != 's2c' or row.get('state') != 'play':
            raise ValueError('wrong protocol direction/state')
        if len(bytes.fromhex(row['data'])) != row['size']:
            raise ValueError('truncated payload')
        elapsed = int(row['elapsed_ns'])
        packets += 1
    if packets == 0 or footer.get('packets') != packets:
        raise ValueError('empty/incomplete packet stream')
    return packets


def reject_incomplete_java_traces(build):
    # Derive negative captures only from an independently decoded fresh Java stream.
    # They are retained as invalid inputs, never substituted into the parity run.
    import copy
    rows = [json.loads(line) for line in (build / 'java-packets.jsonl').read_text().splitlines()]
    validate_stream(rows)
    packets = [n for n, row in enumerate(rows) if row.get('event') == 'packet']
    invalid = {}
    invalid['missing-footer'] = copy.deepcopy(rows[:-1])
    invalid['missing-packet'] = copy.deepcopy(rows[:packets[1]] + rows[packets[1]+1:])
    invalid['truncated-payload'] = copy.deepcopy(rows)
    invalid['truncated-payload'][packets[0]]['data'] = rows[packets[0]]['data'][:-2]
    invalid['closed-channel'] = copy.deepcopy(rows)
    invalid['closed-channel'][-1]['channel_open'] = False
    invalid['empty-stream'] = [copy.deepcopy(rows[0]), {'event':'end','packets':0}]
    folder = build / 'invalid-reader-controls'
    folder.mkdir(exist_ok=True)
    report = {}
    for name, capture in invalid.items():
        (folder / f'{name}.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in capture))
        try:
            validate_stream(capture)
        except ValueError as error:
            report[name] = str(error)
        else:
            raise ValueError(f'incomplete reader accepted {name}')
    (folder / 'report.json').write_text(json.dumps(report, indent=2))


def validate_receipts(build, side):
    script = [json.loads(line) for line in (build / 'script.jsonl').read_text().splitlines()]
    receipts = [json.loads(line) for line in (build / f'{side}-receipts.jsonl').read_text().splitlines()]
    if len(receipts) != len(script):
        raise ValueError(f'{side}: incomplete operation receipts {len(receipts)}/{len(script)}')
    identities = set()
    for n, (op, receipt) in enumerate(zip(script, receipts)):
        if receipt['sequence'] != n or receipt['operation'] != op:
            raise ValueError(f'{side}: altered/missing command or click receipt {n}')
        actual = receipt['actual']
        identities.add((actual['connection'], actual['uuid']))
        if side == 'java' and not (actual.get('connected') and actual.get('channel_active') and actual.get('flushed')):
            raise ValueError('Java action receipt is closed or unflushed')
        if type(actual['game_time']) is not int:
            raise ValueError(f'{side}: missing actual world clock')
    if len(identities) != 1:
        raise ValueError(f'{side}: connection/player changed: {identities}')
    rows = [json.loads(line) for line in (build / f'{side}-packets.jsonl').read_text().splitlines()]
    validate_stream(rows)
    header = rows[0]
    if header['uuid'] != next(iter(identities))[1]:
        raise ValueError(f'{side}: packet stream is not this actual owner')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--strict-all-roundtrip', action='store_true',
                        help='add the executable all-packet byte check, retaining broader order diagnostics')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--read-existing', action='store_true', help='validate retained raw captures without rerunning either world')
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / 'merchant-gate-parity')
    args = parser.parse_args()
    build = args.work_dir.resolve()
    command = [sys.executable, str(lab.ROOT / 'tools/test_command_exec_parity.py'),
               '--merchant-gate-fixture', '--loaded-fixture', '--work-dir', str(build)]
    if args.binary:
        command += ['--binary', str(args.binary.resolve())]
    if args.reference_only:
        command += ['--reference-only']
    result = subprocess.CompletedProcess(command, 0) if args.read_existing else subprocess.run(command, cwd=lab.ROOT)
    # A prerequisite-aborted run remains invalid. Never turn partial captures green.
    if not (build / 'vanilla.jsonl').is_file():
        return result.returncode or 1
    lock, jars, libs = base.verify_artifacts()
    classes = build / 'trace-classes'
    classes.mkdir(exist_ok=True)
    sources = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity'
    dependencies = os.pathsep.join(map(str, [jars['server'], *libs]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-cp', dependencies, '-d', str(classes), str(sources / 'ParityData.java'),
                    str(sources / 'VanillaMerchantTraceReader.java')], check=True)
    reject_incomplete_java_traces(build)
    if args.read_existing:
        print('Trace-only validation of retained captures; this does not rerun the Game comparison.', flush=True)
    trace_failure = 0
    for side in ['java'] if args.reference_only else ['java', 'rust']:
        validate_receipts(build, side)
        mode = 'strict-all' if args.strict_all_roundtrip else 'decoded'
        log_path = build / (f'{side}-strict-all-reader.log' if args.strict_all_roundtrip else f'{side}-trace-reader.log')
        with log_path.open('w') as log:
            reader = subprocess.run(['java', '-Xmx2G', '-cp', os.pathsep.join([str(classes), dependencies]),
                            'dev.horizonlab.parity.VanillaMerchantTraceReader',
                            str(build / f'{side}-packets.jsonl'),
                            str(build / f'{side}-{mode}-packets.jsonl'),
                            *(['--strict-all'] if args.strict_all_roundtrip else [])],
                           cwd=build, stdout=log, stderr=subprocess.STDOUT)
        if reader.returncode:
            print(f'{side}: packet validation failed ({reader.returncode}); see {log_path}', flush=True)
            trace_failure = reader.returncode
    return result.returncode or trace_failure


if __name__ == '__main__':
    sys.exit(main())
