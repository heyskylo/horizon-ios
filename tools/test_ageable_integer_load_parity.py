#!/usr/bin/env python3
"""Ageable/Animal integer NBT loading through actual Game and fresh pinned Java.

Zero observation ticks. Full bodies and absolute startup clocks remain executable
strict diagnostics; all scalar values/types, feedback/callbacks/stores and actor
admissions are compared without masks or tolerances.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

# Independently source-derived expectations, required to pass fresh Java before
# the native capture. All floating inputs are finite; no NaN/boolean expansion.
CASES = [
    ('absent', None, 0), ('zero', '0', 0), ('byte', '-7b', -7), ('short', '13s', 13),
    ('int_min', '-2147483648', -2147483648), ('int_max', '2147483647', 2147483647),
    ('long_above', '2147483648L', -2147483648), ('long_below', '-2147483649L', 2147483647),
    ('long_wrap', '4294967296L', 0), ('long_max', '9223372036854775807L', -1),
    ('long_min', '-9223372036854775808L', 0),
    ('float_pos', '2.5f', 2), ('float_neg', '-2.5f', -3),
    ('double_pos', '2.5d', 2), ('double_neg', '-2.5d', -3),
    ('float_inside', '2147483520f', 2147483520), ('float_above', '2147483648f', 2147483647),
    ('float_min', '-2147483648f', -2147483648), ('float_below', '-2147483904f', -2147483648),
    ('double_inside', '2147483647.75d', 2147483647), ('double_above', '2147483648d', 2147483647),
    ('double_neg_inside', '-2147483647.5d', -2147483648),
    ('double_below', '-2147483648.5d', -2147483648),
    ('double_huge', '1e20d', 2147483647), ('double_neg_huge', '-1e20d', -2147483648),
    ('float_huge', '1e20f', 2147483647), ('float_neg_huge', '-1e20f', -2147483648),
]
FIELDS = ['Age', 'ForcedAge', 'InLove']


def program(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/execute if loaded 4 200 4', 'loaded', 1)
    cmd('/execute if block 4 199 4 stone', 'actor-support', 1)
    cmd('/execute if block 4 200 4 air', 'actor-air', 1)
    cmd('/tp @s 12.5 200 12.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, expected in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, expected)
    cmd('/scoreboard objectives add h055 dummy', 'objective')
    cmd('/execute store result score start h055 run time query gametime', 'clock-start', diagnostic=True)
    for n, (case, source, expected) in enumerate(CASES, 1):
        uuid = f'00000037-0000-0000-0000-{n:012x}'
        values = '' if source is None else ',' + ','.join(k + ':' + source for k in FIELDS)
        cmd(f'/summon pig 4.5 200 4.5 {{UUID:[I;55,0,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,'
            f'PersistenceRequired:1b,AgeLocked:1b,Tags:["h055"]{values}}}', case + '-summon', 1)
        cmd(f'/execute as {uuid} if entity @s[type=pig,tag=h055]', case + '-type', 1)
        for key, result in [('UUID', 4), ('Health', 10), ('Pos', 3), ('NoAI', 1),
                            ('NoGravity', 1), ('PersistenceRequired', 1), ('AgeLocked', 1)]:
            cmd(f'/data get entity {uuid} {key}', case + '-' + key, result)
        for key in FIELDS:
            label = case + '-' + key
            for holder in ['result', 'success']:
                cmd(f'/scoreboard players set {holder} h055 -99', label + '-reset-' + holder, -99)
            cmd(f'/execute store result score result h055 store success score success h055 '
                f'run data get entity {uuid} {key}', label + '-read', expected, admission=False)
            cmd('/scoreboard players get result h055', label + '-result', expected, admission=False)
            cmd('/scoreboard players get success h055', label + '-success', 1, admission=False)
        cmd(f'/data get entity {uuid}', case + '-body', diagnostic=True)
    cmd('/execute store result score end h055 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h055 -= start h055', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h055', 'clock-delta', 0)
    return ops


def main():
    full = '--full-diagnostic' in sys.argv
    if full:
        sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference')
        reference = Path(sys.argv[i + 1]).resolve()
        del sys.argv[i:i + 2]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'ageable-integer-load-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    reference_bytes = reference.read_bytes() if reference else None
    assert reference is None or reference != build / 'vanilla.jsonl', 'reference must remain separate and immutable'
    ops = program(full)
    driver.corpus_module.groups = lambda: [('ageable integer NBT load', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(java) == len(recorded)
    by_label = {op['label']: row for op, row in zip(recorded, java)}
    for case, _, expected in CASES:
        for key in FIELDS:
            for suffix, value in [('read', expected), ('result', expected), ('success', 1)]:
                assert by_label[f'{case}-{key}-{suffix}']['results'] == [[True, value]], 'fresh Java admission'
    if '--reference-only' in sys.argv:
        return
    if reference_bytes:
        assert reference_bytes == (build / 'vanilla.jsonl').read_bytes(), 'fresh reference bytes changed'
    expected = java
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(expected) == len(native) == len(recorded)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, expected, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences), 'differences': differences,
              'comparison': 'complete raw records; no masks or tolerances',
              'reference_sha256': hashlib.sha256((build / 'vanilla.jsonl').read_bytes()).hexdigest()}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    if not full:
        for op, a, b in zip(recorded, expected, native):
            if op['admission']:
                assert a == b, ('fixture admission', op, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
