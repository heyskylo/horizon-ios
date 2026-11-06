#!/usr/bin/env python3
"""Actual Game/Java frame loaders: absent Facing, and every explicit valid direction.

--full-diagnostic also compares whole tags, Rotation and absolute clocks. These
remain complete raw observations in either mode; the scoped gate compares Facing,
Pos and the explicit class/UUID/block_pos/item/Fixed/rotation controls.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver


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
    cmd('/fill 0 99 0 15 99 15 stone', 'floor', 256)
    cmd('/fill 0 100 0 15 105 15 air', 'air')
    cmd('/tp @s 8.5 100 14.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h134 dummy', 'objective')
    cmd('/execute store result score start h134 run time query gametime',
        'clock-start', diagnostic=True)
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type-name', 1)
    for field, result in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd(f'/data get entity @s {field}', 'owner-' + field, result)
    for glow in [False, True]:
        kind = 'glow_item_frame' if glow else 'item_frame'
        z = 10 if glow else 4
        for n, facing in enumerate([None, 0, 1, 2, 3, 4, 5]):
            x = 2 + n * 2
            index = n + 1 + (7 if glow else 0)
            uuid = f'00000086-0000-0000-0000-{index:012x}'
            label = kind + '-' + ('absent' if facing is None else str(facing))
            direction = '' if facing is None else f',Facing:{facing}b'
            cmd(f'/execute if loaded {x} 102 {z}', label + '-loaded', 1)
            cmd(f'/execute if block {x} 99 {z} stone', label + '-floor', 1)
            cmd(f'/execute if block {x} 102 {z} air', label + '-gap', 1)
            cmd(f'/summon {kind} {x + .5} 102.5 {z + .5} '
                f'{{UUID:[I;134,0,0,{index}],block_pos:[I;{x},102,{z}],Fixed:1b,'
                f'Item:{{id:"minecraft:diamond",count:1}},ItemRotation:5b,ItemDropChance:0.25f{direction}}}',
                label + '-summon', 1)
            cmd(f'/execute as {uuid} if entity @s[type=minecraft:{kind}]', label + '-actual-type', 1)
            for field, result in [('UUID', 4), ('block_pos', 3), ('Fixed', 1),
                                  ('ItemRotation', 5), ('ItemDropChance', 0), ('Item', None)]:
                cmd(f'/data get entity {uuid} {field}', label + '-' + field, result)
            cmd(f'/data get entity {uuid} Facing', label + '-Facing',
                0 if facing is None else facing, admission=facing is not None)
            cmd(f'/data get entity {uuid} Pos', label + '-Pos', 3, admission=facing is not None)
            cmd(f'/data get entity {uuid} Rotation', label + '-Rotation', diagnostic=True)
            cmd(f'/data get entity {uuid}', label + '-full-tag', diagnostic=True)
    cmd('/execute store result score now h134 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation now h134 -= start h134', 'clock-delta-operation', 0)
    cmd('/scoreboard players get now h134', 'clock-delta', 0)
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
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'frame-facing-default-parity')]
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = program(full)
    driver.corpus_module.groups = lambda: [('frame absent Facing and valid directions', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    if '--reference-only' in sys.argv:
        return
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    source = reference or build / 'vanilla.jsonl'
    if reference:
        assert source.read_bytes() == (build / 'vanilla.jsonl').read_bytes(), 'fresh Java bytes changed'
    java = [json.loads(s) for s in source.read_text().splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(java) == len(native) == len(recorded)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, java, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences),
              'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
              'comparison': 'complete raw records, no masks/tolerances', 'differences': differences}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    if not full:
        for op, a, b in zip(recorded, java, native):
            if op['admission']:
                assert a == b, ('actual fixture admission', op, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
