import json,sys,hashlib,tempfile,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
Z={'type':'minecraft:random_chance','chance':0.0};O={'type':'minecraft:random_chance','chance':1.0}
P={'h122:zero':Z,'h122:one':O,'h122:not_zero':{'type':'minecraft:inverted','term':Z},'h122:not_one':{'type':'minecraft:inverted','term':O},'h122:all_true':{'type':'minecraft:all_of','terms':[O,O]},'h122:all_false':{'type':'minecraft:all_of','terms':[Z,O]},'h122:any_true':{'type':'minecraft:any_of','terms':[O,Z]},'h122:any_false':{'type':'minecraft:any_of','terms':[Z,Z]},'h122:pig':{'type':'minecraft:entity_properties','entity':'this','predicate':{'minecraft:entity_type':'minecraft:pig'}}}
UID=str(uuid.UUID(int=(122<<96)|1))
def program():
 ops=[]
 def c(cmd,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=cmd,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]));c('/fill 0 199 0 15 199 15 stone','floor',256);c('/tp @s 12.5 200 12.5 0 0','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/data get entity @s UUID','owner-uuid',4);c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[12.5d,200d,12.5d]}]','owner-body',1)
 c('/summon pig 4.5 200 4.5 {UUID:[I;122,0,0,1],Health:10f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b}','pig-summon',1);c('/data get entity '+UID+' UUID','pig-uuid',4)
 guard='/execute if entity @e[type=pig,nbt={UUID:[I;122,0,0,1],Health:10f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Pos:[4.5d,200d,4.5d]}]'
 c(guard,'pig-body',1);c('/execute if loaded 4 200 4','loaded',1);c('/execute if block 4 199 4 stone','support',1);c('/data get entity '+UID,'pig-full',trace=True)
 c('/scoreboard objectives add h122 dummy','objective');c('/function h122:pack','pack-function');c('/scoreboard players get pack h122','pack-admitted',17);c('/execute store result score start h122 run time query gametime','clock-start',trace=True);c('/datapack list enabled','pack-list',trace=True)
 for actor,target in [('pig',UID),('owner','@s')]:
  truth={'zero':False,'one':True,'not_zero':True,'not_one':False,'all_true':True,'all_false':False,'any_true':True,'any_false':False,'pig':actor=='pig'}
  for name,holds in truth.items():
   for sign in ['if','unless']:
    label=f'{actor}-{name}-{sign}'
    for key in ['branch','result','success']:c(f'/scoreboard players set {key} h122 -99',label+'-reset-'+key,-99)
    c('/scoreboard players get branch h122',label+'-before',-99)
    c(f'/execute store result score result h122 store success score success h122 as {target} {sign} predicate h122:{name} run scoreboard players set branch h122 1',label+'-call',admission=False)
    for key in ['result','success']:c(f'/scoreboard players get {key} h122',label+'-'+key,admission=False)
    c('/scoreboard players get branch h122',label+'-after',1 if holds==(sign=='if') else -99,admission=name=='pig')
 c(guard,'pig-preserved',1);c('/data get entity '+UID,'pig-post-full',trace=True);c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[12.5d,200d,12.5d]}]','owner-preserved',1)
 c('/execute store result score end h122 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h122 -= start h122','clock-delta-operation',0);c('/scoreboard players get end h122','clock-delta',0)
 return ops
if __name__=='__main__':
 build=harness_args.require_work_dir().resolve()
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 assert not build.exists();build.mkdir(parents=True)
 ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'predicates':P,'scope':'direct top-level named literal0/1 plus inline logical forms, no nested reference or provider claim'},indent=2)+'\n')
 driver.corpus_module.predicate_selector_fixtures=lambda names:P
 driver.corpus_module.function_return_fixtures=lambda names:{'h122:pack':'scoreboard players set pack h122 17\n'}
 driver.corpus_module.groups=lambda:[('Named random chance endpoints',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows)
 diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y]
 (build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
