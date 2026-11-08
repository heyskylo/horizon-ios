#!/usr/bin/env python3
"""Actual named deterministic predicate context, valid graph contract."""
import json,sys,hashlib,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
P={
 'h121:pig':{'type':'minecraft:entity_properties','entity':'this','predicate':{'minecraft:entity_type':'minecraft:pig'}},
 'h121:cow':{'type':'minecraft:entity_properties','entity':'this','predicate':{'minecraft:entity_type':'minecraft:cow'}},
 'h121:reference':{'type':'minecraft:all_of','terms':['h121:pig']},
 'h121:inverted':{'type':'minecraft:inverted','term':'h121:pig'},
 'h121:conjunction':{'type':'minecraft:all_of','terms':['h121:pig',{'type':'minecraft:inverted','term':'h121:cow'}]},
 'h121:both':{'type':'minecraft:all_of','terms':['h121:pig','h121:cow']},
 'h121:either':{'type':'minecraft:any_of','terms':['h121:pig','h121:cow']},
}
ACTORS={'pig':([121,1,2,3],10,[4.5,200,4.5]),'cow':([121,4,5,6],10,[8.5,200,4.5])}
def uid(parts):return str(uuid.UUID(int=sum((n&0xffffffff)<<(32*(3-i)) for i,n in enumerate(parts))))
def program():
 ops=[]
 def c(cmd,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=cmd,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]));c('/fill 0 199 0 15 199 15 stone','floor',256);c('/tp @s 12.5 200 12.5 0 0','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/data get entity @s UUID','owner-uuid',4);c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[12.5d,200d,12.5d]}]','owner-body',1)
 for kind,(parts,hp,pos) in ACTORS.items():
  ints=','.join(map(str,parts));target=uid(parts);p=','.join(f'{v}d' for v in pos)
  c(f'/summon {kind} {pos[0]} {pos[1]} {pos[2]} {{UUID:[I;{ints}],Health:{hp}f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b}}',kind+'-summon')
  c(f'/data get entity {target} UUID',kind+'-uuid',4)
  c(f'/execute if entity @e[type={kind},nbt={{UUID:[I;{ints}],Health:{hp}f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Pos:[{p}]}}]',kind+'-body',1)
  c(f'/execute if loaded {int(pos[0])} 200 4',kind+'-loaded',1);c(f'/execute if block {int(pos[0])} 199 4 stone',kind+'-support',1)
  c(f'/data get entity {target}',kind+'-raw-body',trace=True)
 c('/scoreboard objectives add h121 dummy','objective');c('/function h121:pack','pack-function');c('/scoreboard players get pack h121','pack-admitted',17);c('/execute store result score start h121 run time query gametime','clock-start',trace=True)
 c('/datapack list enabled','pack-list',trace=True)
 for kind,(parts,hp,pos) in ACTORS.items():
  target=uid(parts)
  truth={'pig':kind=='pig','cow':kind=='cow','reference':kind=='pig','inverted':kind!='pig','conjunction':kind=='pig','both':False,'either':True,'absent':None}
  for name,holds in truth.items():
   for sign in ['if','unless']:
    label=f'{kind}-{name}-{sign}'
    for key in ['branch','result','success']:c(f'/scoreboard players set {key} h121 -99',label+'-reset-'+key,-99)
    c('/scoreboard players get branch h121',label+'-before',-99)
    c(f'/execute store result score result h121 store success score success h121 as {target} {sign} predicate h121:{name} run scoreboard players set branch h121 1',label+'-call',admission=False)
    for key in ['result','success']:c(f'/scoreboard players get {key} h121',label+'-'+key,admission=False)
    expected=1 if holds is not None and holds==(sign=='if') else -99
    c('/scoreboard players get branch h121',label+'-after',expected,admission=name in ['pig','cow','absent'])
 for kind,(parts,hp,pos) in ACTORS.items():
  ints=','.join(map(str,parts));p=','.join(f'{v}d' for v in pos)
  c(f'/execute if entity @e[type={kind},nbt={{UUID:[I;{ints}],Health:{hp}f,NoAI:1b,NoGravity:1b,Pos:[{p}]}}]',kind+'-post-body',1)
  c(f'/data get entity {uid(parts)}',kind+'-post-raw',trace=True)
 c('/execute store result score end h121 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h121 -= start h121','clock-delta-operation',0);c('/scoreboard players get end h121','clock-delta',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).resolve();del sys.argv[i:i+2]
 if '--work-dir' not in sys.argv:
  parent=Path(tempfile.mkdtemp(prefix='named-predicate-',dir=driver.lab.LOCAL));sys.argv+=['--work-dir',str(parent/'run')]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists(),'preserve attempts';build.mkdir(parents=True)
 ops=program();recorded=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),records=len(recorded),labels=[o['label'] for o in recorded],predicates=P,actors=ACTORS,scope='complete valid-graph records strict; raw bodies/absolute clocks/pack lists separate declared diagnostics; invalid-world probes separate'),indent=2)+'\n')
 driver.corpus_module.predicate_selector_fixtures=lambda names:P
 driver.corpus_module.function_return_fixtures=lambda names:{'h121:pack':'scoreboard players set pack h121 17\n'}
 driver.corpus_module.groups=lambda:[('Named deterministic predicate context',ops)]
 expected=reference.read_bytes() if reference else None
 sys.argv.append('--allow-gaps');driver.main()
 jb=(build/'vanilla.jsonl').read_bytes()
 if expected is not None:assert jb==expected and reference.read_bytes()==expected,'reference bytes changed'
 if '--reference-only' not in sys.argv:
  java=[json.loads(s) for s in jb.splitlines()];native=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(recorded)==len(java)==len(native)
  residuals=[dict(label=o['label'],expected=a,actual=b) for o,a,b in zip(recorded,java,native) if a!=b];r=dict(records=len(recorded),passed=len(recorded)-len(residuals),residuals=residuals,reference_sha256=hashlib.sha256(jb).hexdigest());(build/'strict-report.json').write_text(json.dumps(r,indent=2)+'\n')
  for o,a,b in zip(recorded,java,native):
   if o['admission']:assert a==b,('admission',o,a,b)
  print(f'Strict records: {r["passed"]}/{r["records"]}')
  if residuals:raise SystemExit(1)
