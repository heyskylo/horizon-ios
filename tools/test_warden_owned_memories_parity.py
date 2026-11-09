import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('ai-finite',0,'{memories:{"minecraft:is_emerging":{value:{},ttl:2L},"minecraft:dig_cooldown":{value:{},ttl:1200L}}}'),('noai-finite',1,'{memories:{"minecraft:is_emerging":{value:{},ttl:2L},"minecraft:dig_cooldown":{value:{},ttl:1200L},"minecraft:sniff_cooldown":{value:{},ttl:17L}}}'),('ai-absent',0,None),('noai-empty',1,'{memories:{}}'),('noai-permanent',1,'{memories:{"minecraft:is_emerging":{value:{}},"minecraft:dig_cooldown":{value:{}}}}')]
TIMES=[0,1,2,3,5]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(uid,key,label):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h029 -99',label+'-reset-'+k,-99)
  cmd(f'/execute store result score result h029 store success score success h029 run data get entity {uid} {key}',label+'-read',admission=False)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h029',label+'-'+k,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight');cmd('/gamerule random_tick_speed 0','no-random');cmd('/difficulty normal','normal');cmd('/difficulty','actual-difficulty',2)
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/fill 0 200 0 15 204 15 barrier','clear-prefill',1280);cmd('/fill 0 200 0 15 204 15 air','clear',1280)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode');cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1);cmd('/data get entity @s UUID','owner-uuid',4)
 for n,(label,noai,brain) in enumerate(CASES,1):
  x=2+5*((n-1)%3);z=2+7*((n-1)//3)
  cmd(f'/fill {x-1} 200 {z-1} {x+1} 203 {z+1} stone',label+'-cell-solid',36);cmd(f'/fill {x} 200 {z} {x} 203 {z} air',label+'-cell-air',4)
 cmd('/scoreboard objectives add h029 dummy','objective');cmd('/execute store result score start h029 run time query gametime','clock-start',trace=True)
 for n,(label,noai,brain) in enumerate(CASES,1):
  x=2+5*((n-1)%3);z=2+7*((n-1)//3);extra=',Brain:'+brain if brain else ''
  cmd(f'/summon warden {x}.5 200 {z}.5 '+'{'+f'UUID:[I;29,0,0,{n}],Health:500f,NoAI:{noai}b,NoGravity:1b,PersistenceRequired:0b,Motion:[0d,0d,0d],Tags:[h029]'+extra+'}',label+'-summon',1)
 last=0
 for t in TIMES:
  if t:
   cmd(f'/tick step {t-last}',f'tick-{t}',admission=False);ops.append(dict(op='tick',n=t-last))
  cmd('/execute store result score now h029 run time query gametime',f't{t}-absolute',trace=True);cmd('/scoreboard players operation now h029 -= start h029',f't{t}-delta-op',t);cmd('/scoreboard players get now h029',f't{t}-delta',t)
  for n,(label,noai,brain) in enumerate(CASES,1):
   p=f'{label}-t{t}';x=2+5*((n-1)%3);z=2+7*((n-1)//3);uid=str(uuid.UUID(int=(29<<96)|n));ai=',NoAI:1b' if noai else ''
   cmd(f'/execute as {uid} if entity @s[type=warden,nbt='+'{'+f'UUID:[I;29,0,0,{n}],Health:500f,NoGravity:1b,Pos:[{x}.5d,200d,{z}.5d]'+ai+'}]',p+'-actor',1)
   cmd(f'/execute if loaded {x} 200 {z}',p+'-loaded',1);cmd(f'/execute if block {x} 199 {z} stone',p+'-floor',1);cmd(f'/execute if block {x} 200 {z} air',p+'-feet-air',1);cmd(f'/execute if block {x+1} 201 {z} stone',p+'-wall',1)
   if not noai:cmd(f'/execute unless data entity {uid} NoAI',p+'-ai-enabled',1)
   for key,v in [('UUID',4),('Health',500),('Pos',3),('NoGravity',1)]:cmd(f'/data get entity {uid} {key}',p+'-'+key,v)
   cmd(f'/data get entity {uid} Motion',p+'-motion',trace=True);cmd(f'/data get entity {uid}',p+'-full-body',trace=True);cmd(f'/data get entity {uid} Brain',p+'-full-brain',trace=True);cmd(f'/data get entity {uid} Pose',p+'-pose',trace=True)
   for memory in ['is_emerging','dig_cooldown']:
    path=f'Brain.memories."minecraft:{memory}"';cmd(f'/data get entity {uid} {path}',p+'-'+memory+'-entry-diagnostic',trace=True);read(uid,path+'.value',p+'-'+memory+'-unit');read(uid,path+'.ttl',p+'-'+memory+'-ttl');cmd(f'/execute if data entity {uid} '+path,p+'-'+memory+'-presence',admission=False)
   if label=='noai-finite':
    cmd(f'/data get entity {uid} Brain.memories."minecraft:sniff_cooldown"',p+'-unrelated-full',trace=True);cmd(f'/data get entity {uid} Brain.memories."minecraft:sniff_cooldown".value',p+'-unrelated-unit',0);cmd(f'/execute if data entity {uid} '+'{Brain:{memories:{"minecraft:sniff_cooldown":{value:{},ttl:17L}}}}',p+'-unrelated-type',1)
  last=t
 return ops
if __name__=='__main__':
 ref=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');ref=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h029/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(rows),labels=[o['label'] for o in rows],cases=CASES,times=TIMES,contract='Owned saved Unit values/Long TTL/presence complete records strict; whole entries/full body/Brain/pose/motion and absolute clocks predeclared raw diagnostics, no compound-order/pose/height or all-Brain equality claim.'),indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Warden loaded owned memories',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if ref:assert fresh==ref,'fresh Java reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 java=[json.loads(l) for l in fresh.splitlines()];native=[json.loads(l) for l in (build/'rust.jsonl').read_bytes().splitlines()];assert len(java)==len(native)==len(rows)
 diffs=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(rows,java,native) if a!=b];(build/'strict-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,a,b in zip(rows,java,native):
  if o['admission']:assert a==b,('admission',o,a,b)
 print(f'{len(rows)-len(diffs)}/{len(rows)}');sys.exit(bool(diffs))
