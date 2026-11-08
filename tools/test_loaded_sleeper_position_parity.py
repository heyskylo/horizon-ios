#!/usr/bin/env python3
"""Loaded villager first-tick sleep position, normal sleep and awake controls."""
import json,sys,hashlib,tempfile
from pathlib import Path
import test_command_exec_parity as driver
ACTORS=[dict(name='loaded_north',n=1,x=2.5,z=6.5,bed=[2,200,6],facing='north',noai=True),dict(name='loaded_south',n=2,x=6.5,z=7.5,bed=[6,200,7],facing='south',noai=True),dict(name='awake',n=3,x=12.5,z=12.5,bed=None,noai=True),dict(name='normal',n=4,x=10.5,z=6.5,bed=[10,200,6],facing='north',noai=False)]
def uuid(n):return f'000000e9-0000-0000-0000-{n:012x}'
def program(full=False,wake=False):
 ops=[]
 def c(cmd,label,result=None,admission=True,raw=False,java_results=None):
  o=dict(op='cmd',cmd=cmd,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if raw:o.update(record=full,trace=True,admission=False)
  if java_results is not None:o['java_results']=java_results
  ops.append(o)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/gamerule random_tick_speed 0','no-random-ticks');c('/time set midnight','night',18000);c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]));c('/fill 0 199 0 15 199 15 stone','floor',256);c('/tp @s 14.5 200 14.5 0 0','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/data get entity @s UUID','owner-uuid',4);c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner-body',1);c('/execute if loaded 2 200 6','loaded',1)
 for a in [ACTORS[0],ACTORS[1],ACTORS[3]]:
  x,y,z=a['bed'];foot=z+1 if a['facing']=='north' else z-1;occupied='false' if a['name']=='normal' else 'true'
  for part,pz in [('foot',foot),('head',z)]:c(f'/setblock {x} {y} {pz} red_bed[facing={a["facing"]},part={part},occupied={occupied}] strict',a['name']+'-'+part+'-place',1)
  c(f'/execute if block {x} 199 {z} stone',a['name']+'-support',1)
 normal=ACTORS[3];c('/summon villager 10.5 200.5625 6.5 {UUID:[I;233,0,0,4],Tags:[h233_normal],Health:20f,PersistenceRequired:1b,Motion:[0d,0d,0d],Brain:{memories:{"minecraft:home":{value:{dimension:"minecraft:overworld",pos:[I;10,200,6]}}}}}','normal-summon',1);c('/tick step 20','normal-setup-step');ops.append(dict(op='tick',n=20))
 c('/execute as '+uuid(4)+' if entity @s[type=villager,tag=h233_normal,nbt={Health:20f,sleeping_pos:[I;10,200,6],Pos:[10.5d,200.5625d,6.5d]}]','normal-start-admission',1);c('/execute if block 10 200 6 red_bed[facing=north,part=head,occupied=true]','normal-bed-admission',1)
 for a in ACTORS[:3]:
  y=200.5625 if a['bed'] else 200.0;sleep='' if not a['bed'] else ',sleeping_pos:[I;'+','.join(map(str,a['bed']))+']'
  c(f'/summon villager {a["x"]} {y} {a["z"]} {{UUID:[I;233,0,0,{a["n"]}],Tags:[h233_{a["name"]}],Health:20f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Motion:[0d,0d,0d]{sleep}}}',a['name']+'-summon',1)
 c('/scoreboard objectives add h233 dummy','objective');c('/execute store result score start h233 run time query gametime','clock-start',raw=True)
 for tick in [0,1,2]:
  if tick:c('/tick step 1',f't{tick}-step');ops.append(dict(op='tick',n=1))
  c('/execute store result score now h233 run time query gametime',f't{tick}-clock-raw',raw=True);c('/scoreboard players operation now h233 -= start h233',f't{tick}-clock-delta',tick)
  for a in ACTORS:
   name=f't{tick}-{a["name"]}';target=uuid(a['n']);tag='h233_'+a['name'];extra=',NoAI:1b,NoGravity:1b' if a['noai'] else ''
   c(f'/execute as {target} if entity @s[type=villager,tag={tag},nbt={{Health:20f,PersistenceRequired:1b{extra}}}]',name+'-body',1);c(f'/data get entity {target} UUID',name+'-uuid',4);c(f'/data get entity {target} Motion',name+'-motion',3)
   for k in ['result','success']:c(f'/scoreboard players set {k} h233 -99',name+'-reset-'+k,-99)
   c(f'/execute store result score result h233 store success score success h233 run data get entity {target} Pos',name+'-pos',admission=not(a['name'].startswith('loaded') and tick>0),java_results=[[True,3]])
   c('/scoreboard players get result h233',name+'-result',3);c('/scoreboard players get success h233',name+'-success',1)
   y=200.0 if not a['bed'] else 200.5625 if a['name'].startswith('loaded') and tick==0 else 200.5625 if a['name']=='normal' else 200.6875
   motion='[0d,-0.0784000015258789d,0d]' if a['name']=='normal' else '[0d,0d,0d]'
   c(f'/execute as {target} if entity @s[nbt={{Pos:[{a["x"]}d,{y}d,{a["z"]}d],Motion:{motion}}}]',name+'-position',1,admission=not(a['name'].startswith('loaded') and tick>0))
   if a['bed']:
    c(f'/data get entity {target} sleeping_pos',name+'-sleep-pos',3)
    x,by,z=a['bed'];foot=z+1 if a['facing']=='north' else z-1
    for part,pz in [('head',z),('foot',foot)]:c(f'/execute if block {x} {by} {pz} red_bed[facing={a["facing"]},part={part},occupied=true]',name+'-bed-'+part,1)
   else:c(f'/execute unless data entity {target} sleeping_pos',name+'-awake',1)
   c(f'/data get entity {target}',name+'-full-body',raw=True)
 if wake:
  c('/setblock 2 200 13 red_bed[facing=north,part=foot,occupied=true] strict','wake-foot',1);c('/setblock 2 200 12 red_bed[facing=north,part=head,occupied=true] strict','wake-head',1)
  c('/summon villager 2.5 200.5625 12.5 {UUID:[I;233,0,0,5],Tags:[h233_wake],Health:20f,NoAI:1b,NoGravity:1b,sleeping_pos:[I;2,200,12]}','wake-summon',1);c('/data get entity '+uuid(5),'wake-before',admission=False);c('/setblock 2 200 12 air strict','wake-remove-head',1);c('/setblock 2 200 13 air strict','wake-remove-foot',1);c('/tick step 1','wake-step');ops.append(dict(op='tick',n=1));c('/execute as '+uuid(5)+' if entity @s[type=villager,tag=h233_wake,nbt={Health:20f,NoAI:1b,NoGravity:1b}]','wake-alive',1);c('/data get entity '+uuid(5)+' Pos','wake-pos',admission=False);c('/data get entity '+uuid(5)+' sleeping_pos','wake-sleep-pos',admission=False);c('/data get entity '+uuid(5),'wake-after',admission=False)
 return ops
