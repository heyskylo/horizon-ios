#!/usr/bin/env python3
"""Actual /ride and one world tick: baby/adult pig vehicle attachment.

Complete scalar/vector/relationship records are strict. Full bodies and absolute
startup clocks are retained separately; no scaled-position or generic ride claim.
"""
import hashlib
import json
from pathlib import Path
import sys

import harness_args
import test_command_exec_parity as driver

ACTORS = [
    ('baby_vehicle', [8, 1, 0, 1], (4.5, 200.0, 4.5), -24000),
    ('baby_rider', [8, 1, 0, 2], (4.5, 200.0, 8.5), 0),
    ('adult_vehicle', [8, 1, 0, 3], (10.5, 200.0, 4.5), 0),
    ('adult_rider', [8, 1, 0, 4], (10.5, 200.0, 8.5), 0),
    ('unmounted', [8, 1, 0, 5], (7.5, 200.0, 12.5), 0),
]


def target(name):
    return f'@e[type=minecraft:pig,tag=h008_{name},limit=1]'


def program(full=False):
    ops = []

    def cmd(text, label, expected=None, diagnostic=False, admission=True):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if expected is not None:
            op['expect_result'] = expected
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def stored(text, label, expected=None, admission=True):
        for key in ['result', 'success']:
            cmd(f'/scoreboard players set {key} h008 -99', label + '-reset-' + key, -99)
        cmd('/execute store result score result h008 store success score success h008 run ' + text,
            label + '-read', expected, admission=admission)
        cmd('/scoreboard players get result h008', label + '-result', expected, admission=admission)
        cmd('/scoreboard players get success h008', label + '-success', admission=admission)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 8.5 205 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    for key, expected in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, expected)
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    cmd('/scoreboard objectives add h008 dummy', 'objective')
    cmd('/execute store result score start h008 run time query gametime', 'clock-start', diagnostic=True)
    for name, uuid, pos, age in ACTORS:
        x, y, z = pos
        for at, block in [((int(x), 199, int(z)), 'stone'), ((int(x), 200, int(z)), 'air'),
                          ((int(x), 201, int(z)), 'air')]:
            coords = ' '.join(map(str, at))
            cmd('/execute if loaded ' + coords, name + '-loaded-' + str(at[1]), 1)
            cmd(f'/execute if block {coords} {block}', name + '-block-' + str(at[1]), 1)
        nbt_uuid = '[I;' + ','.join(map(str, uuid)) + ']'
        cmd(f'/summon minecraft:pig {x} {y} {z} '
            f'{{UUID:{nbt_uuid},Tags:["h008_{name}"],Age:{age},NoAI:1b,NoGravity:1b,PersistenceRequired:1b}}',
            name + '-summon', 1)
        cmd(f'/execute if entity {target(name)[:-1]},nbt={{UUID:{nbt_uuid}}}]', name + '-selected-identity', 1)
        for key, expected in [('UUID', 4), ('Health', 10), ('Pos', 3), ('Age', age),
                              ('NoAI', 1), ('NoGravity', 1), ('PersistenceRequired', 1), ('Tags', 1)]:
            cmd(f'/data get entity {target(name)} {key}', name + '-initial-' + key, expected)
        cmd('/data get entity ' + target(name), name + '-initial-body', diagnostic=True)
    stored('ride ' + target('unmounted') + ' dismount', 'initial-unmounted')
    for kind in ['baby', 'adult']:
        stored(f'ride {target(kind + "_rider")} mount {target(kind + "_vehicle")}', kind + '-mount', 1)
    cmd('/tick step 1', 'step-one', 1)
    ops.append({'op': 'tick', 'n': 1})
    for name, uuid, _, age in ACTORS:
        nbt_uuid = '[I;' + ','.join(map(str, uuid)) + ']'
        cmd(f'/execute if entity {target(name)[:-1]},nbt={{UUID:{nbt_uuid}}}]', name + '-final-identity', 1)
        for key, expected in [('UUID', 4), ('Health', 10), ('Age', age + 1 if age < 0 else age),
                              ('NoAI', 1), ('NoGravity', 1), ('Tags', 1)]:
            cmd(f'/data get entity {target(name)} {key}', name + '-final-' + key, expected)
        stored('data get entity ' + target(name) + ' Pos', name + '-final-Pos', 3,
               admission=name not in ['baby_rider', 'adult_rider'])
        cmd('/data get entity ' + target(name) + ' Motion', name + '-final-Motion', 3)
        cmd('/data get entity ' + target(name), name + '-final-body', diagnostic=True)
    for kind, uuid in [('baby', ACTORS[0][1]), ('adult', ACTORS[2][1])]:
        stored(f'execute as {target(kind + "_rider")} on vehicle run data get entity @s UUID',
               kind + '-vehicle-UUID', 4)
        cmd(f'/data get entity {target(kind + "_vehicle")} Passengers[0].UUID', kind + '-passenger-UUID', 4)
    stored('ride ' + target('unmounted') + ' dismount', 'final-unmounted')
    cmd('/execute store result score end h008 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h008 -= start h008', 'clock-delta-operation', 1)
    cmd('/scoreboard players get end h008', 'clock-delta', 1)
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
    build = harness_args.ensure_work_dir(driver.lab.LOCAL / 'baby-pig-attachment-parity').resolve()
    assert reference is None or reference != build / 'vanilla.jsonl'
    reference_bytes = reference.read_bytes() if reference else None
    ops = program(full)
    driver.corpus_module.groups = lambda: [('baby pig passenger attachment', ops)]
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
