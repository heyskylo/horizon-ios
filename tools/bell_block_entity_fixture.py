import json,sys,hashlib
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
def program():
 ops=[]
 def c(cmd,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=cmd,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:c(f'/scoreboard players set {k} h174 -99',label+'-reset-'+k,-99)
  c('/execute store result score result h174 store success score success h174 run '+text,label+'-call',admission=admission)
  for k in ['result','success']:c(f'/scoreboard players get {k} h174',label+'-'+k,admission=admission)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]));c('/fill 0 199 0 15 199 15 stone','floor',256);c('/tp @s 12.5 200 12.5','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[12.5d,200d,12.5d]}]','owner-body',1);c('/data get entity @s UUID','owner-uuid',4)
 c('/scoreboard objectives add h174 dummy','objective');c('/execute store result score start h174 run time query gametime','clock-start',trace=True)
 for x,block in [(4,'bell[facing=north,attachment=floor,powered=false]'),(8,'chest[facing=north]'),(10,'stone')]:
  c(f'/setblock {x} 200 4 '+block+' strict',block.split('[')[0]+'-place',1);c(f'/execute if loaded {x} 200 4',str(x)+'-loaded',1);c(f'/execute if block {x} 199 4 stone',str(x)+'-support',1);c(f'/execute if block {x} 200 4 '+block,str(x)+'-state',1)
 for repeat in [0,1]:
  for x,name in [(4,'bell'),(8,'chest')]:
   label=f'{name}-{repeat}';admission=name=='chest';p=f'{x} 200 4'
   read('execute if data block '+p+f' {{id:"minecraft:{name}",x:{x},y:200,z:4}}',label+'-typed-metadata',admission)
   for key in ['id','x','y','z']:read('data get block '+p+' '+key,label+'-'+key,admission)
   c('/data get block '+p,label+'-full',trace=True)
  read('data get block 10 200 4 id',f'stone-{repeat}-negative')
 read('execute if data block 8 200 4 Items','chest-items-present',True);read('data get block 8 200 4 Items','chest-empty-list',True)
 c('/execute if block 4 200 4 bell[facing=north,attachment=floor,powered=false]','bell-preserved',1);c('/execute if block 8 200 4 chest[facing=north]','chest-preserved',1);c('/execute if block 10 200 4 stone','stone-preserved',1)
 c('/execute store result score end h174 run time query gametime','clock-end',trace=True);c('/scoreboard players operation end h174 -= start h174','clock-delta-operation',0);c('/scoreboard players get end h174','clock-delta',0)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists();build.mkdir(parents=True);ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'scope':'direct floor bell identity and coordinates only; chest/nonBE/repeatedreads/zero clocks; raw full compounds diagnostic; no generated/runtime/tick closure'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Bell block entity command identity',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows);diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
