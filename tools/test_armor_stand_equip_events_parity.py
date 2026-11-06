#!/usr/bin/env python3
"""Armor-stand command equipment changes through actual Game and sculk sensors.

Complete records are strict. Full actor/sensor bodies and absolute startup clocks are
predeclared diagnostics; all saved slots and the measured 301-tick delta stay strict.
"""
import hashlib
import json
from pathlib import Path
import sys

import harness_args
import test_command_exec_parity as driver

CASES = [
    ('helmet', 'armor.head', 'iron_helmet', 5, 'head'),
    ('same_helmet', 'armor.head', 'iron_helmet', 0, 'head'),
    ('remove', 'armor.head', 'air', 4, 'head'),
    ('same_empty', 'armor.head', 'air', 0, 'head'),
    ('hand', 'weapon.mainhand', 'stick', 4, 'mainhand'),
    ('first_tick', 'armor.head', 'iron_helmet', 0, 'head'),
]
UUID = '00000082-0000-0000-0000-000000000001'
FIRST = '00000082-0000-0000-0000-000000000002'


def program(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def tick(n, label):
        cmd(f'/tick step {n}', label + '-request')
        ops.append({'op': 'tick', 'n': n})

    def read(text, label, admission=False):
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h130 -99', label + '-reset-' + holder, -99)
        cmd('/execute store result score result h130 store success score success h130 run ' + text,
            label + '-read', admission=admission)
        cmd('/scoreboard players get result h130', label + '-result', admission=admission)
        cmd('/scoreboard players get success h130', label + '-success', admission=admission)

    def stand(number, label):
        uid = UUID if number == 1 else FIRST
        cmd(f'/summon armor_stand 6.5 200 10.5 {{UUID:[I;130,0,0,{number}],Tags:["h130_{label}"],NoGravity:1b,Silent:1b,Health:20f}}', label + '-summon')
        cmd(f'/execute if entity @e[type=armor_stand,nbt={{UUID:[I;130,0,0,{number}],Health:20f,NoGravity:1b,Silent:1b,Pos:[6.5d,200d,10.5d]}}]', label + '-identity', 1)
        cmd(f'/data get entity {uid}', label + '-body', diagnostic=True)
        cmd(f'/execute unless data entity {uid} equipment', label + '-empty-equipment', 1)

    cmd('/tick freeze', 'freeze')
    for key in ['spawn_mobs', 'advance_time', 'mob_griefing']:
        cmd('/gamerule ' + key + ' false', key)
    cmd('/gamerule random_tick_speed 0', 'random-ticks')
    cmd('/forceload add 0 0 15 15', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/fill 0 200 0 15 205 15 air', 'clear')
    cmd('/tp @s 2.5 200 2.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, expected in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, expected)
    for x in [6, 9]:
        cmd(f'/execute if loaded {x} 200 10', f'loaded-{x}', 1)
        cmd(f'/execute if block {x} 199 10 stone', f'support-{x}', 1)
    cmd('/scoreboard objectives add h130 dummy', 'objective')
    cmd('/execute store result score start h130 run time query gametime', 'clock-start', diagnostic=True)
    stand(1, 'ready')
    cmd(f'/execute unless data entity {UUID} attributes[{{id:"minecraft:movement_speed"}}]', 'before-first-tick', 1)
    tick(1, 'first-entity-tick')
    cmd(f'/execute if data entity {UUID} attributes[{{id:"minecraft:movement_speed"}}]', 'first-tick-witness', 1)
    cmd('/setblock 9 200 10 stone strict', 'initial-sensor-cell', 1)
    for case, slot, item, frequency, key in CASES:
        # A removed sensor's scheduled active/cooldown ticks expire while the
        # cell is air. No synthetic listener or tick-queue reset is used.
        cmd('/setblock 9 200 10 air strict', case + '-reset-air', 1)
        tick(45, case + '-drain-old')
        cmd('/setblock 9 200 10 sculk_sensor strict', case + '-sensor', 1)
        tick(2, case + '-sensor-ready')
        cmd('/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase=inactive,power=0]', case + '-ready-state', 1)
        cmd('/data get block 9 200 10 last_vibration_frequency', case + '-ready-frequency', 0)
        cmd('/execute unless data block 9 200 10 listener.event', case + '-no-pending', 1)
        cmd('/execute unless data block 9 200 10 listener.selector.event', case + '-no-candidate', 1)
        uid = FIRST if case == 'first_tick' else UUID
        if case == 'first_tick':
            stand(2, case)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h130 -99', case + '-action-reset-' + holder, -99)
        cmd(f'/execute store result score result h130 store success score success h130 run item replace entity {uid} {slot} with {item}', case + '-change', 1)
        cmd('/scoreboard players get result h130', case + '-action-result', 1)
        cmd('/scoreboard players get success h130', case + '-action-success', 1)
        read(f'data get entity {uid} equipment.{key}', case + '-slot', True)
        cmd(f'/execute if entity @e[type=armor_stand,nbt={{UUID:[I;130,0,0,{2 if case == "first_tick" else 1}],Health:20f,NoGravity:1b,Pos:[6.5d,200d,10.5d]}}]', case + '-live-body', 1)
        for elapsed in range(4):
            if elapsed:
                tick(1, f'{case}-tick{elapsed}')
            phase = 'active' if frequency and elapsed == 3 else 'inactive'
            power = 10 if phase == 'active' else 0
            cmd(f'/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase={phase},power={power}]', f'{case}-state{elapsed}', admission=False)
            if elapsed in [0, 3]:
                read('data get block 9 200 10 last_vibration_frequency', f'{case}-frequency{elapsed}')
            if elapsed in [0, 1, 3]:
                cmd('/data get block 9 200 10', f'{case}-sensor-body{elapsed}', diagnostic=True)
        cmd(f'/data get entity {uid}', case + '-body-after', diagnostic=True)
    cmd('/execute store result score end h130 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h130 -= start h130', 'clock-delta-operation', 301)
    cmd('/scoreboard players get end h130', 'clock-delta', 301)
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
    build = harness_args.ensure_work_dir(driver.lab.LOCAL / 'armor-stand-equip-events-parity').resolve()
    reference_bytes = reference.read_bytes() if reference else None
    assert reference is None or reference != build / 'vanilla.jsonl'
    ops = program(full)
    driver.corpus_module.groups = lambda: [('armor stand command equipment events', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    assert len(recorded) == len(java)
    rows = {op['label']: row for op, row in zip(recorded, java)}
    for case, _, _, frequency, _ in CASES:
        assert rows[case + '-frequency3-read']['results'] == [[True, frequency]], rows[case + '-frequency3-read']
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
        if op['admission'] and not op['label'].endswith(('-body', '-body-after')) and op['label'] not in {'clock-start', 'clock-end'} and '-sensor-body' not in op['label']:
            assert a == b, ('admission', op, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
