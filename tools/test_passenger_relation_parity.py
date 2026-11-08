"""Actual direct-passenger command relation membership; reverse-order and full saves remain raw diagnostics."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

"""H150 predeclared actual direct passenger relation program; no custom observer."""
ACTORS=[('root','cow'),('child','pig'),('grandchild','chicken'),('boat','oak_boat'),('boat_child','cow'),('empty','pig'),('unrelated','cow'),('multi','cow'),('first','pig'),('second','chicken'),('reverse','oak_boat'),('rev_first','pig'),('rev_second','chicken')]
def uuid(name):
 i=1+next(i for i,(n,_) in enumerate(ACTORS) if n==name)
 return f'00000096-0000-0001-0000-{i:012x}'
def words(name):
 return '[I;150,1,0,'+str(1+next(i for i,(n,_) in enumerate(ACTORS) if n==name))+']'
def program():
 ops=[];labels={}
 def cmd(text,label,result=None,admission=True,diagnostic=None,**extra):
  labels[label]=labels.get(label,0)+1
  if labels[label]>1:label+='-'+str(labels[label])
  row=dict(op='cmd',cmd='/'+text.lstrip('/'),label=label,admission=admission)
  if result is not None: row['expect_result']=result
  if diagnostic:row['diagnostic']=diagnostic
  row.update(extra);ops.append(row)
 cmd('tick freeze','freeze',1);cmd('gamemode spectator','spectator');cmd('gamerule spawn_mobs false','no-spawns')
 cmd('forceload add 0 0','load',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('fill 0 199 0 15 199 15 stone','floor',256);cmd('tp @s 8.5 205 8.5 0 0','owner-position',1)
 cmd('execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,205d,8.5d],playerGameType:3}]','owner-state',1)
 cmd('data get entity @s UUID','owner-uuid',4)
 for number,obj in enumerate(['h150','h150r','h150s'],1):cmd(f'scoreboard objectives add {obj} dummy',obj+'-objective',number)
 cmd('execute store result score #start h150 run time query gametime','epoch-start',diagnostic='absolute epoch')
 for name,kind in ACTORS:
  mob=kind!='oak_boat';extra=',NoAI:1b,Health:1f' if mob else ''
  nbt='{UUID:'+words(name)+',Tags:["h150"],NoGravity:1b,Silent:1b,Motion:[0d,0d,0d],Rotation:[0f,0f]'+extra+'}'
  cmd(f'summon {kind} 8.5 200 8.5 '+nbt,name+'-summon',1)
  typed='{UUID:'+words(name)+',NoGravity:1b,Pos:[8.5d,200d,8.5d],Motion:[0d,0d,0d],Rotation:[0f,0f]'+extra+'}'
  cmd(f'execute if entity @e[type={kind},nbt={typed}]',name+'-actual-identity-state',1)
  for field,num in [('UUID',4),('Pos',3),('Motion',3)]:cmd('data get entity '+uuid(name)+' '+field,name+'-'+field,num)
  cmd('execute if loaded 8 200 8',name+'-loaded',1);cmd('execute if block 8 199 8 stone',name+'-floor',1)
  cmd('data get entity '+uuid(name),name+'-full-before',diagnostic='complete actor state/schema/order')
 def mount(child,parent):
  cmd('ride '+uuid(child)+' mount '+uuid(parent),child+'-mount-'+parent,1)
  vehicle(child,parent)
 def vehicle(child,parent):
  cmd('execute as '+uuid(child)+' on vehicle if entity @s[nbt={UUID:'+words(parent)+'}]',child+'-actual-vehicle-'+parent,1)
 def phase(label,parent,selected,watch,depth=1,order_diagnostic=False):
  for obj,value in [('h150',0),('h150r',91),('h150s',17)]:cmd(f'scoreboard players set @e[tag=h150] {obj} {value}',label+'-reset-'+obj)
  route='execute as '+uuid(parent)+' on passengers'+(' on passengers' if depth==2 else '')
  # Per-child stores are after relation expansion; no branch writes another child's counters.
  cmd(route+' store result score @s h150r store success score @s h150s run scoreboard players add @s h150 1',label+'-actual-action',admission=not selected,diagnostic='unsupported reverse mount ordering' if order_diagnostic else None,expect_values=[1]*len(selected))
  for child in watch:
   yes=child in selected
   for obj,value in [('h150',int(yes)),('h150r',1 if yes else 91),('h150s',1 if yes else 17)]:
    cmd('scoreboard players get '+uuid(child)+' '+obj,label+'-'+child+'-'+obj,value,admission=not yes)
  cmd('data get entity '+uuid(parent),label+'-full-parent',diagnostic='complete parent tree/getter/H151/order')
 mount('child','root');mount('grandchild','child');mount('boat_child','boat')
 mount('first','multi');mount('second','multi')
 # Created rev_first then rev_second, but mounted in the opposite order. Full action remains a declared diagnostic.
 mount('rev_second','reverse');mount('rev_first','reverse')
 phase('direct','root',['child'],['child','grandchild','root','unrelated'])
 phase('nested','root',['grandchild'],['child','grandchild','root'],depth=2)
 phase('boat','boat',['boat_child'],['boat_child','boat','unrelated'])
 phase('empty','empty',[],['empty','unrelated'])
 phase('multi','multi',['first','second'],['first','second','multi','unrelated'])
 phase('reverse','reverse',['rev_first','rev_second'],['rev_first','rev_second','reverse'],order_diagnostic=True)
 cmd('ride '+uuid('child')+' dismount','child-dismount',1)
 cmd('execute as '+uuid('child')+' on vehicle if entity @s','child-no-vehicle',expect_values=[])
 vehicle('grandchild','child')
 phase('dismounted','root',[],['child','grandchild','root'])
 # Position after dismount is a separate physical observation; not silently substituted for relation membership.
 cmd('data get entity '+uuid('child')+' Pos','dismount-pos',diagnostic='dismount mechanics outside relation selection')
 mount('child','root')
 phase('remounted','root',['child'],['child','grandchild','root'])
 for name,kind in ACTORS:
  extra=',NoAI:1b,Health:1f' if kind!='oak_boat' else ''
  cmd('execute if entity @e[type='+kind+',nbt={UUID:'+words(name)+',NoGravity:1b'+extra+'}]',name+'-final-identity-state',1)
  cmd('data get entity '+uuid(name),name+'-full-after',diagnostic='complete actor state/schema/order/physics')
 cmd('execute store result score #now h150 run time query gametime','epoch-end',diagnostic='absolute epoch')
 cmd('scoreboard players operation #now h150 -= #start h150','relative-clock-op',0);cmd('scoreboard players get #now h150','relative-clock',0)
 cmd('kill @e[tag=h150]','cleanup-actors',len(ACTORS));cmd('forceload remove 0 0','unload',1)
 for number,obj in enumerate(['h150','h150r','h150s']):cmd('scoreboard objectives remove '+obj,'cleanup-'+obj,2-number)
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
    driver.corpus_module.groups=lambda:[('Direct passenger relation',program())]
    if '--allow-gaps' not in sys.argv:sys.argv.append('--allow-gaps')
    driver.main()
    rows=lambda f:[json.loads(line) for line in f.read_text().splitlines()]
    ops=[o for o in program() if o['op']=='cmd' and o.get('record',True)]
    fresh=rows(folder/'vanilla.jsonl');assert len(fresh)==len(ops)
    for op,row in zip(ops,fresh):
        if 'expect_values' in op:assert row.get('results')==[[True,n] for n in op['expect_values']],('fresh Java callback guard',op,row)
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
    (folder/'passenger-relation-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])

if __name__=='__main__':sys.exit(main())
