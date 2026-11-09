#!/usr/bin/env python3
"""Pillager/trader Inventory through real summon/get commands."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

STACK = '{id:"minecraft:stone",count:3,components:{"minecraft:custom_data":{audit:137}}}'
OFFER = '{buy:{id:"minecraft:emerald",count:1},sell:{id:"minecraft:dirt",count:2}}'
CASES = [(kind, state) for kind in ['pillager', 'wandering_trader']
         for state in ['absent', 'empty', 'stack']]
CASES += [('pig', 'absent'), ('piglin', 'absent'), ('piglin', 'stack')]


def program():
    ops = []
    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if result is not None: op['expect_result'] = result
        if diagnostic: op.update(record=False, trace=True)
        ops.append(op)
    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 14.5 200 14.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    for condition, label in [('loaded 8 200 8', 'loaded'), ('block 8 199 8 stone', 'floor-state'), ('block 8 200 8 air', 'air'), ('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]', 'owner')]:
        cmd('/execute if ' + condition, label, 1)
    cmd('/scoreboard objectives add h137 dummy', 'objective')
    cmd('/execute store result score start h137 run time query gametime', 'clock-start', diagnostic=True)
    for index, (kind, state) in enumerate(CASES, 1):
        label = kind+'-'+state
        uid = f'00000089-0000-0000-0000-{index:012x}'
        x = 1.5 + 1.5 * (index - 1)
        hp = {'pillager':24, 'wandering_trader':20, 'pig':10, 'piglin':16}[kind]
        extra = '' if state == 'absent' else ',Inventory:' + ('[]' if state == 'empty' else '['+STACK+']')
        if kind == 'pillager': extra += ',PatrolLeader:1b,CanJoinRaid:0b'
        if kind == 'wandering_trader': extra += ',DespawnDelay:48000,Offers:{Recipes:['+OFFER+']}'
        if kind == 'piglin': extra += ',IsImmuneToZombification:1b'
        cmd(f'/summon {kind} {x} 200 4.5 {{UUID:[I;137,0,0,{index}],Health:{hp}f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],Tags:["h137_{index}"],CustomName:"h137_{index}"{extra}}}', label+'-summon', 1)
        cmd(f'/execute if entity @e[type={kind},nbt={{UUID:[I;137,0,0,{index}],Health:{hp}f,Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d],NoAI:1b,NoGravity:1b,Tags:["h137_{index}"],CustomName:"h137_{index}"}}]', label+'-actor', 1)
        cmd(f'/data get entity {uid} UUID', label+'-uuid', 4)
        cmd(f'/data get entity {uid}', label+'-body', diagnostic=True)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h137 -99', label+'-reset-'+holder, -99)
        cmd(f'/execute store result score result h137 store success score success h137 run data get entity {uid} Inventory', label+'-inventory-read', admission=False)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players get {holder} h137', label+'-inventory-'+holder, admission=False)
        if kind == 'pig':
            cmd(f'/execute unless data entity {uid} Inventory', label+'-inventory-absent', admission=False)
        else:
            cmd(f'/execute if data entity {uid} Inventory', label+'-inventory-present', admission=False)
            expected = '[]' if state != 'stack' else '['+STACK+']'
            cmd(f'/execute if data entity {uid} {{Inventory:{expected}}}', label+'-inventory-type', admission=False)
            if state == 'stack':
                cmd(f'/data get entity {uid} Inventory[0].id', label+'-item-id', admission=False)
                cmd(f'/data get entity {uid} Inventory[0].count', label+'-item-count', admission=False)
                cmd(f'/data get entity {uid} Inventory[0].components."minecraft:custom_data".audit', label+'-item-data', admission=False)
            else:
                cmd(f'/execute unless data entity {uid} Inventory[0]', label+'-no-item', admission=False)
        if kind == 'pillager':
            cmd(f'/execute if data entity {uid} {{PatrolLeader:1b,CanJoinRaid:0b}}', label+'-flags', 1)
        if kind == 'wandering_trader':
            cmd(f'/data get entity {uid} DespawnDelay', label+'-despawn', 48000)
            cmd(f'/execute if data entity {uid} {{DespawnDelay:48000,Offers:{{Recipes:[{OFFER}]}}}}', label+'-trader-types', 1)
            cmd(f'/data get entity {uid} Offers.Recipes[0].sell.count', label+'-offers', 2)
        if kind == 'piglin':
            cmd(f'/execute if data entity {uid} {{IsImmuneToZombification:1b}}', label+'-immune', 1)
    cmd('/execute store result score end h137 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h137 -= start h137', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h137', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='carrier-inventory-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=CASES, labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete records, no masks/tolerances; bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Carrier inventory', ops)]
    sys.argv.append('--allow-gaps'); driver.main()
    java_bytes = (build/'vanilla.jsonl').read_bytes()
    if expected_bytes is not None:
        assert java_bytes==expected_bytes and reference.read_bytes()==expected_bytes, 'fresh Java reference changed'
    java = [json.loads(s) for s in java_bytes.splitlines()]
    if '--reference-only' in sys.argv: return
    native = [json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded)==len(java)==len(native)
    residuals = [dict(label=op['label'], expected=a, actual=b) for op,a,b in zip(recorded,java,native) if a!=b]
    report = dict(records=len(recorded), passed=len(recorded)-len(residuals), residuals=residuals,
                  reference_sha256=hashlib.sha256(java_bytes).hexdigest())
    (build/'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
    for op,a,b in zip(recorded,java,native):
        if op['admission']: assert a==b, ('admission',op,a,b)
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals: raise SystemExit(1)


if __name__ == '__main__':
    main()
