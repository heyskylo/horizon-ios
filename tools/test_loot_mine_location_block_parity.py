import hashlib,json,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
TABLE={'type':'minecraft:block','pools':[
 {'rolls':1,'entries':[{'type':'minecraft:item','name':'minecraft:emerald','modifier':{'type':'minecraft:set_count','count':2}}]},
 {'rolls':1,'condition':{'type':'minecraft:location_check','offsetX':1,'predicate':{'block':{'blocks':'minecraft:oak_log','state':{'axis':'x'}}}},'entries':[{'type':'minecraft:item','name':'minecraft:diamond'}]}
]}
CASES=[('correct',4,4,'oak_log[axis=x]',True),('wrong-state',8,4,'oak_log[axis=z]',False),('absent',12,4,None,False),('negative-center',-4,-4,'oak_log[axis=x]',True)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h079 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h079 store success score success h079 run '+text,label+'-read',admission=False)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h079',label+'-'+k,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight');cmd('/gamerule random_tick_speed 0','no-random')
 cmd('/forceload add -16 -16 15 15','force',4);ops.append(dict(op='settle',required_chunks=[[-1,-1],[-1,0],[0,-1],[0,0]]))
 cmd('/fill -16 199 -16 15 199 15 stone','floor',1024);cmd('/fill -16 200 -16 15 204 15 barrier','clear-prefill',5120);cmd('/fill -16 200 -16 15 204 15 air','clear',5120)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','actual-owner',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/item replace entity @s weapon.mainhand with stick','plain-held',1);cmd('/data get entity @s SelectedItem','actual-held')
 cmd('/scoreboard objectives add h079 dummy','objective');cmd('/function h079:admit','pack-function');cmd('/scoreboard players get pack h079','pack-score',42)
 cmd('/execute store result score start h079 run time query gametime','clock-start',trace=True)
 for label,x,z,other,positive in CASES:
  src=f'{x} 200 {z}';neighbor=f'{x+1} 200 {z}';dest=f'{x} 200 10'
  cmd('/execute if block '+src+' air',label+'-initial-source-air',1);cmd('/setblock '+src+' stone',label+'-source-place',1)
  if other:cmd('/setblock '+neighbor+' '+other,label+'-counterpart-place',1)
  for p in [src,neighbor,dest]:
   cmd('/execute if loaded '+p,label+'-loaded-'+p,1);cmd('/execute if block '+p.replace('200','199')+' stone',label+'-floor-'+p,1)
  for phase in ['before','after']:
   if phase=='after':
    cmd('/execute if block '+src+' stone',label+'-source-unchanged',1);cmd('/execute if block '+neighbor+' '+(other or 'air'),label+'-counterpart-unchanged',1);continue
   cmd('/execute if block '+src+' stone',label+'-actual-source',1);cmd('/execute if block '+neighbor+' '+(other or 'air'),label+'-actual-counterpart',1)
   cmd('/setblock '+dest+' chest[facing=north,type=single,waterlogged=false]',label+'-chest-place',1)
   cmd('/execute if block '+dest+' chest[facing=north,type=single,waterlogged=false]',label+'-chest-type',1);cmd('/data get block '+dest+' id',label+'-chest-id',15);cmd('/data get block '+dest+' Items',label+'-empty',0)
   cmd('/execute as Player if entity @s[type=player,nbt={Health:20f,playerGameType:1}]',label+'-actual-source-player',1)
   prefix='execute as Player positioned -8.25 200.125 -8.75 run ' if x<0 else 'execute as Player run '
   read(prefix+'loot insert '+dest+' mine '+src+' stick',label+'-mine')
   cmd('/data get block '+dest+' Items',label+'-full-output',admission=False)
   read('data get block '+dest+' Items[{id:"minecraft:diamond"}]',label+'-diamond-stack')
   read('data get block '+dest+' Items[{id:"minecraft:diamond"}].count',label+'-diamond-count')
   cmd('/execute if data block '+dest+' {Items:[{Slot:0b,id:"minecraft:emerald",count:2}]}',label+'-pack-unconditional',1)
   cmd('/execute if data block '+dest+' {Items:[{Slot:1b,id:"minecraft:diamond",count:1}]}',label+'-conditional-type',admission=False)
 cmd('/execute store result score end h079 run time query gametime','clock-end',trace=True);cmd('/scoreboard players operation end h079 -= start h079','clock-delta-operation',0);cmd('/scoreboard players get end h079','clock-delta',0)
 return ops
if __name__=='__main__':
 ref=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');ref=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h079/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(rows),labels=[o['label'] for o in rows],table=TABLE,contract='All complete records strict; absolute clocks alone trace-only. All counterparts loaded; missing/unloaded chunks outside runtime claim.'),indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Loot mine location block offset',ops)];driver.corpus_module.function_return_fixtures=lambda names:{'h079:admit':'scoreboard players set pack h079 42\n'};driver.corpus_module.advancement_loot_fixtures=lambda names:({}, {'minecraft:blocks/stone':TABLE})
 sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if ref:assert fresh==ref,'fresh Java reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 java=[json.loads(l) for l in fresh.splitlines()];native=[json.loads(l) for l in (build/'rust.jsonl').read_bytes().splitlines()];assert len(java)==len(native)==len(rows)
 diffs=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(rows,java,native) if a!=b];(build/'strict-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,a,b in zip(rows,java,native):
  if o['admission']:assert a==b,('admission',o,a,b)
 print(f'{len(rows)-len(diffs)}/{len(rows)}');sys.exit(bool(diffs))
