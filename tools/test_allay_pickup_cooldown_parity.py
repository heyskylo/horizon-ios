#!/usr/bin/env python3
"""Strict actual Game/Java saved item pickup cooldown of an allay."""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver
from allay_pickup_cooldown_corpus import allay_pickup_cooldown

if __name__ == '__main__':
    full = '--full-diagnostic' in sys.argv
    if full:
        sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference')
        reference = Path(sys.argv[i + 1]).resolve()
        del sys.argv[i:i + 2]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'allay-pickup-cooldown-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = allay_pickup_cooldown(full)
    driver.corpus_module.groups = lambda: [('allay pickup cooldown', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(java) == len(recorded)
    for op, row in zip(recorded, java):
        if 'expect_results' in op:
            assert row['results'] == op['expect_results'], ('Java action/callback admission', op, row)
    if '--reference-only' not in sys.argv:
        source = build / 'vanilla.jsonl' if full else (reference or build / 'vanilla.jsonl')
        if reference and not full:
            assert source.read_bytes() == (build / 'vanilla.jsonl').read_bytes(), 'fresh Java bytes changed'
        expected = [json.loads(s) for s in source.read_text().splitlines()]
        native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
        assert len(expected) == len(native) == len(recorded)
        for op, a, b in zip(recorded, expected, native):
            if op['admission'] and not full:
                assert a == b, ('actual actor/state/control admission', op, a, b)
        # `rand-` rows are reported, not compared: the natural allay's
        # flight starts at its first item scan, `Sensor.randomlyDelayStart` with the allay's own random,
        # which is `RandomSource.create()` (seeded from the clock).
        # Two vanilla runs of the same script disagree on them.
        random = [op['label'] for op, a, b in zip(recorded, expected, native)
                  if op['label'].startswith('rand-') and a != b]
        compared = [(op, a, b) for op, a, b in zip(recorded, expected, native) if not op['label'].startswith('rand-')]
        differences = [{'label': op['label'], 'expected': a, 'actual': b} for op, a, b in compared if a != b]
        report = {'records': len(compared), 'passed': len(compared) - len(differences),
                  'random_rows': len(native) - len(compared), 'random_rows_differing': random,
                  'comparison': 'complete raw records, no masks/tolerances',
                  'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                  'fresh_java_sha256': hashlib.sha256((build / 'vanilla.jsonl').read_bytes()).hexdigest(),
                  'differences': differences}
        (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'Strict raw records: {report["passed"]}/{report["records"]}')
        if differences:
            raise SystemExit(1)
