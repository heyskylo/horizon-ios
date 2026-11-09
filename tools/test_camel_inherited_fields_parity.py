import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
FIELDS=['Tame','Temper','Bred','EatingHaystack']
CASES=[
 ('camel-absent','camel',None,(1,0,0,0)),
 ('camel-default','camel','Tame:0b,Temper:0,Bred:0b,EatingHaystack:0b',(1,0,0,0)),
 ('camel-positive','camel','Tame:0b,Temper:50,Bred:1b,EatingHaystack:1b',(1,50,1,1)),
 ('camel-false','camel','Tame:1b,Temper:12,Bred:0b,EatingHaystack:0b',(1,12,0,0)),
 ('husk-absent','camel_husk',None,(1,0,0,0)),
 ('husk-positive','camel_husk','Tame:0b,Temper:50,Bred:1b,EatingHaystack:1b',(1,50,1,1)),
 ('horse','horse','Tame:1b,Temper:12,Bred:1b,EatingHaystack:1b',(1,12,1,1)),
 ('llama','llama','Tame:1b,Temper:12,Bred:1b,EatingHaystack:1b',(1,12,1,1)),
]
DIAGNOSTICS=[
 ('negative','Temper:-1',-1),('large','Temper:256',256),('max-int','Temper:2147483647',2147483647),
 ('fraction','Temper:-1.5f',-2),('long-wrap','Temper:4294967297L',1),('nonnumeric','Temper:"bad"',0),
]
def program(full=False):
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if trace:op.update(record=False,trace=True)
  ops.append(op)
 def read(uid,key,label):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h092 -99',label+'-reset-'+k,-99)
  cmd(f'/execute store result score result h092 store success score success h092 run data get entity {uid} {key}',label+'-read',admission=False)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h092',label+'-'+k,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight');cmd('/gamerule random_tick_speed 0','no-random')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/fill 0 200 0 15 205 15 barrier','clear-prefill',1536);cmd('/fill 0 200 0 15 205 15 air','clear',1536)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode');cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/scoreboard objectives add h092 dummy','objective')
 cmd('/execute store result score prep h092 run time query gametime','clock-before-prepare',trace=True)
 cmd('/execute if score prep h092 matches 0..1023','clock-prepare-admission',1)
 for step in [1024,512,256,128,64,32,16,8,4,2,1]:
  cmd('/execute store result score prep h092 run time query gametime',f'prepare-{step}-clock',trace=True)
  cmd(f'/execute if score prep h092 matches ..{1024-step} run tick step {step}',f'prepare-{step}-step',trace=True)
  ops.append(dict(op='tick',n=step))
 cmd('/time query gametime','clock-prepared',1024)
 cmd('/execute store result score start h092 run time query gametime','clock-start',trace=True)
 cases=CASES+([(label,'camel',fields,(1,temper,0,0)) for label,fields,temper in DIAGNOSTICS] if full else [])
 for n,(label,species,fields,expected) in enumerate(cases,1):
  x=2.5+3*((n-1)%4);z=2.5+3*((n-1)//4);uid=str(uuid.UUID(int=(92<<96)|n));is_camel=species in ['camel','camel_husk']
  extra=(',LastPoseTick:1L' if is_camel else '')+(','+fields if fields else '')
  tag=f'{{UUID:[I;92,0,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],Age:0,Tags:[keep],CustomName:"h092_{n}"'+extra+'}'
  cmd(f'/summon {species} {x} 200 {z} '+tag,label+'-summon',1)
  actor=f'{{UUID:[I;92,0,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Pos:[{x}d,200d,{z}d],Motion:[0d,0d,0d]}}'
  cmd(f'/execute as {uid} if entity @s[type={species},nbt='+actor+']',label+'-actor',1)
  cmd(f'/execute if loaded {int(x)} 200 {int(z)}',label+'-loaded',1);cmd(f'/execute if block {int(x)} 199 {int(z)} stone',label+'-floor',1);cmd(f'/execute if block {int(x)} 200 {int(z)} air',label+'-air',1)
  for k,nr in [('UUID',4),('Health',10),('Pos',3),('Motion',3),('NoAI',1),('NoGravity',1)]:cmd(f'/data get entity {uid} {k}',label+'-'+k,nr)
  if is_camel:
   cmd(f'/data get entity {uid} LastPoseTick',label+'-pose-tick',1)
   cmd(f'/execute if data entity {uid} '+ '{LastPoseTick:1L}',label+'-pose-type',1)
  cmd(f'/data get entity {uid} Pose',label+'-pose-body-field',trace=True)
  cmd(f'/data get entity {uid}',label+'-full-body',trace=True)
  cmd(f'/data get entity {uid} CustomName',label+'-name');cmd(f'/data get entity {uid} Tags',label+'-tags',1)
  for k,v in zip(FIELDS,expected):
   read(uid,k,label+'-'+k)
   cmd(f'/execute if data entity {uid} {k}',label+'-'+k+'-presence',1,False)
   value=str(v)+('b' if k!='Temper' else '')
   cmd(f'/execute if data entity {uid} '+'{'+k+':'+value+'}',label+'-'+k+'-type',1,False)
 cmd('/execute store result score end h092 run time query gametime','clock-end',trace=True);cmd('/scoreboard players operation end h092 -= start h092','clock-delta-operation',0);cmd('/scoreboard players get end h092','clock-delta',0)
 return ops
if __name__=='__main__':
 full='--full-diagnostic' in sys.argv
 if full:sys.argv.remove('--full-diagnostic')
 ref=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');ref=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h092/capture')).resolve();ops=program(full);recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 driver.corpus_module.groups=lambda:[('Camel inherited saved fields',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes();java=[json.loads(l) for l in fresh.splitlines()];assert len(java)==len(recorded)
 if ref:assert ref==fresh,'fresh Java reference changed'
 if '--reference-only' in sys.argv:print('Java records',len(java));sys.exit(0)
 native=[json.loads(l) for l in (build/'rust.jsonl').read_bytes().splitlines()];assert len(native)==len(java)
 diffs=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(recorded,java,native) if a!=b]
 (build/'strict-report.json').write_text(json.dumps(dict(records=len(java),passed=len(java)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,a,b in zip(recorded,java,native):
  if o['admission']:assert a==b,('admission',o,a,b)
 print(f'{len(java)-len(diffs)}/{len(java)}');sys.exit(bool(diffs))
