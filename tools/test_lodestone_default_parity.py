#!/usr/bin/env python3
"""Saved LodestoneTracker default with stationary real item entities, and world
steps for a compass worn in the player's armour slots.

The saved-default corpus has no world steps. The armour family adds one player
tick: a compass in the head armour slot whose lodestone is removed must lose
its target, as `ItemStack.inventoryTick` / `LodestoneTracker.tick` do from
`LivingEntity.tick`'s `EntityEquipment` pass. More ticks cover a loaded
target, an untracked tracker and a target in another dimension. `--no-armor`
leaves that family out and reproduces the corpus before it.

--full-diagnostic records whole entity bodies and absolute clocks too. Those
commands remain traced in the scoped program. All saved-component, typed identity,
presence, callback/store and relative-clock records are strict.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver


def program(full=False, armor=True):
    ops = []

    def cmd(text, label, result=None, callback=None, admission=True, diagnostic=False, record=True):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission, 'record': record}
        if result is not None:
            op['expect_result'] = result
        if callback is not None:
            op['expect_callback'] = callback
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/setblock 12 200 12 lodestone', 'lodestone', 1)
    cmd('/execute if block 12 200 12 lodestone', 'target-block', 1)
    cmd('/execute if loaded 12 200 12', 'target-loaded', 1)
    cmd('/tp @s 8.5 200 12.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h329 dummy', 'objective')
    cmd('/execute store result score start h329 run time query gametime', 'clock-start', diagnostic=True)
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for field, result in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd(f'/data get entity @s {field}', 'owner-' + field, result)

    target = 'target:{dimension:"minecraft:overworld",pos:[I;12,200,12]}'
    cases = [('true', target + ',tracked:true'), ('false', target + ',tracked:false'),
             ('absent', target), ('targetless', 'tracked:true'), ('none', None)]
    for n, (case, value) in enumerate(cases, 1):
        x = n * 2
        uuid = f'00000149-0000-0000-0000-{n:012x}'
        lodestone = '' if value is None else f',"minecraft:lodestone_tracker":{{{value}}}'
        cmd(f'/execute if loaded {x} 201 4', case + '-loaded', 1)
        cmd(f'/execute if block {x} 199 4 stone', case + '-floor', 1)
        cmd(f'/execute if block {x} 201 4 air', case + '-gap', 1)
        cmd(f'/summon item {x + .5} 201 4.5 {{UUID:[I;329,0,0,{n}],NoGravity:1b,'
            f'Motion:[0d,0d,0d],PickupDelay:32767s,CustomName:"Tracker_{case}",'
            f'Item:{{id:"minecraft:compass",count:1,components:{{"minecraft:custom_data":{{keep:17}}{lodestone}}}}}}}',
            case + '-summon', 1)
        cmd(f'/execute as {uuid} if entity @s[type=minecraft:item]', case + '-actual-type', 1)
        for field, result in [('UUID', 4), ('Pos', 3), ('Motion', 3), ('NoGravity', 1),
                              ('Health', 5), ('Age', 0), ('PickupDelay', 32767),
                              ('Item.id', 17), ('Item.count', 1)]:
            cmd(f'/data get entity {uuid} {field}', case + '-' + field, result)
        cmd(f'/data get entity {uuid} Item.components."minecraft:custom_data".keep',
            case + '-custom-data', 17)
        cmd(f'/data get entity {uuid}', case + '-full-body', diagnostic=True)
        component = 'Item.components."minecraft:lodestone_tracker"'
        if case in ('true', 'false', 'absent'):
            cmd(f'/data get entity {uuid} {component}.target.dimension', case + '-dimension', 19)
            cmd(f'/data get entity {uuid} {component}.target.pos', case + '-target-pos', 3)
        for suffix, path, exists in [('component', component, case != 'none'),
                                     ('tracked', component + '.tracked', case == 'false')]:
            affected = case in ('true', 'targetless')
            for holder in ['result', 'success']:
                cmd(f'/scoreboard players set {holder} h329 -99', case + '-' + suffix + '-reset-' + holder, -99)
            cmd(f'/execute store result score result h329 store success score success h329 '
                f'run data get entity {uuid} {path}', case + '-' + suffix + '-read',
                callback=([True, 0] if case == 'false' else [False, 0]) if suffix == 'tracked' else None,
                admission=not affected)
            for holder in ['result', 'success']:
                cmd(f'/scoreboard players get {holder} h329', case + '-' + suffix + '-' + holder,
                    admission=not affected)
            cmd('/scoreboard players set presence h329 -99', case + '-' + suffix + '-reset-presence', -99)
            cmd(f'/execute if data entity {uuid} {path} run scoreboard players set presence h329 1',
                case + '-' + suffix + '-presence', admission=not affected or suffix == 'component')
            ops[-1]['expect_results'] = [[True, 1]] if exists else []
            cmd('/scoreboard players get presence h329', case + '-' + suffix + '-presence-result',
                1 if exists else -99, admission=not affected or suffix == 'component')
    for n, (case, _) in enumerate(cases, 1):
        uuid = f'00000149-0000-0000-0000-{n:012x}'
        for field, result in [('UUID', 4), ('Pos', 3), ('Motion', 3), ('Item.count', 1)]:
            cmd(f'/data get entity {uuid} {field}', case + '-after-' + field, result)
    cmd('/execute store result score now h329 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation now h329 -= start h329', 'clock-delta-operation', 0)
    cmd('/scoreboard players get now h329', 'clock-delta', 0)
    if armor:
        def equip(label, pos, tracked=None, dim='minecraft:overworld'):
            # The equippable component is what lets a non-armour item into an
            # armour slot; the compass itself still ticks its tracker. The
            # command's own feedback names the item, which is another suite's
            # subject, so only the reads below are compared.
            tracker = f'target:{{dimension:"{dim}",pos:[I;{pos},200,12]}}'
            if tracked is not None:
                tracker += f',tracked:{tracked}'
            item = (f'minecraft:compass[minecraft:lodestone_tracker={{{tracker}}},'
                    'minecraft:equippable={slot:"head"},minecraft:custom_data={armor:1}]')
            cmd(f'/item replace entity @s armor.head with {item} 1', label + '-equip',
                admission=False, record=False)

        def tick(label):
            ops.append({'op': 'player_loop', 'n': 1, 'label': label, 'record': True})

        def tracker(label):
            cmd('/data get entity @s equipment.head.components."minecraft:lodestone_tracker"', label)
            cmd('/execute if data entity @s equipment.head.components."minecraft:lodestone_tracker".target',
                label + '-target')
            cmd('/data get entity @s equipment.head.id', label + '-id', 17)

        cmd('/setblock 14 200 12 lodestone', 'armor-stone-setup', 1)
        equip('armor-air', 12)
        tracker('armor-air-before')
        cmd('/setblock 12 200 12 air', 'armor-air-remove', 1)
        tick('armor-air-tick')
        tracker('armor-air-after')
        cmd('/setblock 14 200 12 stone', 'armor-stone-replace', 1)
        equip('armor-stone', 14)
        tracker('armor-stone-before')
        tick('armor-stone-tick')
        tracker('armor-stone-after')
        cmd('/setblock 12 200 12 lodestone', 'armor-loaded-setup', 1)
        equip('armor-loaded', 12)
        tick('armor-loaded-tick')
        tracker('armor-loaded-after')
        equip('armor-untracked', 12, tracked='false')
        cmd('/setblock 12 200 12 air', 'armor-untracked-remove', 1)
        tick('armor-untracked-tick')
        tracker('armor-untracked-after')
        equip('armor-dimension', 14, dim='minecraft:the_nether')
        tick('armor-dimension-tick')
        tracker('armor-dimension-after')
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
        sys.argv += ['--work-dir', str(driver.lab.LOCAL / 'lodestone-default-parity')]
    armor = '--no-armor' not in sys.argv
    if not armor:
        sys.argv.remove('--no-armor')
    build = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    ops = program(full, armor)
    driver.corpus_module.groups = lambda: [('saved lodestone tracker default', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [op for op in ops if (op['op'] == 'cmd' and op.get('record', True))
                or (op['op'] in ('tick', 'player_loop') and op.get('record'))]
    java = [json.loads(s) for s in (build / 'vanilla.jsonl').read_text().splitlines()]
    for op, row in zip(recorded, java):
        if 'expect_results' in op:
            assert row['results'] == op['expect_results'], ('Java action admission', op, row)
    if '--reference-only' in sys.argv:
        return
    source = reference or build / 'vanilla.jsonl'
    if reference:
        assert source.read_bytes() == (build / 'vanilla.jsonl').read_bytes(), 'fresh Java bytes changed'
    expected = [json.loads(s) for s in source.read_text().splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_text().splitlines()]
    assert len(expected) == len(native) == len(recorded)
    differences = [{'label': op['label'], 'expected': a, 'actual': b}
                   for op, a, b in zip(recorded, expected, native) if a != b]
    report = {'records': len(native), 'passed': len(native) - len(differences),
              'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
              'comparison': 'complete raw records, no masks/tolerances', 'differences': differences}
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    if armor:
        slots = ('air', 'stone')
        matched = sum(1 for slot in slots
                      if not any(item['label'].startswith(f'armor-{slot}-') for item in differences))
        print(f'Armor tracking: {matched}/{len(slots)} cases match')
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if not full and not differences:
        for op, a, b in zip(recorded, expected, native):
            if op.get('admission'):
                assert a == b, ('actual fixture admission', op, a, b)
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
