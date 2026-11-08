import json,hashlib,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
CASES=[('absent',None,1),('zero',0,1),('past-negative',-5,1),('past-positive',1,1),('future',999999,1),('ai-control',999999,0)]
def program():
 ops=[]
 def c(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:c(f'/scoreboard players set {k} h262 -99',label+'-reset-'+k,-99)
  c('/execute store result score result h262 store success score success h262 run '+text,label+'-call',admission=admission)
  for k in ['result','success']:c(f'/scoreboard players get {k} h262',label+'-'+k,admission=admission)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/gamerule random_tick_speed 0','random0')
 c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 c('/fill 0 199 0 15 199 15 stone','floor',256);c('/fill 0 200 0 15 204 15 barrier','prefill',1280);c('/fill 0 200 0 15 204 15 air','air',1280)
 c('/tp @s 14.5 200 14.5','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/execute if entity @s[type=player,nbt={Health:20f,Pos:[14.5d,200d,14.5d],playerGameType:1}]','owner-body',1);c('/data get entity @s UUID','owner-uuid',4)
 c('/scoreboard objectives add h262 dummy','objective');c('/execute store result score start h262 run time query gametime','clock-start',trace=True)
 for n,(name,end,noai) in enumerate(CASES,1):
  x=1+2*n;u=str(uuid.UUID(int=(262<<96)|n));extra='' if end is None else f',anger_end_time:{end}L'
  c(f'/summon enderman {x}.5 200 4.5 '+'{'+f'UUID:[I;262,0,0,{n}],Health:40f,NoAI:{noai}b,NoGravity:1b,Motion:[0d,0d,0d],Tags:[h262],CustomName:"{name}"'+extra+'}',name+'-summon',1)
  c(f'/execute as {u} if entity @s[type=enderman,nbt='+'{'+f'UUID:[I;262,0,0,{n}],Health:40f,Pos:[{x}.5d,200d,4.5d],Motion:[0d,0d,0d],NoGravity:1b'+(',NoAI:1b' if noai else '')+'}]',name+'-identity',1)
  c(f'/execute unless data entity {u} angry_at',name+'-no-target',1)
  if not noai:c(f'/execute unless data entity {u} NoAI',name+'-declared-ai',1)
 last=0
 for t in [0,1,2]:
  if t:c('/tick step 1',f'tick-{t}',admission=False);ops.append(dict(op='tick',n=1))
  c('/execute store result score now h262 run time query gametime',f't{t}-absolute-clock',trace=True);c('/scoreboard players operation now h262 -= start h262',f't{t}-clock-op',t);c('/scoreboard players get now h262',f't{t}-clock',t)
  for n,(name,end,noai) in enumerate(CASES,1):
   x=1+2*n;u=str(uuid.UUID(int=(262<<96)|n));label=f'{name}-t{t}'
   c(f'/execute as {u} if entity @s[type=enderman,nbt='+'{'+f'UUID:[I;262,0,0,{n}],Health:40f,NoGravity:1b'+(f',NoAI:1b,Pos:[{x}.5d,200d,4.5d],Motion:[0d,0d,0d]' if noai else '')+'}]',label+'-identity',1)
   c(f'/execute if loaded {x} 200 4',label+'-loaded',1);c(f'/execute if block {x} 199 4 stone',label+'-floor',1)
   read(f'data get entity {u} anger_end_time',label+'-saved-end')
   read(f'data get entity {u} anger_end_time',label+'-repeated-end')
   c(f'/execute unless data entity {u} angry_at',label+'-no-target',1)
   c(f'/data get entity {u}',label+'-full-body',trace=True)
  last=t
 u=str(uuid.UUID(int=(262<<96)|5))
 read(f'data modify entity {u} anger_end_time set value 777777L','live-write',True)
 read(f'data get entity {u} anger_end_time','live-write-saved')
 read(f'data modify entity {u} anger_end_time set value 777777L','unchanged-live-write')
 c('/execute store result score end h262 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h262 -= start h262','clock-delta-op',2);c('/scoreboard players get end h262','clock-delta',2)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists();build.mkdir(parents=True);ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'scope':'Enderman absolute anger_end_time, actual loaded actors and relative clocks0/1/2; no aggression or other neutral species repair'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Enderman absolute anger end',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows);diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
