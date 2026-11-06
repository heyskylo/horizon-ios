#!/usr/bin/env python3
"""Damage's Codec.INT conversion through actual item commands and saved chest stacks.

Complete records are strict. Parent block tags and absolute startup clocks are
predeclared diagnostics; all saved stacks and the zero relative clock stay strict.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

# Number.intValue: float/double truncate, long narrows; validation stays upstream.
CASES = [
    ('plain', None, 0), ('integer', '1', 1), ('float', '1.5f', 1),
    ('double', '2.75d', 2), ('negative_half', '-0.5f', 0),
    ('long_wrap', '4294967297L', 1), ('float_max', '2147483648.0f', 2147483647),
    ('double_max', '1.0e20d', 2147483647),
    ('negative_float', '-1.5f', None), ('negative_double', '-1.5d', None),
    ('long_negative', '2147483648L', None), ('negative_overflow', '-1.0e20d', None),
]


def program(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def read(path, label, result=None, admission=False):
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h087 -99', label + '-reset-' + holder, -99)
        cmd(f'/execute store result score result h087 store success score success h087 '
            f'run data get block 4 200 4 {path}', label + '-read', result, admission)
        cmd('/scoreboard players get result h087', label + '-result', admission=admission)
        cmd('/scoreboard players get success h087', label + '-success', admission=admission)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]', 'chest-place', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    for text, label in [('loaded 4 200 4', 'chest-loaded'), ('block 4 199 4 stone', 'support'),
                        ('block 4 200 4 chest[facing=north,type=single,waterlogged=false]', 'chest-state')]:
        cmd('/execute if ' + text, label, 1)
    cmd('/tp @s 8.5 200 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, expected in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, expected)
    for key, expected in [('id', 15), ('x', 4), ('y', 200), ('z', 4), ('Items', 0)]:
        cmd('/data get block 4 200 4 ' + key, 'initial-' + key, expected)
    cmd('/scoreboard objectives add h087 dummy', 'objective')
    cmd('/execute store result score start h087 run time query gametime', 'clock-start', diagnostic=True)
    for slot, (case, source, expected) in enumerate(CASES):
        path = f'Items[{{Slot:{slot}b}}]'
        cmd(f'/execute unless data block 4 200 4 {path}', case + '-empty', 1)
        if expected is None:
            cmd(f'/item replace block 4 200 4 container.{slot} with stone[custom_data={{keep:87}}] 2', case + '-sentinel', 1)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h087 -99', case + '-write-reset-' + holder, -99)
        parts = ['custom_data={keep:87}']
        if source is not None:
            parts.append('damage=' + source)
        spec = 'diamond_pickaxe[' + ','.join(parts) + ']'
        cmd(f'/execute store result score result h087 store success score success h087 '
            f'run item replace block 4 200 4 container.{slot} with {spec}', case + '-write',
            1 if expected is not None else None, expected is not None)
        cmd('/scoreboard players get result h087', case + '-write-result', admission=expected is not None)
        cmd('/scoreboard players get success h087', case + '-write-success', admission=expected is not None)
        for key, count in [('id', len('minecraft:stone') if expected is None else len('minecraft:diamond_pickaxe')),
                           ('count', 2 if expected is None else 1), ('Slot', slot)]:
            cmd(f'/data get block 4 200 4 {path}.{key}', case + '-' + key, count)
        read(path, case + '-stack')
        read(path + '.components."minecraft:damage"', case + '-damage')
        read(path + '.components."minecraft:custom_data".keep', case + '-custom', 87, True)
        cmd(f'/execute if data block 4 200 4 Items[{{Slot:{slot}b,components:{{"minecraft:damage":{expected}}}}}]'
            if expected else f'/execute unless data block 4 200 4 {path}.components."minecraft:damage"',
            case + '-typed-damage', 1, admission=False)
    cmd('/data get block 4 200 4 Items', 'final-stacks', len(CASES), admission=False)
    cmd('/data get block 4 200 4', 'parent-block', diagnostic=True)
    cmd('/execute store result score end h087 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h087 -= start h087', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h087', 'clock-delta', 0)
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
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'numeric-damage-component-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    reference_bytes = reference.read_bytes() if reference else None
    assert reference is None or reference != build / 'vanilla.jsonl'
    ops = program(full)
    driver.corpus_module.groups = lambda: [('numeric damage component', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(recorded) == len(java)
    rows = {op['label']: row for op, row in zip(recorded, java)}
    for case, _, expected in CASES:
        if expected is None:
            assert rows[case + '-write']['results'] == [], rows[case + '-write']
        else:
            callbacks = [[True, expected]] if expected else [[False, 0]]
            assert rows[case + '-damage-read']['results'] == callbacks, rows[case + '-damage-read']
    if '--reference-only' in sys.argv:
        return
    comparison = reference_bytes or (build / 'vanilla.jsonl').read_bytes()
    if reference_bytes and not full:
        assert reference_bytes == (build / 'vanilla.jsonl').read_bytes(), 'fresh reference changed'
    (build / 'comparison-reference.jsonl').write_bytes(comparison)
    java = [json.loads(s) for s in comparison.decode().splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(recorded) == len(java) == len(native)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, java, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences), 'differences': differences,
              'comparison': 'complete raw records, no masks/tolerances',
              'reference_sha256': hashlib.sha256(comparison).hexdigest()}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    for op, a, b in zip(recorded, java, native):
        if op['admission'] and op['label'] not in {'clock-start', 'parent-block', 'clock-end'}:
            assert a == b, ('admission', op, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
