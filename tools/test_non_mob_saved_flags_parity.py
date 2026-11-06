#!/usr/bin/env python3
"""Mob-only saved fields through actual Game commands.

Complete declared records are strict; full bodies and absolute clocks remain
predeclared raw traces. Modeled non-Mob routes and a supported pig control.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

CLASSES = ['oak_boat', 'oak_chest_boat', 'armor_stand', 'item', 'pig']
VALUES = [('absent', None), ('zero', '0b'), ('one', '1b')]


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
    for condition, label in [('loaded 8 200 8', 'loaded'), ('block 8 199 8 stone', 'floor-state'),
                             ('block 8 200 8 air', 'air'), ('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]', 'owner')]:
        cmd('/execute if '+condition, label, 1)
    cmd('/scoreboard objectives add h104 dummy', 'objective')
    cmd('/execute store result score start h104 run time query gametime', 'clock-start', diagnostic=True)
    index = 0
    for kind in CLASSES:
        for name, value in VALUES:
            index += 1; label = kind+'-'+name
            x,z = 1.5+4*((index-1)%3), 1.5+3*((index-1)//3)
            uid = f'00000068-0000-0000-0000-{index:012x}'
            specific = ',Item:{id:"minecraft:stone",count:1}' if kind=='item' else ',Health:10f' if kind=='pig' else ',Health:20f' if kind=='armor_stand' else ''
            supplied = '' if value is None else f',NoAI:{value},PersistenceRequired:{value}'
            cmd(f'/summon {kind} {x} 200 {z} {{UUID:[I;104,0,0,{index}],Tags:["h104_{index}"],Motion:[0d,0d,0d],Silent:1b,NoGravity:1b,Invulnerable:1b,PortalCooldown:17{specific}{supplied}}}', label+'-summon', 1)
            body = f'UUID:[I;104,0,0,{index}],Pos:[{x}d,200d,{z}d],Motion:[0d,0d,0d],Tags:["h104_{index}"]'
            cmd(f'/execute if entity @e[type={kind},tag=h104_{index},nbt={{{body}}}]', label+'-actor', 1)
            if kind in ['pig','armor_stand']:
                cmd(f'/data get entity {uid} Health', label+'-health', 10 if kind=='pig' else 20)
            if kind=='item':
                cmd(f'/data get entity {uid} Item', label+'-valid-item', 2)
                cmd(f'/execute if data entity {uid} {{Item:{{id:"minecraft:stone",count:1}}}}', label+'-stone', 1)
            cmd(f'/data get entity {uid}', label+'-body', diagnostic=True)
            for key in ['Silent','NoGravity','Invulnerable']:
                cmd(f'/execute if data entity {uid} {{{key}:1b}}', label+'-keep-'+key, 1)
            cmd(f'/data get entity {uid} PortalCooldown', label+'-keep-portal', 17)
            for key in ['NoAI','PersistenceRequired']:
                for holder in ['result','success']:
                    cmd(f'/scoreboard players set {holder} h104 -99', label+'-'+key+'-reset-'+holder, -99)
                cmd(f'/execute store result score result h104 store success score success h104 run data get entity {uid} {key}', label+'-'+key+'-read', admission=False)
                for holder in ['result','success']:
                    cmd(f'/scoreboard players get {holder} h104', label+'-'+key+'-'+holder, admission=False)
                cmd(f'/execute if data entity {uid} {key}', label+'-'+key+'-presence', admission=False)
                if kind=='pig' and (key=='PersistenceRequired' or value=='1b'):
                    cmd(f'/execute if data entity {uid} {{{key}:{1 if value=="1b" else 0}b}}', label+'-'+key+'-byte', admission=False)
            cmd(f'/execute if entity @e[type={kind},tag=h104_{index},nbt={{{body}}}]', label+'-actor-after', 1)
    cmd('/execute store result score end h104 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h104 -= start h104', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h104', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        raise SystemExit('a fresh --work-dir is required')
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        classes=CLASSES, values=VALUES, labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete declared records, no masks/tolerances; whole bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('non-Mob saved Mob-only fields', ops)]
    sys.argv.append('--allow-gaps'); driver.main()
    java_bytes = (build/'vanilla.jsonl').read_bytes()
    if expected_bytes is not None:
        assert java_bytes==expected_bytes and reference.read_bytes()==expected_bytes, 'fresh Java reference changed'
    java = [json.loads(s) for s in java_bytes.splitlines()]
    if '--reference-only' in sys.argv:
        return
    native = [json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded)==len(java)==len(native)
    residuals = [dict(label=op['label'],expected=a,actual=b) for op,a,b in zip(recorded,java,native) if a!=b]
    report = dict(records=len(recorded),passed=len(recorded)-len(residuals),residuals=residuals,
                  reference_sha256=hashlib.sha256(java_bytes).hexdigest())
    (build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n')
    for op,a,b in zip(recorded,java,native):
        if op['admission']:
            assert a==b, ('admission',op,a,b)
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals:
        raise SystemExit(1)


if __name__=='__main__':
    main()
