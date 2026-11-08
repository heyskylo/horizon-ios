import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('fence',1,6),('unsupported',2,10),('removed',3,14)]
TIMES=[0,1,100,101]
def program():
 ops=[];clock=0
 def cmd(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h129 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h129 store success score success h129 run '+text,label+'-read',admission=admission)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h129',label+'-'+k,admission=admission)
 def tick(n):
  nonlocal clock
  cmd(f'/tick step {n}',f'step{clock+n}',admission=False);ops.append(dict(op='tick',n=n));clock+=n
 def clocks(t):
  cmd('/execute store result score now h129 run time query gametime',f't{t}-absolute',trace=True)
  cmd('/scoreboard players operation now h129 -= start h129',f't{t}-delta-op',t)
  cmd('/scoreboard players get now h129',f't{t}-delta',t)
 cmd('/tick freeze','freeze')
 for g in ['spawn_mobs','advance_time','advance_weather','mob_griefing']:cmd('/gamerule '+g+' false',g)
 cmd('/gamerule random_tick_speed 0','random0');cmd('/difficulty normal','normal')
 cmd('/forceload add 0 0 31 31','forceload',4);ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
 cmd('/fill 0 199 0 31 199 31 stone','floor',1024);cmd('/fill 0 200 0 31 204 31 barrier','prefill',5120);cmd('/fill 0 200 0 31 204 31 air','air',5120)
 cmd('/tp @s 2.5 200 2.5','owner-pos',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[2.5d,200d,2.5d],playerGameType:1}]','owner-body',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/scoreboard objectives add h129 dummy','objective');cmd('/execute store result score start h129 run time query gametime','start',trace=True)
 cmd('/summon pig 20.5 200 10.5 {UUID:[I;129,0,0,99],NoAI:1b,NoGravity:1b,Health:10f}','pig',1)
 for name,n,x in CASES:
  cmd(f'/execute if loaded {x} 200 6',name+'-loaded',1);cmd(f'/execute if block {x} 199 6 stone',name+'-floor',1)
  if name!='unsupported':cmd(f'/setblock {x} 200 6 oak_fence strict',name+'-fence',1)
  cmd(f'/execute if block {x} 200 6 '+('air' if name=='unsupported' else 'oak_fence[north=false,east=false,south=false,west=false,waterlogged=false]'),name+'-support',1)
  cmd(f'/summon leash_knot {x}.2 200.8 6.9 {{UUID:[I;129,0,0,{n}],CustomName:"{name}",Tags:["h129"]}}',name+'-summon',1)
 last=0
 for t in TIMES:
  if t:tick(t-last)
  clocks(t)
  cmd('/execute if entity @e[type=pig,nbt={UUID:[I;129,0,0,99],Health:10f,NoAI:1b,NoGravity:1b,Pos:[20.5d,200d,10.5d]}]',f't{t}-pig-control',1)
  for name,n,x in CASES:
   uid=str(uuid.UUID(int=(129<<96)|n));alive=name=='fence' or t<101
   read('execute if entity '+uid,f'{name}-t{t}-direct')
   read(f'execute if entity @e[type=leash_knot,nbt={{UUID:[I;129,0,0,{n}]}}]',f'{name}-t{t}-typed')
   if alive:
    for key in ['UUID','Pos','Motion','Rotation','Air','Invulnerable','OnGround','PortalCooldown','fall_distance']:
     read('data get entity '+uid+' '+key,f'{name}-t{t}-'+key)
    cmd('/data get entity '+uid,f'{name}-t{t}-full',trace=True)
   else:cmd('/data get entity '+uid,f'{name}-t{t}-removed-full',trace=True)
  if t==1:
   cmd('/setblock 14 200 6 air strict','remove-support',1);cmd('/execute if block 14 200 6 air','removed-support-admission',1)
  last=t
 read('execute if entity @e[type=leash_knot]','survivor-count')
 read('kill @e[type=leash_knot]','cleanup-count')
 cmd('/execute unless entity @e[type=leash_knot]','cleanup-empty',1)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h129/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'cases':CASES,'samples':TIMES,'scope':'Explicit knots: direct UUID/type identity, normalized typed Pos/default fields and support lifetime strict; full bodies retained as diagnostics. Natural knots and lead behavior outside runtime scope.'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Summoned leash knots',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if reference:assert fresh==reference,'fresh reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(x) for x in fresh.splitlines()];b=[json.loads(x) for x in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows)
 diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print(len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
