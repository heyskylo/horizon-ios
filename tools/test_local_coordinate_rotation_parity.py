"""Exact local-coordinate rotations through actual UUID-bound pig commands.
All Pos and Rotation observations, callbacks/styles and typed guards are strict.
Only full entity bodies and absolute epochs are predeclared raw diagnostics.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

CASES = [
    {'name': 'south', 'route': 'direct', 'rotation': [0.0, 0.0], 'local': [0.75, 0.5, 1.25],
     'bits': ['4022800000000000', '4069580000000000', '4023800000000000'], 'admission': False},
    {'name': 'west', 'route': 'direct', 'rotation': [90.0, 0.0], 'local': [0.75, 0.5, 1.25],
     'bits': ['401d000000000000', '4069580000000000', '4022800000000000'], 'admission': False},
    {'name': 'north', 'route': 'direct', 'rotation': [180.0, 0.0], 'local': [0.75, 0.5, 1.25],
     'bits': ['401f000000000000', '4069580000000000', '401d000000000000'], 'admission': False},
    {'name': 'east', 'route': 'direct', 'rotation': [270.0, 0.0], 'local': [0.75, 0.5, 1.25],
     'bits': ['4023800000000000', '4069580000000000', '401f000000000000'], 'admission': False},
    {'name': 'oblique', 'route': 'direct', 'rotation': [37.5, -22.25], 'local': [0.1, 0.1, 0.1],
     'bits': ['402117934a6314b9', '40694c2c5ee98f58', '40213562572e889a'], 'admission': False},
    {'name': 'positioned_oblique', 'route': 'positioned', 'rotation': [123.75, 47.125], 'local': [0.1, 0.1, 0.1],
     'bits': ['4020a7648c7aa86b', '406947d514129374', '4021025e8b180036'], 'admission': False},
    {'name': 'world', 'route': 'world', 'rotation': [37.5, -22.25], 'local': [0.75, 0.5, 1.25],
     'bits': ['4022800000000000', '4069580000000000', '4023800000000000'], 'admission': True},
    {'name': 'relative_eyes', 'route': 'relative', 'rotation': [37.5, -22.25], 'local': [0.75, 0.5, 1.25],
     'bits': ['4022800000000000', '4069580000000000', '4023800000000000'], 'admission': True},
    {'name': 'feet', 'route': 'feet', 'rotation': [37.5, -22.25], 'local': [0.0, 0.0, 0.0],
     'bits': ['4012000000000000', '4069000000000000', '4012000000000000'], 'admission': True},
    {'name': 'eyes', 'route': 'eyes', 'rotation': [37.5, -22.25], 'local': [0.0, 0.0, 0.0],
     'bits': ['4012000000000000', '4069187ae1400000', '4012000000000000'], 'admission': True},
]

def values(case):
    return [struct.unpack('>d',bytes.fromhex(h))[0] for h in case['bits']]

def action(case, actor):
    yaw,pitch=case['rotation'];left,up,forward=case['local']
    frame=f'execute positioned 8.5 202.25 8.5 rotated {yaw} {pitch}'
    local=f'^{left} ^{up} ^{forward}'
    if case['route']=='direct':return frame+f' run tp {actor} '+local
    if case['route']=='positioned':return frame+' positioned '+local+f' run tp {actor} ~ ~ ~'
    if case['route']=='world':return frame+f' run tp {actor} 9.25 202.75 9.75'
    if case['route']=='relative':return frame+f' anchored eyes run tp {actor} ~0.75 ~0.5 ~1.25'
    return f'execute as {actor} at @s anchored '+case['route']+f' rotated {yaw} {pitch} run tp @s ^ ^ ^'

def program():
    ops=[]
    def cmd(text,label,result=None,admission=True,diagnostic=None,**extra):
        row=dict(op='cmd',cmd='/'+text.lstrip('/'),label=label,admission=admission)
        if result is not None:row['expect_result']=result
        if diagnostic:row['diagnostic']=diagnostic
        row.update(extra);ops.append(row)
    cmd('tick freeze','freeze',1)
    cmd('gamemode spectator','spectator')
    cmd('gamerule spawn_mobs false','no-spawns')
    cmd('forceload add 0 0','load',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('fill 0 199 0 15 199 15 stone','floor',256)
    cmd('tp @s 8.5 205 8.5 0 0','owner-position',1)
    cmd('execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,205d,8.5d],playerGameType:3}]','owner-state',1)
    cmd('data get entity @s UUID','owner-uuid',4)
    cmd('scoreboard objectives add h149 dummy','objective',1)
    cmd('execute store result score #start h149 run time query gametime','epoch-start',diagnostic='absolute epoch')
    for i,case in enumerate(CASES,1):
        name=case['name'];actor=f'00000095-0000-0001-0000-{i:012x}'
        nbt=f'{{UUID:[I;149,1,0,{i}],Tags:["h149"],Health:10f,NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Motion:[0d,0d,0d],Rotation:[11f,7f]}}'
        cmd('summon pig 4.5 200 4.5 '+nbt,name+'-summon',1)
        guard=f'@e[type=pig,nbt={{UUID:[I;149,1,0,{i}],Health:10f,NoAI:1b,NoGravity:1b,Pos:[4.5d,200d,4.5d],Motion:[0d,0d,0d],Rotation:[11f,7f]}}]'
        cmd('execute if entity '+guard,name+'-identity-type-state',1)
        cmd('data get entity '+actor+' UUID',name+'-uuid',4)
        cmd('data get entity '+actor+' Pos',name+'-initial-pos',3)
        cmd('data get entity '+actor+' Rotation',name+'-initial-rotation',2)
        cmd('execute if loaded 4 200 4',name+'-loaded',1)
        cmd('execute if block 4 199 4 stone',name+'-floor',1)
        cmd('data get entity '+actor,name+'-full-before',diagnostic='full actor body/order/unrelated saved state')
        cmd('scoreboard players set #r h149 91',name+'-action-reset-result',91)
        cmd('scoreboard players set #s h149 17',name+'-action-reset-success',17)
        cmd('execute store result score #r h149 store success score #s h149 run '+action(case,actor),name+'-action',1,admission=case['admission'])
        cmd('scoreboard players get #r h149',name+'-action-result',1,admission=case['admission'])
        cmd('scoreboard players get #s h149',name+'-action-success',1,admission=case['admission'])
        cmd('scoreboard players set #r h149 31',name+'-pos-reset-result',31)
        cmd('scoreboard players set #s h149 19',name+'-pos-reset-success',19)
        cmd('execute store result score #r h149 store success score #s h149 run data get entity '+actor+' Pos',name+'-actual-pos',3,admission=case['admission'])
        cmd('scoreboard players get #r h149',name+'-pos-result',3,admission=case['admission'])
        cmd('scoreboard players get #s h149',name+'-pos-success',1,admission=case['admission'])
        pos='['+','.join(repr(v)+'d' for v in values(case))+']'
        cmd('execute if entity '+f'@e[type=pig,nbt={{UUID:[I;149,1,0,{i}],Pos:{pos}}}]',name+'-exact-typed-double-pos',1,admission=case['admission'])
        cmd('data get entity '+actor+' Rotation',name+'-actual-rotation',2)
        cmd('execute if entity '+f'@e[type=pig,nbt={{UUID:[I;149,1,0,{i}],Health:10f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],Rotation:[11f,7f]}}]',name+'-unchanged-state',1)
        cmd('execute as '+actor+' at @s if block ~ 199 ~ stone',name+'-post-floor',1)
        cmd('execute as '+actor+' at @s if loaded ~ ~ ~',name+'-post-loaded',1)
        cmd('data get entity '+actor,name+'-full-after',diagnostic='full actor body/order/unrelated saved state')
    cmd('execute store result score #now h149 run time query gametime','epoch-end',diagnostic='absolute epoch')
    cmd('scoreboard players operation #now h149 -= #start h149','relative-clock-op',0)
    cmd('scoreboard players get #now h149','relative-clock',0)
    cmd('execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,205d,8.5d],playerGameType:3}]','final-owner-state',1)
    cmd('kill @e[tag=h149]','cleanup-actors',len(CASES))
    cmd('forceload remove 0 0','unload',1)
    cmd('scoreboard objectives remove h149','cleanup-objective',0)
    cmd('tick unfreeze','cleanup-unfreeze',0)
    return ops

def main():
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir',type=Path)
    options,remaining=parser.parse_known_args()
    sys.argv=[sys.argv[0]]+remaining
    if '--help' in sys.argv or '-h' in sys.argv:
        driver.main()
        return 0
    if '--work-dir' not in sys.argv:parser.error('--work-dir must name a fresh capture directory')
    folder=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not folder.exists(),'preserve previous attempts'
    driver.corpus_module.groups=lambda:[('Exact local-coordinate rotation',program())]
    if '--allow-gaps' not in sys.argv:sys.argv.append('--allow-gaps')
    driver.main()
    rows=lambda f:[json.loads(line) for line in f.read_text().splitlines()]
    ops=[o for o in program() if o['op']=='cmd' and o.get('record',True)]
    fresh=rows(folder/'vanilla.jsonl');assert len(fresh)==len(ops)
    if '--reference-only' in sys.argv:return 0
    reference=options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java,native=rows(reference),rows(folder/'rust.jsonl')
    assert len(java)==len(native)==len(ops)
    assert all(a==b for op,a,b in zip(ops,java,fresh) if not op.get('diagnostic')),'fresh exact Java controls changed'
    report=dict(strict=0,admissions=0,strict_failures=[],diagnostic_records=0,diagnostic_differences=[],reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diag=bool(op.get('diagnostic'));report['diagnostic_records' if diag else 'strict']+=1
        if a!=b:report['diagnostic_differences' if diag else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diag:
            report['admissions']+=1
            assert a==b,('positive independent admission/control',op,a,b)
    (folder/'local-coordinate-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])

if __name__=='__main__':sys.exit(main())
