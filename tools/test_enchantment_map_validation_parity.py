#!/usr/bin/env python3
"""Enchantment-map command admission; builtin contract and separate loader diagnostic."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES=[('negative','{sharpness:-1}'),('zero','{sharpness:0}'),
       ('one','{sharpness:1}'),('max','{sharpness:255}'),('over','{sharpness:256}'),
       ('large','{sharpness:100000}'),('float','{sharpness:1.5f}'),('byte','{sharpness:1b}'),
       ('string','{sharpness:"1"}'),('legacy','{levels:{sharpness:1}}'),('empty','{}'),
       ('unknown','{"minecraft:h024_missing":1}'),('qualified','{"minecraft:sharpness":1}'),
       ('long_wrap','{sharpness:4294967297L}'),('long_precise','{sharpness:9007199254740993L}'),
       ('negative_half','{sharpness:-0.5d}'),
       ('custom_unknown','{"h024:missing":1}')]
LOADER_CASES = [('negative', '{sharpness:-1}'), ('zero', '{sharpness:0}'), ('one', '{sharpness:1}'), ('max', '{sharpness:255}'), ('over', '{sharpness:256}'), ('large', '{sharpness:100000}'), ('float', '{sharpness:1.5f}'), ('byte', '{sharpness:1b}'), ('string', '{sharpness:"1"}'), ('legacy', '{levels:{sharpness:1}}'), ('empty', '{}'), ('unknown', '{"minecraft:h024_missing":1}'), ('qualified', '{"minecraft:sharpness":1}'), ('long_wrap', '{sharpness:4294967297L}'), ('long_precise', '{sharpness:9007199254740993L}'), ('negative_half', '{sharpness:-0.5d}'), ('custom', '{"h024:audit":1}'), ('custom_unknown', '{"h024:missing":1}')]
CUSTOM={'description':'H024 audit','supported_items':'minecraft:diamond_sword','weight':1,'max_level':1,
        'min_cost':{'base':1,'per_level_above_first':0},'max_cost':{'base':1,'per_level_above_first':0},
        'anvil_cost':1,'slots':['mainhand'],'effects':{}}
_original_installer=driver.install_datapack_fixtures

def install(world,predicates,functions,jar,*args,**kwargs):
    functions=dict(functions);functions['h024:ready']='return 1\n'
    _original_installer(world,predicates,functions,jar,*args,**kwargs)
    target=world/'datapacks/command-selector-fixture/data/h024/enchantment/audit.json'
    target.parent.mkdir(parents=True,exist_ok=True);target.write_text(json.dumps(CUSTOM))



def program(loader=False):
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
    cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-place',1)
    cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1)
    cmd('/gamemode creative @s','owner-mode')
    cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
    cmd('/data get entity @s UUID','owner-uuid',4)
    cmd('/execute if loaded 4 200 4','loaded',1)
    cmd('/execute if block 4 199 4 stone','support',1)
    cmd('/execute if block 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-state',1)
    for key,value in [('id',15),('x',4),('y',200),('z',4),('Items',0)]:
        cmd('/data get block 4 200 4 '+key,'chest-'+key,value)
    if loader:
        cmd('/datapack list enabled','pack-admission')
        cmd('/function h024:ready','function-admission',1)
        # This precondition needs the actual definition: only_compatible filters unknown enchantments.
        table={'pools':[{'rolls':1,'entries':[{'type':'minecraft:item','name':'minecraft:diamond_sword',
            'modifier':{'type':'minecraft:enchant_randomly','options':'h024:audit','only_compatible':True}}]}]}
        cmd('/loot replace block 4 200 4 container.1 loot '+json.dumps(table,separators=(',',':')),'loaded-custom-loot',1)
        cmd('/execute if data block 4 200 4 {Items:[{Slot:1b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:enchantments":{"h024:audit":1}}}]}','actual-custom-definition',1)
        cmd('/data get block 4 200 4 Items[{Slot:1b}]','custom-witness-item')
    else:
        cmd('/item replace block 4 200 4 container.1 with stone[custom_data={keep:1}] 7','unrelated-place',1)
        cmd('/execute if data block 4 200 4 {Items:[{Slot:1b,id:"minecraft:stone",count:7,components:{"minecraft:custom_data":{keep:1}}}]}','unrelated-initial',1)
    cmd('/scoreboard objectives add h024 dummy','objective')
    cmd('/execute store result score start h024 run time query gametime','clock-start',diagnostic=True)
    for component,item in [('enchantments','diamond_sword'),('stored_enchantments','enchanted_book')]:
        path='Items[{Slot:0b}]'
        for label,value in (LOADER_CASES if loader else CASES):
            name=component+'-'+label
            cmd('/item replace block 4 200 4 container.0 with emerald[custom_data={h024:1}] 4',name+'-reset-target',1)
            cmd('/data get block 4 200 4 '+path,name+'-initial-stack',admission=True)
            for key in ['result','success']:
                cmd(f'/scoreboard players set {key} h024 -99',name+'-reset-'+key,-99)
            cmd(f'/execute store result score result h024 store success score success h024 run item replace block 4 200 4 container.0 with minecraft:{item}[{component}={value}]',name+'-replace',admission=False)
            for key in ['result','success']:
                cmd(f'/scoreboard players get {key} h024',name+'-'+key,admission=False)
            cmd('/data get block 4 200 4 '+path,name+'-saved-stack',admission=False,diagnostic=label=='empty')
            cmd('/data get block 4 200 4 '+path+'.id',name+'-saved-id',admission=False)
            cmd('/data get block 4 200 4 '+path+'.count',name+'-saved-count',admission=False)
            cmd('/data get block 4 200 4 '+path+'.components."minecraft:'+component+'"',name+'-saved-enchantments',admission=False)
            cmd('/execute if data block 4 200 4 {Items:[{Slot:0b,id:"minecraft:emerald",count:4,components:{"minecraft:custom_data":{h024:1}}}]}',name+'-initial-retained',admission=False)
    if loader:
        cmd('/execute if data block 4 200 4 {Items:[{Slot:1b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:enchantments":{"h024:audit":1}}}]}','custom-witness-retained',1)
    else:
        cmd('/execute if data block 4 200 4 {Items:[{Slot:1b,id:"minecraft:stone",count:7,components:{"minecraft:custom_data":{keep:1}}}]}','unrelated-retained',1)
    cmd('/data get block 4 200 4','final-full-body',diagnostic=True)
    cmd('/execute store result score end h024 run time query gametime','clock-end',diagnostic=True)
    cmd('/scoreboard players operation end h024 -= start h024','clock-delta-operation',0)
    cmd('/scoreboard players get end h024','clock-delta',0)
    return ops


def main():
    loader = "--loader-diagnostic" in sys.argv
    if loader:
        sys.argv.remove("--loader-diagnostic")
        driver.install_datapack_fixtures = install
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='enchantment-entry-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(loader); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=(LOADER_CASES if loader else CASES), labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete records, no masks/tolerances; bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Command enchantment entry admission', ops)]
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
