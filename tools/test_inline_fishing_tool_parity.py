#!/usr/bin/env python3
"""Inline /loot fish TOOL through the existing actual Game command route.

All outputs, slots, presence and callbacks are strict. Only full parent/player
bodies and absolute startup clocks are separately retained diagnostics.
"""
import hashlib
import json
from pathlib import Path
import sys

import harness_args
import test_command_exec_parity as driver


def table(item=None, kind='minecraft:fishing'):
    entry = {'type': 'minecraft:item', 'name': 'minecraft:diamond'}
    if item:
        entry['condition'] = {'type': 'minecraft:match_tool', 'predicate': {'items': 'minecraft:' + item}}
    return json.dumps({'type': kind, 'pools': [{'rolls': 1, 'entries': [entry]}]}, separators=(',', ':'))


CASES = [
    ('rod_rod', 'fish', table('fishing_rod'), 'fishing_rod', 'diamond'),
    ('rod_stick', 'fish', table('fishing_rod'), 'stick', None),
    ('stick_rod', 'fish', table('stick'), 'fishing_rod', None),
    ('stick_stick', 'fish', table('stick'), 'stick', 'diamond'),
    ('unconditional', 'fish', table(), 'stick', 'diamond'),
    ('plain_no_tool', 'loot', table('fishing_rod', 'minecraft:chest'), None, None),
    ('omitted_tool', 'fish', table('fishing_rod'), None, None),
    ('actual_mainhand', 'fish', table('fishing_rod'), 'mainhand', 'diamond'),
    ('named_shears', 'fish', 'minecraft:blocks/cobweb', 'shears', 'cobweb'),
    ('named_stick', 'fish', 'minecraft:blocks/cobweb', 'stick', 'string'),
]


def program(full=False):
    ops = []

    def cmd(text, label, expected=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if expected is not None:
            op['expect_result'] = expected
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def stored(text, label, expected=None, admission=False):
        for key in ['result', 'success']:
            cmd(f'/scoreboard players set {key} h041 -99', label + '-reset-' + key, -99)
        cmd('/execute store result score result h041 store success score success h041 run ' + text,
            label + '-read', expected, admission)
        cmd('/scoreboard players get result h041', label + '-result', expected, admission)
        cmd('/scoreboard players get success h041', label + '-success', admission=admission)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]', 'chest-place', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    for at, block in [('4 199 4', 'stone'), ('4 200 4', 'chest[facing=north,type=single,waterlogged=false]'),
                      ('8 200 8', 'air')]:
        cmd('/execute if loaded ' + at, 'loaded-' + at, 1)
        cmd('/execute if block ' + at + ' ' + block, 'block-' + at, 1)
    cmd('/tp @s 8.5 200 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, expected in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, expected)
    cmd('/item replace entity @s weapon.mainhand with minecraft:fishing_rod', 'held-place', 1)
    for phase in ['initial']:
        cmd('/data get entity @s SelectedItem.id', phase + '-held-id', 21)
        cmd('/data get entity @s SelectedItem.count', phase + '-held-count', 1)
    cmd('/data get entity @s', 'initial-owner-body', diagnostic=True)
    for key, expected in [('id', 15), ('x', 4), ('y', 200), ('z', 4), ('Items', 0)]:
        cmd('/data get block 4 200 4 ' + key, 'initial-chest-' + key, expected)
    cmd('/scoreboard objectives add h041 dummy', 'objective')
    cmd('/execute store result score start h041 run time query gametime', 'clock-start', diagnostic=True)
    for slot, (name, mode, source, tool, output) in enumerate(CASES):
        path = f'Items[{{Slot:{slot}b}}]'
        cmd(f'/execute unless data block 4 200 4 {path}', name + '-slot-initially-empty', 1)
        stored('data get block 4 200 4 ' + path, name + '-pre-slot', admission=True)
        origin = ' 8 200 8' if mode == 'fish' else ''
        tool_arg = (' ' + tool if tool in ['mainhand', 'offhand'] else ' minecraft:' + tool) if tool else ''
        command = f'loot replace block 4 200 4 container.{slot} {mode} {source}{origin}{tool_arg}'
        stored(command, name + '-loot', 1 if output else None)
        stored('data get block 4 200 4 ' + path, name + '-post-slot', 3 if output else None)
        stored(f'execute if data block 4 200 4 {path}', name + '-presence')
        for key, expected in [('id', len('minecraft:' + output) if output else None),
                              ('count', 1 if output else None), ('Slot', slot if output else None)]:
            stored('data get block 4 200 4 ' + path + '.' + key, name + '-' + key, expected)
    cmd('/data get block 4 200 4 Items', 'final-all-stacks', 6, admission=False)
    cmd('/data get entity @s SelectedItem.id', 'final-held-id', 21)
    cmd('/data get entity @s SelectedItem.count', 'final-held-count', 1)
    cmd('/data get block 4 200 4', 'final-parent-block', diagnostic=True)
    cmd('/execute store result score end h041 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h041 -= start h041', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h041', 'clock-delta', 0)
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
    build = harness_args.ensure_work_dir(driver.lab.LOCAL / 'inline-fishing-tool-parity').resolve()
    assert reference is None or reference != build / 'vanilla.jsonl'
    reference_bytes = reference.read_bytes() if reference else None
    ops = program(full)
    driver.corpus_module.groups = lambda: [('inline fishing tool context', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    if '--reference-only' in sys.argv:
        return
    captured = (build / 'vanilla.jsonl').read_bytes()
    if reference_bytes and not full:
        assert captured == reference_bytes, 'fresh canonical reference changed'
    compared = reference_bytes or captured
    (build / 'comparison-reference.jsonl').write_bytes(compared)
    java = [json.loads(s) for s in compared.decode().splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    assert len(recorded) == len(java) == len(native)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, java, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences), 'differences': differences,
              'reference_sha256': hashlib.sha256(compared).hexdigest(), 'comparison': 'complete raw records'}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    if not full:
        for op, a, b in zip(recorded, java, native):
            if op['admission']:
                assert a == b, ('fixture admission', op['label'], a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
