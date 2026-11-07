#!/usr/bin/env python3
"""Real Game/Java loaded-furnace ticks; compare every complete raw record."""
import hashlib
import json
from pathlib import Path
import sys
import test_command_exec_parity as driver
from furnace_loaded_total_corpus import selected_totals, recipes_used_totals

def used_admissions(build, ops, native=False):
    side = 'rust' if native else 'java'
    rows = [json.loads(line.split('command trace: ', 1)[1])
            for line in (build/(side+'.log')).read_text().splitlines() if 'command trace: ' in line]
    commands = [op for op in ops if op['op']=='cmd']
    assert len(rows)==len(commands), ('complete actual trace', side, len(rows), len(commands))
    (build/(side+'-trace.json')).write_text(json.dumps(rows, indent=2)+'\n')
    for op, row in zip(commands, rows):
        assert row['cmd']==op['cmd'], ('trace command identity', op, row)
        parts = op['label'].split('-')
        subject = parts[0] in {'missing','zero','five','normal','started'}
        common = any(op['label'].endswith('-'+field) for field in
                     ['loaded','floor','lit','identity','no-fuel','lit_time_remaining','lit_total_time'])
        admit = not native or not subject or parts[0] in {'normal','started'} or parts[1] in {'0','setup','load'} or common
        if admit and 'expect_result' in op:
            assert row['results']==[[True, op['expect_result']]], (side, 'actual guard', op, row)
        if not native and 'expected_text' in op:
            assert [''.join(span[0] for span in message) for message in row['messages']]==[op['expected_text']], ('typed RecipesUsed', op, row)


if __name__ == '__main__':
    used = '--recipes-used-only' in sys.argv
    if used: sys.argv.remove('--recipes-used-only')
    full = '--full-diagnostic' in sys.argv
    assert not (used and full), 'choose one diagnostic program'
    if full: sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        at = sys.argv.index('--reference')
        reference = Path(sys.argv[at+1]).resolve()
        del sys.argv[at:at+2]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(driver.lab.LOCAL/'furnace-loaded-total-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    ops = recipes_used_totals() if used else selected_totals(full)
    driver.corpus_module.groups = lambda: [('loaded furnace cooking totals', ops)]
    # The shared runner retains its own report; this wrapper always additionally
    # compares untouched complete JSON records, without its presentation masks.
    sys.argv.append('--allow-gaps')
    driver.main()
    if used: used_admissions(build, ops)
    if '--reference-only' not in sys.argv:
        source = reference or build/'vanilla.jsonl'
        if reference and not full:
            assert source.read_bytes() == (build/'vanilla.jsonl').read_bytes(), 'fresh Java canonical reference bytes changed'
        java = [json.loads(s) for s in source.read_text().splitlines()]
        native = [json.loads(s) for s in (build/'rust.jsonl').read_text().splitlines()]
        if used: used_admissions(build, ops, native=True)
        recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
        assert len(java)==len(native)==len(recorded), 'complete record count'
        differences = [{'label': op['label'], 'expected': a, 'actual': b}
                       for op, a, b in zip(recorded, java, native) if a != b]
        report = {'records': len(java), 'passed': len(java)-len(differences),
                  'comparison': 'all declared complete raw JSON records; no masks/tolerances',
                  'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                  'fresh_java_sha256': hashlib.sha256((build/'vanilla.jsonl').read_bytes()).hexdigest(),
                  'full_diagnostic': full,
                  'differences': differences}
        (build/'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
        print(f'Strict raw records: {report["passed"]}/{report["records"]}')
        if differences:
            raise SystemExit(1)
