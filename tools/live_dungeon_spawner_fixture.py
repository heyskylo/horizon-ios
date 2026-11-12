import json,sys,hashlib
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
def program():
 ops=[]
 def c(cmd,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=cmd,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:c(f'/scoreboard players set {k} h173 -99',label+'-reset-'+k,-99)
  c('/execute store result score result h173 store success score success h173 run '+text,label+'-call',admission=admission)
  for k in ['result','success']:c(f'/scoreboard players get {k} h173',label+'-'+k,admission=admission)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/forceload add -16 -16 31 31','force-nine',9);ops.append(dict(op='settle',required_chunks=[[x,z] for x in [-1,0,1] for z in [-1,0,1]]))
 c('/fill 0 198 0 31 199 31 stone','floor',2048);c('/tp @s 24.5 200 24.5','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[24.5d,200d,24.5d]}]','owner-body',1);c('/data get entity @s UUID','owner-uuid',4)
 c('/scoreboard objectives add h173 dummy','objective');c('/execute store result score start h173 run time query gametime','clock-start',trace=True)
 c('/fill 3 199 3 13 204 13 barrier strict','room-prefill',726);c('/fill 3 199 3 13 204 13 stone strict','room-shell',726);c('/fill 6 200 6 10 203 10 air strict','cavity',100);c('/fill 11 200 8 13 201 8 air strict','one-door',6)
 for x,y,z,block,label in [(8,199,8,'stone','floor'),(4,199,4,'stone','wide-floor'),(12,204,12,'stone','wide-ceiling'),(8,204,8,'stone','ceiling'),(8,200,8,'air','center'),(12,200,8,'air','door-low'),(12,201,8,'air','door-high'),(11,200,7,'stone','door-side'),(5,200,8,'stone','opposite-wall')]:c(f'/execute if block {x} {y} {z} {block}','room-'+label,1)
 for x,z in [(-1,-1),(-1,0),(-1,1),(0,-1),(0,0),(0,1),(1,-1),(1,0),(1,1)]:c(f'/execute if loaded {x*16} 200 {z*16}',f'loaded-{x}-{z}',1)
 c('/setblock 24 200 24 spawner{SpawnData:{entity:{id:"minecraft:zombie"}},Delay:20s} strict','direct-spawner',1);c('/setblock 24 200 26 chest strict','ordinary-chest',1);c('/setblock 26 200 26 stone strict','non-be',1)
 read('execute if data block 24 200 24 {id:"minecraft:mob_spawner",x:24,y:200,z:24,Delay:20s,SpawnData:{entity:{id:"minecraft:zombie"}}}','direct-typed',True)
 read('place feature minecraft:monster_room 8 200 8','feature-ack',True)
 c('/execute if block 8 200 8 spawner','actual-generated-spawner-block',1)
 for key in ['id','x','y','z']:read('data get block 8 200 8 '+key,'generated-'+key)
 read('execute if data block 8 200 8 {id:"minecraft:mob_spawner",x:8,y:200,z:8}','generated-typed-metadata')
 read('execute if data block 8 200 8 SpawnData.entity.id','generated-choice-present')
 c('/scoreboard players set choice h173 0','choice-zero',0)
 for mob in ['skeleton','zombie','spider']:c('/execute if data block 8 200 8 {SpawnData:{entity:{id:"minecraft:'+mob+'"}}} run scoreboard players add choice h173 1','choice-'+mob,trace=True)
 c('/scoreboard players get choice h173','choice-typed-valid-member',1,admission=False)
 c('/data get block 8 200 8 SpawnData.entity.id','actual-chosen-entity',trace=True);c('/data get block 8 200 8','generated-full',trace=True)
 read('data get block 24 200 24 SpawnData.entity.id','direct-saved-id',True);read('execute if data block 24 200 24 {Delay:20s,SpawnData:{entity:{id:"minecraft:zombie"}}}','direct-preserved',True)
 read('execute if data block 24 200 26 {id:"minecraft:chest",x:24,y:200,z:26,Items:[]}','chest-typed',True);read('data get block 26 200 26 id','non-be-negative',True)
 c('/data get block 24 200 24','direct-full',trace=True);c('/data get block 24 200 26','chest-full',trace=True)
 c('/execute if block 1 199 1 stone','unrelated-floor',1);c('/execute if block 8 204 8 stone','ceiling-preserved',1);c('/execute if block 26 200 26 stone','non-be-preserved',1)
 c('/execute store result score end h173 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h173 -= start h173','clock-delta-operation',0);c('/scoreboard players get end h173','clock-delta',0)
 return ops
if __name__=='__main__':
 build=harness_args.require_work_dir().resolve()
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 assert not build.exists();build.mkdir(parents=True);ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'scope':'actual named monster_room placement; deterministic admission/cavity; generated typed BE presence and actual chosen valid type membership strict; exact random choice/full bodies/RNG outcomes predeclared raw; no spawned mobs/chest rolls/restart claim'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Live dungeon spawner entity',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows);diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
