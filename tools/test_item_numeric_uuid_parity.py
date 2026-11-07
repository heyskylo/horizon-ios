#!/usr/bin/env python3
"""Item numeric UUID codec through real summon/get commands; complete declared records."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [
    ('absent', None), ('int', '[I;54,1,-2,3]'),
    ('long_array', '[L;54L,4294967297L,-4294967298L,9007199254740993L]'),
    ('float_list', '[54.9f,1.75f,-2.75f,-0.5f]'),
    ('double_list', '[54.9d,1.75d,-2.75d,-0.5d]'),
    ('long_list', '[54L,4294967297L,-4294967298L,9007199254740993L]'),
    ('byte_array', '[B;54b,1b,-2b,3b]'),
    ('string', '"00000036-0000-0001-ffff-fffe00000003"'),
    ('short', '[I;54,1,-2]'), ('long', '[I;54,1,-2,3,4]'),
    ('scalar', '7'), ('compound', '{value:7}'),
    ('owner_only', '[I;54,1,-2,3]'), ('thrower_only', '[I;54,1,-2,3]'),
]


def program():
    ops=[]
    def cmd(text,label,result=None,admission=True,diagnostic=False):
        op=dict(op='cmd',cmd=text,label=label,admission=admission)
        if result is not None:op['expect_result']=result
        if diagnostic:op.update(record=False,trace=True)
        ops.append(op)
    cmd('/tick freeze','freeze')
    cmd('/gamerule spawn_mobs false','no-spawn')
    cmd('/gamerule advance_time false','no-daylight')
    cmd('/forceload add 0 0','force',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone','floor',256)
    cmd('/tp @s 14.5 200 14.5 0 0','player-position',1)
    cmd('/gamemode creative @s','player-mode')
    cmd('/execute if loaded 4 200 4','loaded',1)
    cmd('/execute if block 4 199 4 stone','support',1)
    cmd('/execute if block 4 200 4 air','air',1)
    cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','player',1)
    cmd('/data get entity @s UUID','player-uuid')
    cmd('/scoreboard objectives add h054 dummy','objective')
    cmd('/execute store result score start h054 run time query gametime','clock-start',diagnostic=True)
    for index,(label,value) in enumerate(CASES,1):
        x=2.5+2*((index-1)%5);z=2.5+2*((index-1)//5)
        uid=f'00000036-0000-0000-0000-{index:012x}'
        extra=''
        if value is not None:
            if label!='thrower_only':extra+=',Owner:'+value
            if label!='owner_only':extra+=',Thrower:'+value
        cmd(f'/summon item {x} 200 {z} {{UUID:[I;54,0,0,{index}],Health:5s,NoGravity:1b,Motion:[0d,0d,0d],Item:{{id:"minecraft:stone",count:1,components:{{"minecraft:custom_data":{{h054:1}}}}}},Tags:["h054_{index}"],CustomName:"h054_{index}"'+extra+'}',label+'-summon',1)
        cmd(f'/execute if entity @e[type=item,nbt={{UUID:[I;54,0,0,{index}],Health:5s,Pos:[{x}d,200d,{z}d],Motion:[0d,0d,0d],NoGravity:1b,Tags:["h054_{index}"],CustomName:"h054_{index}",Item:{{id:"minecraft:stone",count:1,components:{{"minecraft:custom_data":{{h054:1}}}}}}}}]',label+'-actor',1)
        cmd(f'/data get entity {uid} UUID',label+'-uuid',4)
        cmd(f'/data get entity {uid} Item',label+'-item')
        cmd(f'/data get entity {uid}',label+'-body',diagnostic=True)
        for field in ['Owner','Thrower']:
            key=label+'-'+field
            for holder in ['result','success']:
                cmd(f'/scoreboard players set {holder} h054 -99',key+'-reset-'+holder,-99)
            cmd(f'/execute store result score result h054 store success score success h054 run data get entity {uid} {field}',key+'-read',admission=False)
            for holder in ['result','success']:
                cmd(f'/scoreboard players get {holder} h054',key+'-'+holder,admission=False)
            cmd(f'/execute if data entity {uid} {field}',key+'-presence',admission=False)
    cmd('/execute store result score end h054 run time query gametime','clock-end',diagnostic=True)
    cmd('/scoreboard players operation end h054 -= start h054','clock-delta-operation',0)
    cmd('/scoreboard players get end h054','clock-delta',0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='item-numeric-uuid-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Item numeric UUID codec', ops)]
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
