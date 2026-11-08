"""Actual interleaved entity selectors: global slice and strict spatial diagnostics."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

ACTORS = [('a', 'pig', [17.5, 200, 16.25]), ('b', 'armor_stand', [15.75, 200, 15.5]),
          ('c', 'item', [15.5, 200, 17]), ('d', 'pig', [17, 200, 15.75]),
          ('e', 'armor_stand', [18.5, 200, 16.5]), ('f', 'item', [14, 200, 16.75])]
UUID_LAST = dict(zip('abcdef', [41, 7, 93, 2, 68, 11]))
QUERIES = [('global_all', 'sort=arbitrary', 'abcdef', 'global'),
           ('global_limit2', 'sort=arbitrary,limit=2', 'ab', 'global'),
           ('global_limit6', 'sort=arbitrary,limit=6', 'abcdef', 'global'),
           ('filtered_limit3', 'type=!minecraft:armor_stand,sort=arbitrary,limit=3', 'acd', 'global'),
           ('spatial_all', 'distance=..3,sort=arbitrary', 'bcfdae', 'spatial'),
           ('spatial_limit2', 'distance=..3,sort=arbitrary,limit=2', 'bc', 'spatial'),
           ('nearest', 'sort=nearest', 'bdcafe', 'control'),
           ('furthest', 'sort=furthest', 'efacdb', 'control')]

def uid(name):
    return f'0000010c-0000-0000-0000-{UUID_LAST[name]:012x}'

def words(name):
    return '[I;268,0,0,'+str(UUID_LAST[name])+']'

def program():
    ops=[]
    def cmd(text,label,result=None,group='control',raw=False,**extra):
        row=dict(op='cmd',cmd='/'+text,label=label,group=group,admission=group=='control' and not raw)
        if result is not None:row['expect_result']=result
        if raw:row['raw_diagnostic']=True
        row.update(extra);ops.append(row)
    cmd('tick freeze','freeze',1)
    cmd('gamemode spectator','owner-mode')
    cmd('gamerule spawn_mobs false','no-spawns')
    cmd('forceload add 0 0 31 31','force-load',4)
    ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
    cmd('fill 0 199 0 31 199 31 stone','floor',1024)
    cmd('tp @s 16.0 205.0 16.0 0 0','owner-position',1)
    cmd('execute if entity @s[type=player,nbt={Health:20f,Pos:[16d,205d,16d],playerGameType:3}]','owner-state',1)
    cmd('data get entity @s UUID','owner-uuid',4)
    cmd('scoreboard objectives add h268 dummy','objective',1)
    cmd('execute store result score #start h268 run time query gametime','epoch-start',raw=True)
    for n,(name,kind,pos) in enumerate(ACTORS,1):
        at=' '.join(str(float(v)) for v in pos);coords='['+','.join(str(v)+'d' for v in pos)+']'
        extra=',NoAI:1b,Health:10f,PersistenceRequired:1b' if kind=='pig' else ',Health:20f' if kind=='armor_stand' else ',Item:{id:"minecraft:stone",count:1},PickupDelay:32767s'
        tag='{UUID:'+words(name)+',Tags:["h268"],NoGravity:1b,Silent:1b,Motion:[0d,0d,0d],Rotation:[0f,0f]'+extra+'}'
        cmd('scoreboard players set #r h268 91',name+'-summon-reset-r',91)
        cmd('scoreboard players set #s h268 17',name+'-summon-reset-s',17)
        cmd('execute store result score #r h268 store success score #s h268 run summon '+kind+' '+at+' '+tag,name+'-actual-summon',1)
        cmd('scoreboard players get #r h268',name+'-summon-result',1)
        cmd('scoreboard players get #s h268',name+'-summon-success',1)
        cmd('execute if entity @e[tag=h268]',name+'-chronological-count',n)
        typed='{UUID:'+words(name)+',NoGravity:1b,Pos:'+coords+',Motion:[0d,0d,0d]'+extra+'}'
        cmd('execute if entity @e[type='+kind+',nbt='+typed+']',name+'-actual-type-identity-state',1)
        for field,count in [('UUID',4),('Pos',3),('Motion',3),('NoGravity',1)]:cmd('data get entity '+uid(name)+' '+field,name+'-saved-'+field,count)
        if kind!='item':cmd('data get entity '+uid(name)+' Health',name+'-health',10 if kind=='pig' else 20)
        else:cmd('data get entity '+uid(name)+' Item',name+'-actual-item',2)
        cmd('execute as '+uid(name)+' at @s if loaded ~ ~ ~',name+'-loaded',1)
        cmd('execute as '+uid(name)+' at @s if block ~ 199 ~ stone',name+'-floor',1)
        cmd('data get entity '+uid(name),name+'-full-before',raw=True)
    for name,options,order,group in QUERIES:
        selector='@e[tag=h268,'+options+']';path=name+'.order'
        cmd('data modify storage h268:proof '+path+' set value []',name+'-empty-order',1)
        for holder,value in [('r',91),('s',17),('count',-99)]:cmd(f'scoreboard players set #{holder} h268 {value}',name+'-reset-'+holder,value)
        cmd('execute positioned 16.0 200.0 16.0 store result score #r h268 store success score #s h268 as '+selector+' run data modify storage h268:proof '+path+' append from entity @s UUID',name+'-actual-ordered-action',group=group,expect_values=[1]*len(order))
        cmd('scoreboard players get #r h268',name+'-result',1)
        cmd('scoreboard players get #s h268',name+'-success',1)
        cmd('execute positioned 16.0 200.0 16.0 store result score #count h268 run execute if entity '+selector,name+'-actual-limit-count',len(order))
        cmd('scoreboard players get #count h268',name+'-fresh-count-store',len(order))
        cmd('data get storage h268:proof '+path,name+'-full-ordered-uuid-list',len(order),group=group)
        for i,actor in enumerate(order):
            # Read the actual appended array at its actual index, not an unordered list predicate.
            cmd(f'data get storage h268:proof {path}[{i}][3]',name+'-ordered-index-'+str(i),UUID_LAST[actor],group=group)
    for name,kind,pos in ACTORS:
        cmd('execute if entity @e[type='+kind+',nbt={UUID:'+words(name)+',NoGravity:1b,Pos:['+','.join(str(v)+'d' for v in pos)+']}]',name+'-unchanged-state',1)
        cmd('data get entity '+uid(name),name+'-full-after',raw=True)
    cmd('execute store result score #now h268 run time query gametime','epoch-end',raw=True)
    cmd('scoreboard players operation #now h268 -= #start h268','relative-clock-op',0)
    cmd('scoreboard players get #now h268','relative-clock',0)
    cmd('kill @e[tag=h268]','cleanup-actors',6)
    cmd('forceload remove 0 0 31 31','cleanup-load',4)
    cmd('scoreboard objectives remove h268','cleanup-objective',0)
    cmd('tick unfreeze','cleanup-unfreeze',0)
    return ops

def main():
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir',type=Path)
    parser.add_argument('--strict-all',action='store_true')
    options,remaining=parser.parse_known_args();sys.argv=[sys.argv[0]]+remaining
    if '--help' in sys.argv or '-h' in sys.argv:driver.main();return 0
    if '--work-dir' not in sys.argv:parser.error('--work-dir must be a new capture directory')
    folder=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not folder.exists(),'preserve raw attempts'
    driver.corpus_module.groups=lambda:[('Interleaved global selector order',program())]
    if '--allow-gaps' not in sys.argv:sys.argv.append('--allow-gaps')
    driver.main()
    rows=lambda f:[json.loads(line) for line in f.read_text().splitlines()]
    ops=[o for o in program() if o['op']=='cmd' and o.get('record',True)]
    fresh=rows(folder/'vanilla.jsonl');assert len(fresh)==len(ops)
    if '--reference-only' in sys.argv:return 0
    reference=options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java,native=rows(reference),rows(folder/'rust.jsonl');assert len(java)==len(native)==len(ops)
    assert all(a==b for op,a,b in zip(ops,java,fresh) if not op.get('raw_diagnostic')),'independent fresh Java non-epoch facts changed'
    report=dict(primary_total=0,primary_failures=[],spatial_strict_total=0,spatial_strict_failures=[],full_total=len(ops),full_failures=[],admissions=0,raw_diagnostic_rows=0,reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diff=dict(index=i,label=op['label'],java=a,native=b)
        if a!=b:report['full_failures'].append(diff)
        if op.get('raw_diagnostic'):report['raw_diagnostic_rows']+=1;continue
        spatial=op['group']=='spatial';key='spatial_strict' if spatial else 'primary'
        report[key+'_total']+=1
        if a!=b:report[key+'_failures'].append(diff)
        if op['admission']:
            report['admissions']+=1
            assert a==b,('independent actual setup/control',op,a,b)
    (folder/'selector-order-report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
    return bool(report['full_failures'] if options.strict_all else report['primary_failures'])

if __name__=='__main__':sys.exit(main())
