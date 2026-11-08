import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('absent',None),('zero','0b'),('one','1'),('byte127','127b'),('byte128','-128b'),('byte255','-1b'),('int256','256'),('intminus','-1'),('longexact','9007199254740993L'),('longminus','-4294967297L'),('floatpositive','1.75f'),('floatnegativehalf','-0.5f'),('doublenegative','-1.75d'),('doublewrap','256.75d'),('positiveoverflow','1e20d'),('negativeoverflow','-1e20d')]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False,expected=None):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h158 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h158 store success score success h158 run '+text,label+'-read',result=expected,admission=admission)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h158',label+'-'+k,admission=admission)
 cmd('/tick freeze','freeze')
 for g in ['spawn_mobs','advance_time','advance_weather','mob_griefing']:cmd('/gamerule '+g+' false',g)
 cmd('/gamerule random_tick_speed 0','random0');cmd('/difficulty normal','normal')
 cmd('/forceload add 0 0 31 31','forceload',4);ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
 cmd('/fill 0 199 0 31 199 31 stone','floor',1024);cmd('/fill 0 200 0 31 204 31 barrier','prefill',5120);cmd('/fill 0 200 0 31 204 31 air','air',5120)
 cmd('/tp @s 2.5 200 2.5','player-pos',1);cmd('/gamemode creative @s','player-mode')
 cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[2.5d,200d,2.5d],playerGameType:1}]','player-body',1);cmd('/data get entity @s UUID','player-uuid',4)
 cmd('/scoreboard objectives add h158 dummy','objective');cmd('/execute store result score start h158 run time query gametime','start',trace=True)
 for n,(name,amp) in enumerate(CASES,1):
  x=6+(n-1)%4*4;z=6+(n-1)//4*4;uid=str(uuid.UUID(int=(158<<96)|n));suffix='' if amp is None else ',amplifier:'+amp
  cmd(f'/execute if loaded {x} 200 {z}',name+'-loaded',1);cmd(f'/execute if block {x} 199 {z} stone',name+'-support',1)
  cmd(f'/summon pig {x}.5 200 {z}.5 {{UUID:[I;158,0,0,{n}],NoAI:1b,NoGravity:1b,Health:10f,CustomName:"{name}",Tags:["h158"],active_effects:[{{id:"minecraft:night_vision",duration:200,ambient:1b,show_particles:0b,show_icon:0b'+suffix+'}]}',name+'-summon',1)
  cmd(f'/execute if entity @e[type=pig,nbt={{UUID:[I;158,0,0,{n}],Pos:[{x}.5d,200d,{z}.5d],Health:10f,NoAI:1b,NoGravity:1b}}]',name+'-typed-body',1)
  cmd('/execute if entity '+uid,name+'-direct-uuid',1)
  cmd('/execute if data entity '+uid+' active_effects[{id:"minecraft:night_vision",duration:200,ambient:1b,show_particles:0b,show_icon:0b}]',name+'-effect-admission',1)
  for key,result in [('UUID',4),('Pos',3),('Health',10),('active_effects[0].duration',200)]:read('data get entity '+uid+' '+key,name+'-'+key,True,result)
  for key in ['active_effects[0].amplifier','active_effects[0].ambient','active_effects[0].show_particles','active_effects[0].show_icon']:
   read('data get entity '+uid+' '+key,name+'-'+key)
  read('execute if data entity '+uid+' active_effects[0].amplifier',name+'-amplifier-presence')
  cmd('/data get entity '+uid+' active_effects',name+'-full-effects',trace=True)
  cmd('/data get entity '+uid,name+'-full-body',trace=True)
 cmd('/summon cow 24.5 200 24.5 {UUID:[I;158,0,0,99],NoAI:1b,NoGravity:1b,Health:10f}','cow',1)
 cmd('/execute if entity @e[type=cow,nbt={UUID:[I;158,0,0,99],Health:10f,NoAI:1b,NoGravity:1b,Pos:[24.5d,200d,24.5d]}]','cow-body',1)
 cmd('/execute unless data entity 0000009e-0000-0000-0000-000000000063 active_effects','cow-no-effects',1)
 cmd('/data get entity 0000009e-0000-0000-0000-000000000063','cow-full-body',trace=True)
 cmd('/execute if entity @e[type=pig,tag=h158]','pig-count',16)
 cmd('/execute store result score now h158 run time query gametime','end-absolute',trace=True);cmd('/scoreboard players operation now h158 -= start h158','relative-op',0);cmd('/scoreboard players get now h158','relative-clock',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h158/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'strict_records':sum(not o.get('diagnostic',False) for o in rows),'diagnostic_labels':[o['label'] for o in rows if o.get('diagnostic',False)],'labels':[o['label'] for o in rows],'cases':CASES,'samples':[0],'scope':'Mob amplifier boxed byte codec, actual identity/effect/duration/flags/load/zero-clock strict; complete effects and bodies retained raw diagnostics.'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Mob amplifier codec',ops)];sys.argv.append('--allow-gaps');driver.main()
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
