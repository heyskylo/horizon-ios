import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('white-unshorn',0,False),('white-shorn',0,True),('red-unshorn',14,False),('red-shorn',14,True)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h036 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h036 store success score success h036 run '+text,label+'-read',admission=False)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h036',label+'-'+k,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight');cmd('/gamerule random_tick_speed 0','no-random')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/fill 0 200 0 15 204 15 barrier','clear-prefill',1280);cmd('/fill 0 200 0 15 204 15 air','clear',1280)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','actual-owner',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/item replace entity @s weapon.mainhand with stick','plain-held',1);cmd('/data get entity @s SelectedItem','actual-held')
 cmd('/scoreboard objectives add h036 dummy','objective');cmd('/execute store result score start h036 run time query gametime','clock-start',trace=True)
 for n,(label,color,sheared) in enumerate(CASES,1):
  uid=str(uuid.UUID(int=(36<<96)|n));x=2+3*(n-1);z=4;ch=f'{x} 200 8';wool='white_wool' if color==0 else 'red_wool'
  actor=f'{{UUID:[I;36,0,0,{n}],Health:8f,NoAI:1b,NoGravity:1b,Age:0,Color:{color}b,Sheared:{int(sheared)}b,Motion:[0d,0d,0d],Tags:[h036]}}'
  cmd(f'/summon sheep {x}.5 200 {z}.5 '+actor,label+'-summon',1)
  guard=f'/execute as {uid} if entity @s[type=sheep,nbt='+actor[:-1]+f',Pos:[{x}.5d,200d,{z}.5d]'+'}]'
  cmd(guard,label+'-actual-before',1)
  for p in [f'{x} 200 {z}',ch]:
   cmd('/execute if loaded '+p,label+'-loaded-'+p,1);cmd('/execute if block '+p.replace('200','199')+' stone',label+'-floor-'+p,1)
  for key,v in [('UUID',4),('Health',8),('Pos',3),('Motion',3),('NoAI',1),('NoGravity',1),('Color',color),('Sheared',int(sheared))]:cmd(f'/data get entity {uid} {key}',label+'-body-'+key,v)
  cmd(f'/data get entity {uid}',label+'-full-before',trace=True)
  cmd(f'/setblock {ch} chest[facing=north,type=single,waterlogged=false]',label+'-chest-place',1)
  cmd(f'/execute if block {ch} chest[facing=north,type=single,waterlogged=false]',label+'-chest-type',1)
  cmd(f'/data get block {ch} id',label+'-chest-id',15);cmd(f'/data get block {ch} Items',label+'-initial-empty',0)
  cmd('/execute as Player if entity @s[type=player,nbt={Health:20f,playerGameType:1}]',label+'-actual-source',1)
  read(f'execute as Player run loot insert {ch} kill {uid}',label+'-loot')
  cmd(f'/data get block {ch} Items',label+'-full-output',trace=True)
  cmd(f'/data get block {ch} Items',label+'-stack-count',admission=False)
  read(f'data get block {ch} Items[{{id:"minecraft:{wool}"}}]',label+'-wool-stack')
  read(f'data get block {ch} Items[{{id:"minecraft:{wool}"}}].count',label+'-wool-count')
  cmd(f'/execute if data block {ch} '+'{Items:[{id:"minecraft:'+wool+'",count:1}]}',label+'-typed-wool',admission=False)
  cmd(f'/execute if data block {ch} '+'{Items:[{id:"minecraft:mutton"}]}',label+'-meat-positive',1)
  cmd(f'/execute store result score meat h036 run data get block {ch} Items[{{id:"minecraft:mutton"}}].count',label+'-random-meat-count',trace=True)
  cmd('/execute if score meat h036 matches 1..2',label+'-meat-table-domain',1)
  cmd(f'/data get block {ch} Items[{{id:"minecraft:mutton"}}]',label+'-meat-full',trace=True)
  cmd(guard,label+'-actual-after',1);cmd(f'/data get entity {uid}',label+'-full-after',trace=True)
 cmd('/setblock 2 200 12 chest','positive-chest',1);cmd('/data get block 2 200 12 Items','positive-empty',0)
 read('/loot insert 2 200 12 loot {pools:[{rolls:1,entries:[{type:"minecraft:item",name:"minecraft:diamond"}]}]}'.lstrip('/'),'unconditioned-positive')
 cmd('/execute if data block 2 200 12 {Items:[{Slot:0b,id:"minecraft:diamond",count:1}]}','positive-saved',1)
 cmd('/execute store result score end h036 run time query gametime','clock-end',trace=True);cmd('/scoreboard players operation end h036 -= start h036','clock-delta-operation',0);cmd('/scoreboard players get end h036','clock-delta',0)
 return ops
if __name__=='__main__':
 ref=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');ref=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h036/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(rows),labels=[o['label'] for o in rows],diagnostics=[o['label'] for o in ops if o.get('trace')],contract='Strict deterministic full records; random meat quantities/full output and full actor bodies are separate raw traces. No random quantity equality claim.'),indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Loot kill actual sheep state',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if ref:assert fresh==ref,'fresh Java bytes changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 java=[json.loads(l) for l in fresh.splitlines()];native=[json.loads(l) for l in (build/'rust.jsonl').read_bytes().splitlines()];assert len(java)==len(native)==len(rows)
 diffs=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(rows,java,native) if a!=b];(build/'strict-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,a,b in zip(rows,java,native):
  if o['admission']:assert a==b,('admission',o,a,b)
 print(f'{len(rows)-len(diffs)}/{len(rows)}');sys.exit(bool(diffs))
