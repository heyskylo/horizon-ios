#!/usr/bin/env python3
"""NoAI Dolphin air after actual superclass ticks, in air and water.

Complete scalar/action records stay strict. Full bodies and absolute clocks are
predeclared executable diagnostics; the actual two-tick delta remains strict.
"""
import hashlib
import json
from pathlib import Path
import sys
try:
    from . import harness_args, test_command_exec_parity as driver
except ImportError:
    import harness_args
    import test_command_exec_parity as driver


def subjects():
    return [
        (1, 'air_full', False, True, 4800, [3.5, 201.0, 3.5]),
        (2, 'air_low', False, True, 100, [6.5, 201.0, 3.5]),
        (3, 'water_full', True, True, 4800, [10.5, 201.0, 10.5]),
        (4, 'water_low', True, True, 100, [12.5, 201.0, 10.5]),
        (5, 'normal_water', True, False, 4800, [10.5, 201.0, 13.5]),
    ]


def program(full=False):
    ops = []

    def cmd(text, label, expected=None, admission=True, diagnostic=False):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if expected is not None:
            op['expect_result'] = expected
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def tick(n, label):
        cmd(f'/tick step {n}', label + '-request')
        ops.append(dict(op='tick', n=n))

    cmd('/tick freeze', 'freeze')
    for key in ['spawn_mobs', 'advance_time', 'mob_griefing']:
        cmd('/gamerule ' + key + ' false', key)
    cmd('/gamerule random_tick_speed 0', 'random-ticks')
    cmd('/forceload add 0 0 15 15', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/fill 0 200 0 15 205 15 air', 'clear')
    cmd('/fill 8 200 8 15 204 15 glass', 'pool-shell', 320)
    cmd('/fill 9 200 9 14 204 14 water', 'pool-water', 180)
    cmd('/tp @s 1.5 200 1.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, value in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, value)
    cmd('/scoreboard objectives add h072 dummy', 'objective')
    cmd('/execute store result score start h072 run time query gametime', 'clock-start', diagnostic=True)
    for i, label, water, no_ai, initial, pos in subjects():
        xyz = ' '.join(map(str, pos))
        body = '{UUID:[I;72,0,0,%d],Tags:["h072_%s"],Health:10f,NoAI:%db,NoGravity:1b,PersistenceRequired:1b,Air:%ds,attributes:[{id:"minecraft:movement_speed",base:0.0d}]}' % (i, label, no_ai, initial)
        cmd(f'/summon dolphin {xyz} {body}', label + '-summon')
        cmd(f'/execute if loaded {int(pos[0])} 201 {int(pos[2])}', label + '-loaded', 1)
        cmd(f'/execute if block {int(pos[0])} 199 {int(pos[2])} stone', label + '-floor', 1)
        for y in [201, 202]:
            block = 'water[level=0]' if water else 'air'
            cmd(f'/execute if block {int(pos[0])} {y} {int(pos[2])} {block}', f'{label}-medium-{y}', 1)
    for elapsed in [0, 1, 2]:
        if elapsed:
            tick(1, f'tick{elapsed}')
        for i, label, water, no_ai, initial, pos in subjects():
            uid = f'00000048-0000-0000-0000-{i:012x}'
            phase = f'{label}-{elapsed}'
            body = '{UUID:[I;72,0,0,%d],Tags:["h072_%s"],Health:10f,NoGravity:1b,PersistenceRequired:1b}' % (i, label)
            cmd(f'/execute if entity @e[type=dolphin,nbt={body}]', phase + '-identity', 1)
            cmd(f'/execute {"if" if no_ai else "unless"} entity @e[type=dolphin,nbt={{UUID:[I;72,0,0,{i}],NoAI:1b}}]', phase + '-ai', 1)
            cmd(f'/data get entity {uid} Pos', phase + '-position')
            for field in ['result', 'success']:
                cmd(f'/scoreboard players set {field} h072 -99', phase + '-reset-' + field, -99)
            value = initial if elapsed == 0 else (4800 if no_ai else initial - elapsed)
            cmd(f'/execute store result score result h072 store success score success h072 run data get entity {uid} Air', phase + '-air-read', admission=False)
            cmd('/scoreboard players get result h072', phase + '-air-result', admission=False)
            cmd('/scoreboard players get success h072', phase + '-air-success', 1)
            cmd(f'/execute if entity @e[type=dolphin,nbt={{UUID:[I;72,0,0,{i}],Air:{value}s}}]', phase + '-air-typed', admission=False)
            cmd(f'/data get entity {uid}', phase + '-body', diagnostic=True)
    cmd('/execute store result score end h072 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h072 -= start h072', 'clock-delta-operation', 2)
    cmd('/scoreboard players get end h072', 'clock-delta', 2)
    return ops


def main():
    full = '--full-diagnostic' in sys.argv
    if full:
        sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference')
        reference = Path(sys.argv[i + 1]).resolve().read_bytes()
        del sys.argv[i:i + 2]
    build = harness_args.ensure_work_dir(driver.lab.LOCAL / 'noai-dolphin-air-parity').resolve()
    ops = program(full)
    driver.corpus_module.groups = lambda: [('NoAI dolphin air', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [o for o in ops if o['op'] == 'cmd' and o.get('record', True)]
    java_bytes = (build / 'vanilla.jsonl').read_bytes()
    java = [json.loads(s) for s in java_bytes.splitlines()]
    rows = {o['label']: r for o, r in zip(recorded, java)}
    assert len(recorded) == len(java)
    for elapsed in [0, 1, 2]:
        for i, label, water, no_ai, initial, pos in subjects():
            value = initial if elapsed == 0 else (4800 if no_ai else initial - elapsed)
            assert rows[f'{label}-{elapsed}-air-read']['results'] == [[True, value]]
            assert rows[f'{label}-{elapsed}-air-typed']['results'] == [[True, 1]]
    if '--reference-only' in sys.argv:
        return
    if reference and not full:
        assert reference == java_bytes, 'fresh reference changed'
    comparison = reference or java_bytes
    (build / 'comparison-reference.jsonl').write_bytes(comparison)
    java = [json.loads(s) for s in comparison.splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded) == len(java) == len(native)
    differences = [dict(label=o['label'], expected=a, actual=b) for o, a, b in zip(recorded, java, native) if a != b]
    report = dict(records=len(native), passed=len(native)-len(differences), differences=differences,
                  reference_sha256=hashlib.sha256(comparison).hexdigest(), comparison='complete records; no masks or tolerances')
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
    for o, a, b in zip(recorded, java, native):
        if o['admission'] and not o['label'].endswith('-body') and o['label'] not in ['clock-start', 'clock-end']:
            assert a == b, ('admission', o, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
