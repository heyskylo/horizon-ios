import json,hashlib,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('honey-husk','husk',4,202.0,4,1),('no-push','husk',4,202.0,12,1),('honey-stand','armor_stand',12,202.0,4,1),('vertical-husk','husk',12,202.0,12,0)]
PISTONS=[('honey-husk',3,201,4,'east',5,201,4),('honey-stand',11,201,4,'east',13,201,4),('vertical-husk',12,200,12,'up',12,202,12)]
def program():
 ops=[]
 def c(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:c(f'/scoreboard players set {k} h255 -99',label+'-reset-'+k,-99)
  c('/execute store result score result h255 store success score success h255 run '+text,label+'-call',admission=admission)
  for k in ['result','success']:c(f'/scoreboard players get {k} h255',label+'-'+k,admission=admission)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/gamerule random_tick_speed 0','random0')
 c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 c('/fill 0 199 0 15 199 15 stone','floor',256);c('/fill 0 200 0 15 205 15 barrier','prefill',1536);c('/fill 0 200 0 15 205 15 air','air',1536)
 c('/tp @s 1.5 200 1.5','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/execute if entity @s[type=player,nbt={Health:20f,Pos:[1.5d,200d,1.5d],playerGameType:1}]','owner-body',1);c('/data get entity @s UUID','owner-uuid',4)
 c('/scoreboard objectives add h255 dummy','objective');c('/execute store result score start h255 run time query gametime','clock-start',trace=True)
 for name,x,y,z,face,dx,dy,dz in PISTONS:c(f'/setblock {x} {y} {z} piston[facing={face}]',name+'-piston-place',1)
 for name,kind,x,y,z,ground in CASES:
  sy=201;support='stone' if name=='vertical-husk' else 'honey_block';c(f'/setblock {x} {sy} {z} {support}',name+'-support',1);c(f'/execute if block {x} {sy} {z} {support}',name+'-support-admitted',1)
 for n,(name,kind,x,y,z,ground) in enumerate(CASES,1):
  u=str(uuid.UUID(int=(255<<96)|n));noai=',NoAI:1b' if kind=='husk' else ''
  c(f'/summon {kind} {x}.5 {y} {z}.5 '+'{'+f'UUID:[I;255,0,0,{n}],Health:10f,NoGravity:1b,OnGround:{ground}b,Motion:[0d,0d,0d],Tags:[h255],CustomName:"{name}"'+noai+'}',name+'-summon',1)
  c(f'/execute as {u} if entity @s[type={kind},nbt='+'{'+f'UUID:[I;255,0,0,{n}],Health:10f,Pos:[{x}.5d,{y}d,{z}.5d],Motion:[0d,0d,0d],OnGround:{ground}b,NoGravity:1b'+noai+'}]',name+'-initial-body',1)
 for t in [0,1,2,3]:
  if t==1:
   for name,x,y,z in [('honey-husk',3,201,5),('honey-stand',11,201,5),('vertical-husk',13,200,12)]:c(f'/setblock {x} {y} {z} redstone_block',name+'-power',1)
  if t:c('/tick step 1',f'tick-{t}',admission=False);ops.append(dict(op='tick',n=1))
  c('/execute store result score now h255 run time query gametime',f't{t}-absolute-clock',trace=True);c('/scoreboard players operation now h255 -= start h255',f't{t}-clock-op',t);c('/scoreboard players get now h255',f't{t}-clock',t)
  for n,(name,kind,x,y,z,ground) in enumerate(CASES,1):
   u=str(uuid.UUID(int=(255<<96)|n));label=f'{name}-t{t}';noai=',NoAI:1b' if kind=='husk' else ''
   c(f'/execute as {u} if entity @s[type={kind},nbt='+'{'+f'UUID:[I;255,0,0,{n}],Health:10f,NoGravity:1b'+noai+'}]',label+'-identity',1)
   c(f'/execute if loaded {x} 200 {z}',label+'-loaded',1);c(f'/execute if block {x} 199 {z} stone',label+'-floor',1)
   for field in ['Pos','Motion','OnGround']:read(f'data get entity {u} '+field,label+'-'+field)
   if name=='honey-husk':
    px='4.5' if t==0 else '5.0';c(f'/execute if data entity {u} '+'{Pos:['+px+'d,202.0d,4.5d]}',label+'-physical-expected',1,False)
   c(f'/data get entity {u}',label+'-full-body',trace=True)
  if t:
   for name,x,y,z,face,dx,dy,dz in PISTONS:
    label=f'{name}-t{t}';c(f'/execute if block {x} {y} {z} piston[facing={face},extended=true]',label+'-piston-extended',1)
    read(f'data get block {dx} {dy} {dz} progress',label+'-actual-progress',True)
    px,py,pz=(x,y,z+1) if face=='east' else (x+1,y,z);c(f'/execute if block {px} {py} {pz} redstone_block',label+'-power-retained',1)
    c(f'/data get block {dx} {dy} {dz}',label+'-full-moving-body',trace=True)
  c('/data get entity @s',f't{t}-owner-full',trace=True)
 c('/execute store result score end h255 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h255 -= start h255','clock-delta-op',3);c('/scoreboard players get end h255','clock-delta',3)
 return ops
if __name__=='__main__':
 build=harness_args.require_work_dir().resolve()
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 assert not build.exists();build.mkdir(parents=True);ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'scope':'actual piston honey Husk first/second push; no-push ArmorStand and vertical controls; complete Pos/Motion/OnGround; actual block/piston progress and clocks'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('NoAI Mob piston ground',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows);diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
