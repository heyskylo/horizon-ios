#!/usr/bin/env python3
"""Empty command loot positions through real summon/get commands; complete declared records."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

def table(counts):
    pools=[]
    for count in counts:
        entry={'type':'minecraft:item','name':'minecraft:iron_ingot',
               'modifier':{'type':'minecraft:set_count','count':count}}
        pools.append({'rolls':1,'entries':[entry]})
    return json.dumps({'type':'minecraft:chest','pools':pools},separators=(',',':'))

TABLES=[('zero',table([0])),('zero_next',table([0,1])),
        ('none',table([])),('positive',table([1]))]
CASES=[(name+'_'+mode,mode,t) for name,t in TABLES for mode in ['implicit','explicit2','insert']]
CASES += [('empty_entry','implicit',json.dumps({'pools':[{'rolls':1,'entries':[{'type':'minecraft:empty'}]}]},separators=(',',':'))),
          ('named_positive','implicit','minecraft:blocks/diamond_block')]


def program():
    ops=[]
    def cmd(text,label,result=None,admission=True,diagnostic=False):
        op=dict(op='cmd',cmd=text,label=label,admission=admission)
        if result is not None:op['expect_result']=result
        if diagnostic:op.update(record=False,trace=True)
        ops.append(op)
    def stored(text,label,admission=False):
        for key in ['result','success']:
            cmd(f'/scoreboard players set {key} h039 -99',label+'-reset-'+key,-99)
        cmd('/execute store result score result h039 store success score success h039 run '+text,label,admission=admission)
        for key in ['result','success']:
            cmd(f'/scoreboard players get {key} h039',label+'-'+key,admission=admission)
    cmd('/tick freeze','freeze')
    cmd('/gamerule spawn_mobs false','no-spawn')
    cmd('/gamerule advance_time false','no-daylight')
    cmd('/forceload add 0 0','force',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone','floor',256)
    cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1)
    cmd('/gamemode creative @s','owner-mode')
    cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
    cmd('/data get entity @s UUID','owner-uuid',4)
    cmd('/scoreboard objectives add h039 dummy','objective')
    cmd('/execute store result score start h039 run time query gametime','clock-start',diagnostic=True)
    for i,(name,mode,source) in enumerate(CASES):
        x=1+3*(i%4);z=1+3*(i//4);at=f'{x} 200 {z}'
        cmd('/setblock '+at+' chest[facing=north,type=single,waterlogged=false]',name+'-place',1)
        cmd('/execute if loaded '+at,name+'-loaded',1)
        cmd(f'/execute if block {x} 199 {z} stone',name+'-floor',1)
        cmd('/execute if block '+at+' chest[facing=north,type=single,waterlogged=false]',name+'-block',1)
        for key,result in [('id',15),('x',x),('y',200),('z',z),('Items',0)]:
            cmd('/data get block '+at+' '+key,name+'-be-'+key,result)
        for slot,item,count in [(0,'diamond',3),(1,'emerald',4),(2,'gold_ingot',5)]:
            cmd(f'/item replace block {at} container.{slot} with minecraft:{item} {count}',name+f'-initial-slot{slot}',1)
        cmd('/data get block '+at+' Items',name+'-initial-inventory',3)
        cmd('/execute if data block '+at+' {Items:[{Slot:0b,id:"minecraft:diamond",count:3},{Slot:1b,id:"minecraft:emerald",count:4},{Slot:2b,id:"minecraft:gold_ingot",count:5}]}',name+'-initial-types',1)
        dest='insert '+at if mode=='insert' else 'replace block '+at+' container.0'+(' 2' if mode=='explicit2' else '')
        control=name.startswith('positive') or name in ['empty_entry','named_positive']
        stored('loot '+dest+' loot '+source,name+'-loot',control)
        stored('data get block '+at+' Items',name+'-post-inventory',control)
        cmd('/execute if data block '+at+' {Items:[{Slot:2b,id:"minecraft:gold_ingot",count:5}]}',name+'-unaffected-slot',1)
        cmd('/data get block '+at,name+'-full-body',diagnostic=True)
    cmd('/execute store result score end h039 run time query gametime','clock-end',diagnostic=True)
    cmd('/scoreboard players operation end h039 -= start h039','clock-delta-operation',0)
    cmd('/scoreboard players get end h039','clock-delta',0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='empty-loot-positions-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Empty command loot positions', ops)]
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
