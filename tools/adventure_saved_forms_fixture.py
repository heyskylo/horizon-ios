import json,hashlib,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
CASES=[
 ('absent',None,None,None),
 ('break-string','can_break','{blocks:"minecraft:stone"}','{blocks:"minecraft:stone"}'),
 ('break-singleton','can_break','{blocks:["minecraft:stone"]}','{blocks:"minecraft:stone"}'),
 ('break-multiple','can_break','{blocks:["minecraft:stone","minecraft:dirt"]}','{blocks:["minecraft:stone","minecraft:dirt"]}'),
 ('break-custom-compound','can_break','{predicates:{"minecraft:custom_data":{mark:7}}}','{predicates:{"minecraft:custom_data":"{mark:7}"}}'),
 ('break-custom-string','can_break','{predicates:{"minecraft:custom_data":"{mark:7}"}}','{predicates:{"minecraft:custom_data":"{mark:7}"}}'),
 ('break-state','can_break','{state:{axis:"y"}}','{state:{axis:"y"}}'),
 ('break-default','can_break','{}','{}'),
 ('break-list-two','can_break','[{blocks:"minecraft:stone"},{blocks:"minecraft:dirt"}]','[{blocks:"minecraft:stone"},{blocks:"minecraft:dirt"}]'),
 ('break-exact-custom','can_break','{components:{"minecraft:custom_data":{mark:7}}}','{components:{"minecraft:custom_data":{mark:7}}}'),
 ('place-singleton','can_place_on','{blocks:["minecraft:stone"]}','{blocks:"minecraft:stone"}'),
 ('place-custom-compound','can_place_on','{predicates:{"minecraft:custom_data":{mark:7}}}','{predicates:{"minecraft:custom_data":"{mark:7}"}}'),
 ('place-string','can_place_on','{blocks:"minecraft:stone"}','{blocks:"minecraft:stone"}'),
]
def program():
 ops=[]
 def c(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:c(f'/scoreboard players set {k} h285 -99',label+'-reset-'+k,-99)
  c('/execute store result score result h285 store success score success h285 run '+text,label+'-call',admission=admission)
  for k in ['result','success']:c(f'/scoreboard players get {k} h285',label+'-'+k,admission=admission)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/gamerule random_tick_speed 0','random0')
 c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 c('/fill 0 199 0 15 199 15 stone','floor',256);c('/fill 0 200 0 15 204 15 barrier','prefill',1280);c('/fill 0 200 0 15 204 15 air','air',1280)
 c('/tp @s 14.5 200 14.5','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/execute if entity @s[type=player,nbt={Health:20f,Pos:[14.5d,200d,14.5d],playerGameType:1}]','owner-body',1);c('/data get entity @s UUID','owner-uuid',4)
 c('/scoreboard objectives add h285 dummy','objective');c('/execute store result score start h285 run time query gametime','clock-start',trace=True)
 c('/execute if loaded 4 200 4','target-loaded',1);c('/execute if block 4 199 4 stone','target-floor',1);c('/setblock 4 200 4 chest strict','chest',1)
 read('execute if data block 4 200 4 {id:"minecraft:chest",x:4,y:200,z:4,Items:[]}','initial-chest-typed',True)
 for key in ['id','x','y','z']:read('data get block 4 200 4 '+key,'chest-'+key,True)
 for slot,(name,key,value,expected) in enumerate(CASES):
  patch='custom_data={keep:17},repair_cost=2'+(','+key+'='+value if key else '')
  read(f'item replace block 4 200 4 container.{slot} with minecraft:stone['+patch+'] 3',name+'-replace',True)
  base='{Slot:'+str(slot)+'b,id:"minecraft:stone",count:3,components:{"minecraft:custom_data":{keep:17},"minecraft:repair_cost":2}}'
  c('/execute if data block 4 200 4 {Items:['+base+']}',name+'-actual-item-identity-count-components',1)
  path=f'Items[{{Slot:{slot}b}}].components."minecraft:{key or "can_break"}"'
  read('data get block 4 200 4 '+path,name+'-complete-saved-component')
  if key:
   actual='{Items:[{Slot:'+str(slot)+'b,components:{"minecraft:'+key+'":'+expected+'}}]}'
   read('execute if data block 4 200 4 '+actual,name+'-complete-typed-saved-form')
  else:read('execute unless data block 4 200 4 '+path,name+'-absent')
  c(f'/data get block 4 200 4 Items[{{Slot:{slot}b}}]',name+'-full-stack',trace=True)
 read('data get block 4 200 4 Items','final-inventory-count')
 c('/execute if block 4 200 4 chest','chest-retained',1);c('/execute if block 4 199 4 stone','floor-retained',1)
 c('/data get block 4 200 4','full-chest',trace=True);c('/data get entity @s','owner-full-body',trace=True)
 c('/execute store result score end h285 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h285 -= start h285','clock-delta-op',0);c('/scoreboard players get end h285','clock-delta',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists();build.mkdir(parents=True);ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'scope':'saved-only two adventure predicate forms; actual chest and item/count/components/owner/load/floor/clock; unchanged string/list/empty/exact/state controls; full stacks/order/body raw; no adventure execution or packets'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Saved adventure predicate forms',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows);diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
