#!/usr/bin/env python3
"""World-pack enchantment catalog: deterministic compatible loot application."""
import json,sys,hashlib,tempfile
from pathlib import Path
import test_command_exec_parity as driver
CUSTOM=dict(description='World catalog loaded',supported_items='minecraft:diamond_sword',weight=1,max_level=1,min_cost=dict(base=1,per_level_above_first=0),max_cost=dict(base=1,per_level_above_first=0),anvil_cost=1,slots=['mainhand'],effects={'minecraft:damage':[{'effect':{'type':'minecraft:add','value':2.0}}]})
_original=driver.install_datapack_fixtures
def install(world,predicates,functions,jar,*args,**kwargs):
 functions=dict(functions);functions['wpenchant:ready']='return 1\n';_original(world,predicates,functions,jar,*args,**kwargs)
 p=world/'datapacks/command-selector-fixture/data/wpenchant/enchantment/loaded.json';p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(CUSTOM)+'\n')
driver.install_datapack_fixtures=install
CASES=[('custom_sword','minecraft:diamond_sword','wpenchant:loaded',True),('custom_stone','minecraft:stone','wpenchant:loaded',False),('vanilla_sword','minecraft:diamond_sword','minecraft:mending',True),('vanilla_stone','minecraft:stone','minecraft:mending',False)]
def program():
 ops=[]
 def c(cmd,label,result=None,admission=True,raw=False,java_results=None):
  o=dict(op='cmd',cmd=cmd,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if raw:o.update(record=False,trace=True)
  if java_results is not None:o['java_results']=java_results
  ops.append(o)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]));c('/fill 0 199 0 15 199 15 stone','floor',256);c('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-place',1);c('/tp @s 12.5 200 12.5 0 0','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/data get entity @s UUID','owner-uuid',4);c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[12.5d,200d,12.5d]}]','owner-body',1);c('/execute if loaded 4 200 4','loaded',1);c('/execute if block 4 199 4 stone','support',1);c('/execute if block 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-state',1)
 for field,result in [('id',15),('x',4),('y',200),('z',4),('Items',0)]:c('/data get block 4 200 4 '+field,'chest-'+field,result)
 c('/function wpenchant:ready','pack-function',1);c('/datapack list enabled','packs-raw',raw=True);c('/scoreboard objectives add wp dummy','objective');c('/execute store result score start wp run time query gametime','clock-start',raw=True)
 c('/item replace block 4 200 4 container.3 with emerald[custom_data={keep:17}] 3','unrelated-place',1)
 for name,item,enchantment,compatible in CASES:
  c('/item replace block 4 200 4 container.0 with diamond[custom_data={sentinel:23}] 4',name+'-sentinel',1)
  c('/execute if data block 4 200 4 Items[{Slot:0b,id:"minecraft:diamond",count:4,components:{"minecraft:custom_data":{sentinel:23}}}]',name+'-sentinel-check',1)
  for k in ['result','success']:c(f'/scoreboard players set {k} wp -99',name+'-reset-'+k,-99)
  table={'pools':[{'rolls':2,'entries':[{'type':'minecraft:item','name':item,'modifier':{'type':'minecraft:enchant_randomly','options':enchantment,'only_compatible':True}}]}]}
  c('/execute store result score result wp store success score success wp run loot replace block 4 200 4 container.0 loot '+json.dumps(table,separators=(',',':')),name+'-loot',2)
  for k in ['result','success']:c(f'/scoreboard players get {k} wp',name+'-'+k,2 if k=='result' else 1)
  base='Items[{Slot:0b}]';c('/data get block 4 200 4 '+base+'.id',name+'-id',len(item));c('/data get block 4 200 4 '+base+'.count',name+'-count',1)
  c('/data get block 4 200 4 Items[{Slot:1b}].id',name+'-second-id',len(item));c('/data get block 4 200 4 Items[{Slot:1b}].count',name+'-second-count',1)
  c('/data get block 4 200 4 Items[{Slot:1b}].components."minecraft:enchantments"."'+enchantment+'"',name+'-second-level',admission=name!='custom_sword',java_results=[[True,1]] if compatible else [[False,0]])
  path=base+'.components."minecraft:enchantments"'
  c('/execute if data block 4 200 4 '+path,name+'-enchantment-presence',admission=not name.startswith('custom_sword'),java_results=[[True,1]] if compatible else [[False,0]])
  for k in ['result','success']:c(f'/scoreboard players set {k} wp -99',name+'-read-reset-'+k,-99)
  c('/execute store result score result wp store success score success wp run data get block 4 200 4 '+path,name+'-enchantment-value',admission=not name.startswith('custom_sword'),java_results=[[True,1]] if compatible else [[False,0]])
  for k in ['result','success']:c(f'/scoreboard players get {k} wp',name+'-read-'+k,admission=not name.startswith('custom_sword'))
  c('/data get block 4 200 4 '+path+'."'+enchantment+'"',name+'-level',admission=not name.startswith('custom_sword'),java_results=[[True,1]] if compatible else [[False,0]])
  c('/execute if data block 4 200 4 Items[{Slot:3b,id:"minecraft:emerald",count:3,components:{"minecraft:custom_data":{keep:17}}}]',name+'-unrelated-unchanged',1)
  c('/data get block 4 200 4 '+base,name+'-full-stack',raw=True)
 c('/execute store result score end wp run time query gametime','clock-end',raw=True);c('/scoreboard players operation end wp -= start wp','clock-delta-operation',0);c('/scoreboard players get end wp','clock-delta',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).resolve();del sys.argv[i:i+2]
 if '--work-dir' not in sys.argv:
  parent=Path(tempfile.mkdtemp(prefix='world-pack-enchantments-',dir=driver.lab.LOCAL));sys.argv+=['--work-dir',str(parent/'run')]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists(),'preserve attempts';build.mkdir(parents=True)
 ops=program();recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)];(build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(recorded),labels=[o['label'] for o in recorded],definition=CUSTOM,cases=CASES),indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('World-pack enchantment catalog',ops)];expected=reference.read_bytes() if reference else None;sys.argv.append('--allow-gaps');driver.main();jb=(build/'vanilla.jsonl').read_bytes()
 if expected is not None:assert jb==expected and reference.read_bytes()==expected,'reference changed'
 java=[json.loads(s) for s in jb.splitlines()];assert len(java)==len(recorded)
 for o,row in zip(recorded,java):
  if 'java_results' in o:assert row['results']==o['java_results'],('Java admission',o,row)
 if '--reference-only' not in sys.argv:
  native=[json.loads(s) for s in (build/'rust.jsonl').read_text().splitlines()];assert len(native)==len(recorded);res=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(recorded,java,native) if a!=b];r=dict(records=len(recorded),passed=len(recorded)-len(res),residuals=res,reference_sha256=hashlib.sha256(jb).hexdigest());(build/'strict-report.json').write_text(json.dumps(r,indent=2)+'\n')
  for o,a,b in zip(recorded,java,native):
   if o['admission']:assert a==b,('admission',o,a,b)
  print(f'Strict records: {r["passed"]}/{r["records"]}')
  if res:raise SystemExit(1)
