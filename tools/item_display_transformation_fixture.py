import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
IDENTITY='{translation:[0f,0f,0f],left_rotation:[0f,0f,0f,1f],scale:[1f,1f,1f],right_rotation:[0f,0f,0f,1f]}'
CHANGED='{translation:[1.25f,-2.5f,3f],left_rotation:[0f,1f,0f,0f],scale:[2f,3f,4f],right_rotation:[0f,0f,0f,1f]}'
CASES=[('omitted',None),('identity',IDENTITY),('nondefault',CHANGED)]
TIMES=[0]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False,expected=None):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h132 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h132 store success score success h132 run '+text,label+'-read',result=expected,admission=admission)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h132',label+'-'+k,admission=admission)
 cmd('/tick freeze','freeze')
 for g in ['spawn_mobs','advance_time','advance_weather','mob_griefing']:cmd('/gamerule '+g+' false',g)
 cmd('/gamerule random_tick_speed 0','random0');cmd('/difficulty normal','normal')
 cmd('/forceload add 0 0 31 31','forceload',4);ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
 cmd('/fill 0 199 0 31 199 31 stone','floor',1024);cmd('/fill 0 200 0 31 204 31 barrier','prefill',5120);cmd('/fill 0 200 0 31 204 31 air','air',5120)
 cmd('/tp @s 2.5 200 2.5','player-pos',1);cmd('/gamemode creative @s','player-mode')
 cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[2.5d,200d,2.5d],playerGameType:1}]','player-body',1);cmd('/data get entity @s UUID','player-uuid',4)
 cmd('/scoreboard objectives add h132 dummy','objective');cmd('/execute store result score start h132 run time query gametime','start',trace=True)
 cmd('/summon pig 20.5 200 10.5 {UUID:[I;132,0,0,99],NoAI:1b,NoGravity:1b,Health:10f}','pig',1)
 cmd('/summon marker 22.25 200.5 12.75 {UUID:[I;132,0,0,98],data:{h132:9}}','marker',1)
 for n,(name,transform) in enumerate(CASES,1):
  x=4*n+2;uid=str(uuid.UUID(int=(132<<96)|n));suffix='' if transform is None else ',transformation:'+transform
  cmd(f'/execute if loaded {x} 200 6',name+'-loaded',1);cmd(f'/execute if block {x} 199 6 stone',name+'-support',1)
  cmd(f'/summon item_display {x}.5 200 6.5 {{UUID:[I;132,0,0,{n}],Rotation:[12.5f,-20f],NoGravity:1b,CustomName:"{name}",Tags:["h132"],item:{{id:"minecraft:diamond",count:2,components:{{"minecraft:custom_data":{{h132:7}}}}}},item_display:"fixed"'+suffix+'}',name+'-summon',1)
  cmd(f'/execute if entity @e[type=item_display,nbt={{UUID:[I;132,0,0,{n}],Pos:[{x}.5d,200d,6.5d],Rotation:[12.5f,-20f],NoGravity:1b}}]',name+'-typed-body',1)
  cmd('/execute if entity '+uid,name+'-direct-uuid',1)
  for key,result in [('UUID',4),('Pos',3),('Rotation',2),('item.id',17),('item.count',2),('item.components."minecraft:custom_data".h132',7),('item_display',5)]:
   read('data get entity '+uid+' '+key,name+'-'+key,True,result)
  for key in ['transformation','transformation.translation','transformation.left_rotation','transformation.scale','transformation.right_rotation']:
   read('data get entity '+uid+' '+key,name+'-'+key)
  cmd('/data get entity '+uid,name+'-full-body',trace=True)
 cmd('/execute if entity @e[type=item_display]','display-count',3)
 cmd('/execute if entity @e[type=pig,nbt={UUID:[I;132,0,0,99],Health:10f,NoAI:1b,NoGravity:1b,Pos:[20.5d,200d,10.5d]}]','pig-control',1)
 cmd('/execute unless data entity 00000084-0000-0000-0000-000000000063 transformation','pig-no-transform',1)
 cmd('/execute if entity @e[type=marker,nbt={UUID:[I;132,0,0,98],Pos:[22.25d,200.5d,12.75d],data:{h132:9}}]','marker-control',1)
 cmd('/execute unless data entity 00000084-0000-0000-0000-000000000062 transformation','marker-no-transform',1)
 for kind in ['pig','marker']:cmd('/data get entity @e[type='+kind+',limit=1]',kind+'-full-body',trace=True)
 cmd('/execute store result score now h132 run time query gametime','end-absolute',trace=True);cmd('/scoreboard players operation now h132 -= start h132','relative-op',0);cmd('/scoreboard players get now h132','relative-clock',0)
 for op in ops:
  if op.get('label') in {name+'-transformation-read' for name,_ in CASES}:op['diagnostic']=True
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h132/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'strict_records':sum(not o.get('diagnostic',False) for o in rows),'diagnostic_labels':[o['label'] for o in rows if o.get('diagnostic',False)],'labels':[o['label'] for o in rows],'cases':CASES,'samples':TIMES,'scope':'Item-display saved omitted/identity/nondefault compound transformation and four typed Float lists strict; valid stack and identity/load/zero-clock guards strict. Full bodies, subtype defaults and durations retained raw, not whole-display parity.'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Item display saved transformation',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if reference:assert fresh==reference,'fresh reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(x) for x in fresh.splitlines()];b=[json.loads(x) for x in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows)
 diffs=[dict(label=o['label'],expected=x,actual=y,diagnostic=o.get('diagnostic',False)) for o,x,y in zip(rows,a,b) if x!=y]
 (build/'full-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 strict=[o for o in rows if not o.get('diagnostic',False)];gaps=[d for d in diffs if not d['diagnostic']]
 (build/'strict-report.json').write_text(json.dumps(dict(records=len(strict),passed=len(strict)-len(gaps),differences=gaps),indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('full',len(rows)-len(diffs),'/',len(rows),'strict',len(strict)-len(gaps),'/',len(strict));sys.exit(bool(gaps))
