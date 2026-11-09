"""H289 named mob-equipment custom_data; actual state stays typed NBT."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

PATTERNS = {
    'byte': '{h289:1b}',
    'typed_int': '{h289:1}',
    'nested': '{nested:{k:7}}',
    'list_subset': '{entries:[{k:2b}]}',
    'list_order': '{entries:[{k:2b},{k:1b}]}',
    'empty_list': '{entries:[]}',
    'long': '{wide:9007199254740993L}',
    'wrong_long': '{wide:9007199254740992L}',
    'empty': '{}',
    'two': '{h289:2b}',
}
ACTORS = [
    ('rich', '{h289:1b,nested:{k:7,extra:9},entries:[{k:1b,extra:2b},{k:2b}],wide:9007199254740993L,spare:"keep"}', [('byte',True),('typed_int',False),('nested',True),('list_subset',True),('list_order',True),('empty_list',False),('long',True),('wrong_long',False)]),
    ('missing', '{other:1b}', [('byte',False)]),
    ('wrong', '{h289:2b}', [('byte',False),('two',True)]),
    ('plain', None, [('byte',False),('empty',True)]),
    ('nested_wrong', '{h289:1b,nested:{k:8,extra:9}}', [('nested',False),('byte',True)]),
    ('empty_list', '{entries:[]}', [('empty_list',True),('list_subset',False)]),
]


def predicates():
    def equipment(slot, want):
        return {'type':'minecraft:entity_properties','entity':'this',
                'predicate':{'minecraft:entity_type':'minecraft:pig', 'minecraft:equipment':{slot:want}}}
    always={'type':'minecraft:all_of','terms':[]}
    out={'h289:always':always, 'h289:never':{'type':'minecraft:inverted','term':always},
         'h289:slot':equipment('mainhand',{'items':'minecraft:stick','count':1}),
         'h289:wrong_item':equipment('mainhand',{'items':'minecraft:paper'}),
         'h289:wrong_count':equipment('mainhand',{'items':'minecraft:stick','count':2}),
         'h289:empty_offhand':equipment('offhand',{'items':'minecraft:stick'})}
    for name,pattern in PATTERNS.items():
        out['h289:'+name]=equipment('mainhand',{'items':'minecraft:stick','count':1,
                    'predicates':{'minecraft:custom_data':pattern}})
    return out


def program():
    ops=[]
    def cmd(text,label,result=None,callback=None,admission=True,diagnostic=None,snapshot=None):
        op=dict(op='cmd',cmd=text,label=label,admission=admission)
        if result is not None:op['expect_result']=result
        if callback is not None:op['expect_callback']=callback
        if diagnostic:op['diagnostic']=diagnostic
        if snapshot:op.update(actor_snapshot=snapshot[0],snapshot_phase=snapshot[1])
        ops.append(op)
    cmd('/tick freeze','freeze',1)
    cmd('/gamerule spawn_mobs false','no-spawns')
    cmd('/gamerule random_tick_speed 0','no-random-ticks')
    cmd('/forceload add 0 0','forceload',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone','floor',256)
    cmd('/tp @s 8.5 200 8.5 0 0','owner-position',1)
    cmd('/scoreboard objectives add h289 dummy','objective',1)
    cmd('/datapack list enabled','pack-enabled')
    cmd('/execute if predicate h289:always','registry-always',1)
    cmd('/execute unless predicate h289:never','registry-never',1)
    cmd('/execute store result score start h289 run time query gametime','absolute-start',diagnostic='absolute startup epoch; relative zero strict')
    for i,(name,data,probes) in enumerate(ACTORS,1):
        actor=f'00000121-0000-0001-0000-{i:012x}'
        uid=f'[I;289,1,0,{i}]';x=i+0.5
        components='' if data is None else ',components:{"minecraft:custom_data":'+data+'}'
        fields=f'UUID:{uid},Tags:["h289_subject"],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f,equipment:{{mainhand:{{id:"minecraft:stick",count:1{components}}}}}'
        cmd(f'/summon minecraft:pig {x} 200 4.5 '+'{'+fields+'}',name+'-summon',1)
        body=f'UUID:{uid},Health:10f,Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b'
        cmd(f'/execute as {actor} if entity @s[type=minecraft:pig,tag=h289_subject,nbt={{{body}}}]',name+'-actual-actor',1)
        cmd(f'/execute if loaded {i} 200 4',name+'-loaded',1)
        cmd(f'/execute if block {i} 199 4 stone',name+'-floor',1)
        cmd(f'/data get entity {actor} UUID',name+'-UUID',4)
        cmd(f'/data get entity {actor} Health',name+'-health',10)
        cmd(f'/data get entity {actor} equipment.mainhand.id',name+'-item-id',len('minecraft:stick'))
        cmd(f'/data get entity {actor} equipment.mainhand.count',name+'-item-count',1)
        cmd(f'/execute unless data entity {actor} equipment.offhand',name+'-actual-empty-offhand',1)
        if data is None:
            cmd(f'/execute unless data entity {actor} equipment.mainhand.components."minecraft:custom_data"',name+'-actual-custom-data-absent',1)
        else:
            cmd(f'/execute if data entity {actor} equipment.mainhand.components{{"minecraft:custom_data":{data}}}',name+'-actual-typed-custom-data',1)
            cmd(f'/data get entity {actor} equipment.mainhand.components."minecraft:custom_data"',name+'-custom-data-body',diagnostic='complete typed custom_data/order')
        cmd(f'/data get entity {actor} equipment.mainhand',name+'-stack-before',diagnostic='complete actual saved stack/order',snapshot=(name,'before'))
        cmd(f'/data get entity {actor}',name+'-actor-body',diagnostic='complete actual saved actor/order/clock')
        for control,want in [('slot',True),('wrong_item',False),('wrong_count',False),('empty_offhand',False)]:
            cmd(f'/execute as {actor} if predicate h289:{control}',name+'-control-'+control,callback=[want,int(want)])
        for pred,want in probes:
            label=name+'-'+pred
            cmd('/scoreboard players set result h289 31',label+'-fresh-result',31)
            cmd('/scoreboard players set success h289 17',label+'-fresh-success',17)
            cmd(f'/execute store result score result h289 store success score success h289 as {actor} if predicate h289:{pred}',label+'-named-query',callback=[want,int(want)],admission=want)
            cmd('/scoreboard players get result h289',label+'-result',int(want),admission=want)
            cmd('/scoreboard players get success h289',label+'-success',int(want),admission=want)
            cmd(f'/execute as {actor} unless predicate h289:{pred}',label+'-unless',callback=[not want,int(not want)],admission=want)
        # The independent command subpredicate route is retained as H195 control diagnostics.
        cmd(f'/execute if items entity {actor} weapon.mainhand minecraft:stick[minecraft:custom_data~{{h289:1b}}]',name+'-direct-command-control',diagnostic='independent command subpredicate/getter (H195 and #11286); unchanged by H289')
        cmd(f'/data get entity {actor} equipment.mainhand',name+'-stack-after',diagnostic='complete actual saved stack/order',snapshot=(name,'after'))
        cmd('/execute store result score now h289 run time query gametime',name+'-absolute-clock',diagnostic='absolute epoch')
        cmd('/execute if score now h289 = start h289',name+'-relative-zero',1)
    cmd('/kill @e[tag=h289_subject]','cleanup-actors',len(ACTORS))
    cmd('/forceload remove 0 0','cleanup-force',1)
    cmd('/scoreboard objectives remove h289','cleanup-objective')
    cmd('/tick unfreeze','cleanup-unfreeze',0)
    return ops


def snapshot_changes(ops, rows):
    groups = {}
    for op,row in zip(ops,rows):
        if 'actor_snapshot' in op:
            groups.setdefault(op['actor_snapshot'],{})[op['snapshot_phase']] = row
    assert len(groups)==len(ACTORS)
    return [name for name,pair in groups.items() if pair['before']!=pair['after']]


def main():
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir',type=Path)
    options,rest=parser.parse_known_args()
    sys.argv=[sys.argv[0],*rest]
    if '--work-dir' not in sys.argv:
        parser.error('--work-dir must name a fresh capture directory')
    folder=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not folder.exists(),'preserve previous attempts'
    driver.corpus_module.groups=lambda:[('Named mob equipment custom_data',program())]
    driver.corpus_module.predicate_selector_fixtures=lambda groups:predicates()
    if '--allow-gaps' not in sys.argv:
        sys.argv.append('--allow-gaps')
    driver.main()
    rows=lambda f:[json.loads(line) for line in f.read_text().splitlines()]
    ops=[o for o in program() if o['op']=='cmd' and o.get('record',True)]
    fresh=rows(folder/'vanilla.jsonl')
    assert len(fresh)==len(ops)
    assert not snapshot_changes(ops,fresh),'Java observations changed actual equipment'
    if '--reference-only' in sys.argv:
        return 0
    reference=options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java,native=rows(reference),rows(folder/'rust.jsonl')
    assert len(java)==len(native)==len(ops)
    assert all(a==b for o,a,b in zip(ops,java,fresh) if not o.get('diagnostic')),'fresh strict Java reference changed'
    report=dict(strict=0,admissions=0,strict_failures=[],diagnostic_records=0,diagnostic_differences=[],equipment_changes=snapshot_changes(ops,native),reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diagnostic=bool(op.get('diagnostic'))
        report['diagnostic_records' if diagnostic else 'strict']+=1
        if a!=b:
            report['diagnostic_differences' if diagnostic else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diagnostic:
            report['admissions']+=1
            assert a==b,('positive admission',op,a,b)
    (folder/'mob-custom-data-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'] or report['equipment_changes'])


if __name__=='__main__':
    sys.exit(main())