if __name__=='__main__':
 full='--full-diagnostic' in sys.argv;wake='--wake-diagnostic' in sys.argv
 for flag in ['--full-diagnostic','--wake-diagnostic']:
  if flag in sys.argv:sys.argv.remove(flag)
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).resolve();del sys.argv[i:i+2]
 if '--work-dir' not in sys.argv:
  parent=Path(tempfile.mkdtemp(prefix='h233-',dir=driver.lab.LOCAL));sys.argv+=['--work-dir',str(parent/'run')]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists(),'preserve attempts';build.mkdir(parents=True)
 ops=program(full,wake);recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)];(build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(recorded),full=full,wake=wake,labels=[o['label'] for o in recorded]),indent=2)+'\n');driver.corpus_module.groups=lambda:[('Loaded sleeper first tick',ops)];expected=reference.read_bytes() if reference else None;sys.argv.append('--allow-gaps');driver.main();jb=(build/'vanilla.jsonl').read_bytes()
 if expected is not None:assert jb==expected and reference.read_bytes()==expected,'reference bytes changed'
 java=[json.loads(s) for s in jb.splitlines()];assert len(java)==len(recorded)
 for o,row in zip(recorded,java):
  if 'java_results' in o:assert row['results']==o['java_results'],('Java admission',o,row)
 if '--reference-only' not in sys.argv:
  native=[json.loads(s) for s in (build/'rust.jsonl').read_text().splitlines()];assert len(native)==len(recorded);res=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(recorded,java,native) if a!=b];r=dict(records=len(recorded),passed=len(recorded)-len(res),residuals=res,full=full,wake=wake,reference_sha256=hashlib.sha256(jb).hexdigest());(build/'strict-report.json').write_text(json.dumps(r,indent=2)+'\n')
  if not full and not wake:
   for o,a,b in zip(recorded,java,native):
    if o['admission']:assert a==b,('admission',o,a,b)
  print(f'Strict records: {r["passed"]}/{r["records"]}')
  if res:raise SystemExit(1)
