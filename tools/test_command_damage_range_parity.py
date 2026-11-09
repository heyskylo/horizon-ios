"""Actual chest damage-only command predicates; full raw diagnostics stay separate."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver
STACKS = [
    ('zero', 'diamond_sword', None), ('below', 'diamond_sword', 3),
    ('edge', 'diamond_sword', 4), ('above', 'diamond_sword', 5),
    ('absent', 'stone', None), ('removed', 'diamond_sword', 'removed'),
]

def program():
    ops=[]
    def cmd(text,label,result=None,callback=None,admission=True,diagnostic=None,snapshot=None):
        row=dict(op='cmd',cmd=text,label=label,admission=admission)
        if result is not None:row['expect_result']=result
        if callback is not None:row['expect_callback']=callback
        if diagnostic:row['diagnostic']=diagnostic
        if snapshot:row['snapshot']=snapshot
        ops.append(row)
    cmd('/tick freeze','freeze',1)
    cmd('/gamemode spectator','spectator')
    cmd('/gamerule spawn_mobs false','no-spawns')
    cmd('/scoreboard objectives add h195 dummy','objective',1)
    cmd('/forceload add 0 0','forceload',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone','floor',256)
    cmd('/tp @s 8.5 202 8.5 0 0','position',1)
    cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,202d,8.5d],playerGameType:3}]','owner-state',1)
    cmd('/data get entity @s UUID','owner-uuid',4)
    cmd('/execute store result score #start h195 run time query gametime','epoch-start',diagnostic='absolute epoch')
    entries=[]
    for slot,(name,item,damage) in enumerate(STACKS):
        parts=['"minecraft:custom_data":{marker:195}']
        if isinstance(damage,int):parts.append('"minecraft:damage":'+str(damage))
        if damage=='removed':parts.append('"!minecraft:damage":{}')
        entries.append('{Slot:%db,id:"minecraft:%s",count:1,components:{%s}}'%(slot,item,','.join(parts)))
    cmd('/setblock 4 200 4 chest{Items:['+','.join(entries)+']}','loaded-items',1)
    cmd('/execute if loaded 4 200 4','loaded',1)
    cmd('/execute if block 4 199 4 stone','support',1)
    cmd('/execute if block 4 200 4 chest','block',1)
    cmd('/execute if data block 4 200 4 {id:"minecraft:chest",x:4,y:200,z:4}','identity',1)
    for slot,(name,item,damage) in enumerate(STACKS):
        path=f'Items[{{Slot:{slot}b}}]'
        cmd(f'/execute if data block 4 200 4 Items[{{Slot:{slot}b,id:"minecraft:{item}",count:1}}]',name+'-saved-id-count-type',1)
        cmd('/data get block 4 200 4 '+path+'.count',name+'-count',1)
        cmd('/data get block 4 200 4 '+path+'.components."minecraft:custom_data".marker',name+'-unrelated',195)
        if isinstance(damage,int):
            cmd(f'/execute if data block 4 200 4 Items[{{Slot:{slot}b,components:{{"minecraft:damage":{damage}}}}}]',name+'-typed-damage',1)
            cmd('/data get block 4 200 4 '+path+'.components."minecraft:damage"',name+'-saved-damage',damage)
        else:
            cmd('/execute unless data block 4 200 4 '+path+'.components."minecraft:damage"',name+'-no-saved-damage',1)
        if damage=='removed':
            cmd('/execute if data block 4 200 4 '+path+'.components."!minecraft:damage"',name+'-saved-removal',1)
        cmd(f'/execute if items block 4 200 4 container.{slot} {item}',name+'-actual-slot',1)
        cmd('/data get block 4 200 4 '+path,name+'-full-stack-before',diagnostic='full saved stack/order',snapshot=name+'-before')
        effective='unless' if name in ('absent','removed') else 'if'
        cmd(f'/execute {effective} items block 4 200 4 container.{slot} {item}[minecraft:damage]',name+'-effective-presence',1,diagnostic='existing generic component-presence route/default projection')
    cmd('/execute unless data block 4 200 4 Items[{Slot:6b}]','empty-slot',1)
    def probe(slot,term,want,label,item='diamond_sword'):
        pred=item+'['+term+']'
        cmd('/scoreboard players set #r h195 91',label+'-reset-result',91)
        cmd('/scoreboard players set #s h195 17',label+'-reset-success',17)
        cmd(f'/execute store result score #r h195 store success score #s h195 if items block 4 200 4 container.{slot} {pred}',label+'-if',1 if want else None,callback=None if want else [False,0],admission=False)
        cmd('/scoreboard players get #r h195',label+'-result',int(want),admission=False)
        cmd('/scoreboard players get #s h195',label+'-success',int(want),admission=False)
        cmd('/scoreboard players set #branch h195 23',label+'-reset-branch',23)
        cmd(f'/execute unless items block 4 200 4 container.{slot} {pred} run scoreboard players set #branch h195 7',label+'-unless',None if want else 7,admission=False)
        if want:ops[-1]['expect_no_callbacks']=True
        cmd('/scoreboard players get #branch h195',label+'-branch',23 if want else 7,admission=False)
    for slot,damage in [(1,3),(2,4),(3,5)]:
        for key,bounds,want in [('exact','4',damage==4),('min','{min:4}',damage>=4),('max','{max:4}',damage<=4),('closed','{min:3,max:4}',3<=damage<=4)]:
            probe(slot,'minecraft:damage~{damage:'+bounds+'}',want,f'{slot}-{key}')
    probe(0,'damage~{damage:0}',True,'plain-zero')
    probe(0,'damage~{damage:{min:1}}',False,'plain-nonzero')
    probe(4,'damage~{damage:0}',False,'stone-absent',item='stone')
    probe(5,'damage~{damage:0}',False,'removed-absent')
    probe(2,'!damage~{damage:4}',False,'negated-edge')
    probe(1,'!damage~{damage:4}',True,'negated-below')
    probe(4,'damage~{damage:4}',False,'wrong-item')
    probe(6,'damage~{damage:4}',False,'empty')
    probe(1,'damage=3',True,'equality-control')
    probe(1,'damage=4',False,'equality-negative')
    probe(1,'count~1',True,'count-control')
    probe(1,'custom_data~{marker:195}',True,'unrelated-control')
    for slot,(name,_,_) in enumerate(STACKS):
        cmd('/data get block 4 200 4 '+f'Items[{{Slot:{slot}b}}]',name+'-full-stack-after',diagnostic='full saved stack/order',snapshot=name+'-after')
    cmd('/data get block 4 200 4','full-block',diagnostic='full saved block/order')
    cmd('/execute store result score #now h195 run time query gametime','epoch-end',diagnostic='absolute epoch')
    cmd('/scoreboard players operation #now h195 -= #start h195','relative-clock-op',0)
    cmd('/scoreboard players get #now h195','relative-clock',0)
    cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,202d,8.5d],playerGameType:3}]','final-owner-state',1)
    cmd('/setblock 4 200 4 air','cleanup-block',1)
    cmd('/forceload remove 0 0','unload',1)
    cmd('/scoreboard objectives remove h195','cleanup-objective',0)
    cmd('/tick unfreeze','cleanup-unfreeze',0)
    return ops

def main():
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir',type=Path)
    options,remaining=parser.parse_known_args()
    sys.argv=[sys.argv[0]]+remaining
    if '--help' in sys.argv or '-h' in sys.argv:
        driver.main()
        return 0
    if '--work-dir' not in sys.argv:
        parser.error('--work-dir must name a fresh capture directory')
    folder=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not folder.exists(),'preserve earlier attempts'
    driver.corpus_module.groups=lambda:[('Command damage range',program())]
    if '--allow-gaps' not in sys.argv:sys.argv.append('--allow-gaps')
    driver.main()
    rows=lambda f:[json.loads(line) for line in f.read_text().splitlines()]
    ops=[o for o in program() if o['op']=='cmd' and o.get('record',True)]
    fresh=rows(folder/'vanilla.jsonl')
    assert len(fresh)==len(ops)
    for op,row in zip(ops,fresh):
        if op.get('expect_no_callbacks'):assert row.get('results')==[],('blocked branch callback',op,row)
    def changed(stacks):
        by_label={op['snapshot']:row for op,row in zip(ops,stacks) if op.get('snapshot')}
        return [name for name,_,_ in STACKS if by_label[name+'-before']!=by_label[name+'-after']]
    assert not changed(fresh),'Java queries changed actual saved stacks'
    if '--reference-only' in sys.argv:return 0
    reference=options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java,native=rows(reference),rows(folder/'rust.jsonl')
    assert len(java)==len(native)==len(ops)
    assert all(a==b for op,a,b in zip(ops,java,fresh) if not op.get('diagnostic')),'fresh strict Java reference changed'
    report=dict(strict=0,admissions=0,strict_failures=[],diagnostic_records=0,diagnostic_differences=[],stack_changes=changed(native),reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diag=bool(op.get('diagnostic'))
        report['diagnostic_records' if diag else 'strict']+=1
        if a!=b:report['diagnostic_differences' if diag else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diag:
            report['admissions']+=1
            assert a==b,('positive admission',op,a,b)
    (folder/'damage-range-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'] or report['stack_changes'])

if __name__=='__main__':sys.exit(main())
