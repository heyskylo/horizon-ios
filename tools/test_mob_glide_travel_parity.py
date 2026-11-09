#!/usr/bin/env python3
"""Strict actual Game/Java dry-air Husk glide travel proof (H169)."""
import sys,json,hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
import test_command_exec_parity as d
ops=[]
def cmd(s,label,expected=None,admission=True,trace=False):
 o=dict(op='cmd',cmd=s,label=label,admission=admission)
 if expected is not None:o['expect_result']=expected
 if trace:o.update(record=False,trace=True)
 ops.append(o)
cmd('/tick freeze','freeze')
for k,v in [('spawn_mobs','false'),('advance_time','false'),('random_tick_speed','0'),('advance_weather','false')]:cmd(f'/gamerule {k} {v}',k)
cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
cmd('/fill 0 199 0 15 199 15 stone','floor',256)
cmd('/tp @s 8.5 200 8.5 0 0','owner-pos',1);cmd('/gamemode creative @s','creative')
cmd('/scoreboard objectives add h169 dummy','objective')
actors=[('glide',1,0,1,4.5),('still',2,1,1,8.5),('air',3,0,0,12.5)]
for name,n,noai,glide,x in actors:
 cmd(f'/summon husk {x} 300 8.5 {{UUID:[I;169,0,0,{n}],Tags:["h169_{name}"],NoAI:{noai}b,PersistenceRequired:1b,FallFlying:{glide}b,Rotation:[0f,0f],Motion:[0d,0d,0d],equipment:{{chest:{{id:"minecraft:elytra",count:1}}}}}}',name+'-summon',1)
cmd('/execute store result score start h169 run time query gametime','start',trace=True)
for tick in [0,1,2]:
 if tick:cmd('/tick step 1',f'{tick}-step');ops.append(dict(op='tick',n=1))
 cmd('/execute store result score now h169 run time query gametime',f'{tick}-raw-clock',trace=True)
 cmd('/scoreboard players operation now h169 -= start h169',f'{tick}-clock-delta',tick)
 cmd('/execute if loaded 8 300 8',f'{tick}-load',1)
 cmd('/execute if block 8 199 8 stone',f'{tick}-floor',1)
 cmd('/execute if entity @s[type=player,name=Player]',f'{tick}-owner',1)
 for name,n,noai,glide,x in actors:
  u=f'000000a9-0000-0000-0000-{n:012}';label=f'{tick}-{name}'
  cmd(f'/execute as {u} if entity @s[type=husk,tag=h169_{name}]',label+'-type',1)
  for key,val in [('UUID',4),('Health',20),('equipment.chest.count',1)]:cmd(f'/data get entity {u} {key}',label+'-'+key,val)
  cmd(f'/execute if data entity {u} equipment.chest{{id:"minecraft:elytra",count:1}}',label+'-gear',1)
  cmd(f'/execute if data entity {u} {{FallFlying:{glide}b}}',label+'-flag',1)
  if noai:cmd(f'/execute if data entity {u} {{NoAI:1b,Pos:[{x}d,300d,8.5d],Motion:[0d,0d,0d]}}',label+'-stationary',1)
  for field in ['Pos','Motion']:
   cmd(f'/data get entity {u} {field}',label+'-'+field,3,admission=(tick==0 or noai==1))
  # Every projection has independently fresh callback/result/success score stores.
  cmd('/scoreboard players set result h169 -77',label+'-result-reset',-77)
  cmd('/scoreboard players set success h169 -88',label+'-success-reset',-88)
  cmd(f'/execute store result score result h169 store success score success h169 run data get entity {u} Pos',label+'-stored-pos',3,admission=(tick==0 or noai==1))
  cmd('/scoreboard players get result h169',label+'-result',3)
  cmd('/scoreboard players get success h169',label+'-success',1)
  cmd(f'/data get entity {u}',label+'-body',trace=True)
if __name__=='__main__':
 if '--work-dir' not in sys.argv:
  sys.argv += ['--work-dir', str(d.lab.LOCAL / 'mob-glide-travel-parity')]
 # Current driver reads an undeclared unrelated diagnostic flag; local default only.
 original_parse=d.argparse.ArgumentParser.parse_args
 def parse_with_default(self,*a,**kw):
  self.set_defaults(seed_sync_gaps_diagnostic=False)
  return original_parse(self,*a,**kw)
 d.argparse.ArgumentParser.parse_args=parse_with_default
 d.corpus_module.groups=lambda:[('h169 actual glide travel',ops)]
 sys.argv.append('--allow-gaps');d.main()
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]);recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 java=[json.loads(x) for x in (build/'vanilla.jsonl').read_text().splitlines()]
 assert len(java)==len(recorded)
 for op,row in zip(recorded,java):
  if 'expect_result' in op:assert row['results']==[[True,op['expect_result']]],(op,row)
 if '--reference-only' not in sys.argv:
  native=[json.loads(x) for x in (build/'rust.jsonl').read_text().splitlines()];assert len(native)==len(java)
  diffs=[]
  for o,a,b in zip(recorded,java,native):
   if o['admission']:assert a==b,('native admission',o,a,b)
   if a!=b:diffs.append(dict(label=o['label'],expected=a,actual=b))
  report=dict(total=len(java),passed=len(java)-len(diffs),differences=diffs)
  (build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n');print(report)
  if diffs:raise SystemExit(1)
