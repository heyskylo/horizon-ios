#!/usr/bin/env python3
"""Interaction saved fields through actual Game commands.

Complete declared records are strict; full bodies and absolute clocks remain
predeclared raw traces. Three Interaction cases, another Other kind and a modeled pig control.
"""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [('absent',None),('defaults',('1f','1f','0b')),('nondefault',('2.5f','3.5f','1b'))]
FIELDS = ['width','height','response']


def program():
    ops=[]
    def cmd(text,label,result=None,admission=True,diagnostic=False):
        op=dict(op='cmd',cmd=text,label=label,admission=admission)
        if result is not None: op['expect_result']=result
        if diagnostic: op.update(record=False,trace=True)
        ops.append(op)
    def stored(command,label,admission=False):
        for holder in ['result','success']:
            cmd(f'/scoreboard players set {holder} h109 -99',label+'-reset-'+holder,-99)
        cmd('/execute store result score result h109 store success score success h109 run '+command,label+'-read',admission=admission)
        for holder in ['result','success']:
            cmd(f'/scoreboard players get {holder} h109',label+'-'+holder,admission=admission)
    cmd('/tick freeze','freeze')
    cmd('/gamerule spawn_mobs false','no-spawn')
    cmd('/gamerule advance_time false','no-daylight')
    cmd('/forceload add 0 0','force',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone','floor',256)
    cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1)
    cmd('/gamemode creative @s','owner-mode')
    for condition,label in [('loaded 8 200 8','loaded'),('block 8 199 8 stone','floor-state'),('block 8 200 8 air','air'),('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]','owner')]:
        cmd('/execute if '+condition,label,1)
    cmd('/scoreboard objectives add h109 dummy','objective')
    cmd('/execute store result score start h109 run time query gametime','clock-start',diagnostic=True)
    for index,(label,values) in enumerate(CASES,1):
        x=1.5+3*(index-1);uid=f'0000006d-0000-0000-0000-{index:012x}'
        fields='' if values is None else ','+','.join(f'{k}:{v}' for k,v in zip(FIELDS,values))
        # Historical UUID payloads are not used as live actor witnesses.
        records=',attack:{player:[I;109,1,0,91],timestamp:17L},interaction:{player:[I;109,1,0,92],timestamp:23L}' if label=='nondefault' else ''
        cmd(f'/summon interaction {x} 200 4.5 {{UUID:[I;109,0,0,{index}],Rotation:[15f,-30f],Motion:[0d,0d,0d],NoGravity:1b,Silent:1b,Tags:["h109_{index}"],CustomName:"h109_{index}"{fields}{records}}}',label+'-summon',1)
        cmd(f'/execute if entity @e[type=interaction,nbt={{UUID:[I;109,0,0,{index}],Pos:[{x}d,200d,4.5d],Rotation:[15f,-30f],Motion:[0d,0d,0d],NoGravity:1b,Silent:1b,Tags:["h109_{index}"],CustomName:"h109_{index}"}}]',label+'-actor',1)
        cmd(f'/data get entity {uid} UUID',label+'-uuid',4)
        cmd(f'/data get entity {uid}',label+'-body',diagnostic=True)
        for key,value in zip(FIELDS,values or ('1f','1f','0b')):
            stored(f'data get entity {uid} {key}',label+'-'+key)
            cmd(f'/execute if data entity {uid} {key}',label+'-'+key+'-presence',admission=False)
            cmd(f'/execute if data entity {uid} {{{key}:{value}}}',label+'-'+key+'-type',admission=False)
        for key in ['attack','interaction']:
            if label=='nondefault':
                n=91 if key=='attack' else 92;time=17 if key=='attack' else 23
                cmd(f'/data get entity {uid} {key}.player',label+'-'+key+'-uuid',4)
                cmd(f'/data get entity {uid} {key}.timestamp',label+'-'+key+'-time',time)
                cmd(f'/execute if data entity {uid} {{{key}:{{player:[I;109,1,0,{n}],timestamp:{time}L}}}}',label+'-'+key+'-types',1)
            else:
                cmd(f'/execute unless data entity {uid} {key}',label+'-no-'+key,1)
    for kind,index in [('marker',4),('pig',5)]:
        uid=f'0000006d-0000-0000-0000-{index:012x}'
        extra=',NoAI:1b,PersistenceRequired:1b,Health:10f' if kind=='pig' else ',data:{audit:7}'
        cmd(f'/summon {kind} 10.5 200 {index+3}.5 {{UUID:[I;109,0,0,{index}],Rotation:[0f,0f],NoGravity:1b,Tags:["h109_control"]{extra}}}',kind+'-summon',1)
        cmd(f'/execute if entity @e[type={kind},nbt={{UUID:[I;109,0,0,{index}],Pos:[10.5d,200d,{index+3}.5d],Rotation:[0f,0f],NoGravity:1b,Tags:["h109_control"]}}]',kind+'-actor',1)
        cmd(f'/data get entity {uid} UUID',kind+'-uuid',4)
        cmd(f'/data get entity {uid}',kind+'-body',diagnostic=True)
        if kind=='pig':
            cmd(f'/execute if data entity {uid} {{Health:10f,NoAI:1b,PersistenceRequired:1b}}',kind+'-live-state',1)
        else: cmd(f'/data get entity {uid} data.audit',kind+'-data',7)
        for key in FIELDS:
            stored(f'data get entity {uid} {key}',kind+'-'+key,True)
            cmd(f'/execute unless data entity {uid} {key}',kind+'-'+key+'-absent',1)
    cmd('/execute store result score end h109 run time query gametime','clock-end',diagnostic=True)
    cmd('/scoreboard players operation end h109 -= start h109','clock-delta-operation',0)
    cmd('/scoreboard players get end h109','clock-delta',0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='interaction-saved-defaults-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=CASES, fields=FIELDS, labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete declared records, no masks/tolerances; whole bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Interaction saved defaults', ops)]
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
