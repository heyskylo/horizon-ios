import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
CASES=[('arrow','arrow','chorus_flower',True,True,False,True,1),('snowball','snowball','chorus_flower',True,True,False,True,1),('pearl','ender_pearl','chorus_flower',True,True,False,False,0),('rule','arrow','chorus_flower',False,True,False,False,0),('owner','arrow','chorus_flower',True,True,True,False,0),('stone','arrow','stone',True,True,False,False,0),('nodrops','snowball','chorus_flower',True,False,False,True,0),('direct-chorus',None,'chorus_flower',True,True,False,True,0),('direct-dirt',None,'dirt',True,True,False,True,1)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,trace=False,callback=None):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if callback is not None:o['expect_callback']=callback
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,expected=None,callback=None,admission=False):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h116 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h116 store success score success h116 run '+text,label+'-read',result=expected,admission=admission,callback=callback)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h116',label+'-'+k,admission=admission)
 cmd('/tick freeze','freeze')
 for g in ['spawn_mobs','advance_time','advance_weather']:cmd('/gamerule '+g+' false',g)
 for g in ['mob_griefing','projectiles_can_break_blocks','block_drops']:cmd('/gamerule '+g+' true',g)
 cmd('/gamerule random_tick_speed 0','random0');cmd('/difficulty normal','normal')
 cmd('/forceload add 0 0 31 31','forceload',4);ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
 cmd('/fill 0 198 0 31 199 31 stone','floor',2048);cmd('/fill 0 200 0 31 205 31 barrier','prefill',6144);cmd('/fill 0 200 0 31 205 31 air','air',6144)
 cmd('/tp @s 2.5 200 2.5','player-pos',1);cmd('/gamemode creative @s','player-mode')
 cmd('/execute if entity @s[type=player,nbt={Health:20f,Pos:[2.5d,200d,2.5d],playerGameType:1}]','player-body',1);cmd('/data get entity @s UUID','player-uuid',4)
 cmd('/scoreboard objectives add h116 dummy','objective');cmd('/execute store result score start h116 run time query gametime','start',trace=True)
 cmd('/execute unless entity @e[type=item]','initial-unfiltered-empty',1)
 cmd('/summon pig 24.5 200 24.5 {UUID:[I;116,0,0,99],NoAI:1b,NoGravity:1b,Health:10f}','owner-pig',1)
 cmd('/execute if entity @e[type=pig,nbt={UUID:[I;116,0,0,99],NoAI:1b,NoGravity:1b,Health:10f,Pos:[24.5d,200d,24.5d]}]','owner-body',1)
 for n,(name,kind,block,rule,drops,owner,broken,count) in enumerate(CASES,1):
  x=6+(n-1)%3*6;z=6+(n-1)//3*6;pos=f'{x} 200 {z}';uid=str(uuid.UUID(int=(116<<96)|n));local=f'@e[type=item,x={x-1},y=198,z={z-1},dx=2,dy=6,dz=2]';one=local[:-1]+',limit=1]'
  cmd('/gamerule projectiles_can_break_blocks '+str(rule).lower(),name+'-projectile-rule-set');cmd('/gamerule projectiles_can_break_blocks',name+'-projectile-rule',int(rule))
  cmd('/gamerule block_drops '+str(drops).lower(),name+'-drop-rule-set');cmd('/gamerule block_drops',name+'-drop-rule',int(drops))
  cmd(f'/setblock {x} 199 {z} end_stone strict',name+'-support',1)
  cmd('/setblock '+pos+' '+block+('[age=0]' if block=='chorus_flower' else '')+' strict',name+'-source',1)
  for text,label in [(f'if loaded {pos}','loaded'),(f'if block {x} 198 {z} stone','floor'),(f'if block {x} 199 {z} end_stone','support-state'),(f'if block {pos} '+block,'source-state'),(f'if block {x} 201 {z} air','path-air'),('unless entity '+local,'local-items-empty')]:cmd('/execute '+text,name+'-'+label,1)
  cmd('/scoreboard players set before h116 0',name+'-before-zero',0)
  cmd('/execute store result score before h116 run execute if entity @e[type=item]',name+'-unfiltered-global-before',trace=True)
  cmd('/execute store result score case_start h116 run time query gametime',name+'-clock-before',trace=True)
  if kind:
   own=',Owner:[I;116,0,0,99]' if owner else '';extra=',pickup:0b' if kind=='arrow' else ''
   cmd(f'/summon {kind} {x}.5 202.5 {z}.5 {{UUID:[I;116,0,0,{n}],NoGravity:1b,Motion:[0d,-2d,0d]'+own+extra+'}',name+'-projectile',1)
   cmd(f'/execute if entity @e[type={kind},nbt={{UUID:[I;116,0,0,{n}],Pos:[{x}.5d,202.5d,{z}.5d],Motion:[0d,-2d,0d],NoGravity:1b}}]',name+'-projectile-body',1)
   cmd('/execute if entity '+uid,name+'-projectile-uuid',1)
   for key,expected in [('UUID',4),('Pos',3),('Motion',3)]:read('data get entity '+uid+' '+key,name+'-initial-'+key,expected=expected,admission=True)
   if owner:cmd(f'/execute if data entity {uid} Owner',name+'-owner-ref',1)
   cmd('/data get entity '+uid,name+'-projectile-before-full',trace=True)
   cmd('/tick step 1',name+'-step',admission=False)
   ops.append(dict(op='tick',n=1))
   if kind=='arrow':cmd(f'/execute if entity @e[type=arrow,nbt={{UUID:[I;116,0,0,{n}],inGround:1b}}]',name+'-actual-collision',1)
   else:cmd('/execute unless entity '+uid,name+'-actual-collision',1)
   cmd('/data get entity '+uid,name+'-projectile-after-full',trace=True)
  else:read('setblock '+pos+' air destroy',name+'-direct-destroy',expected=1,admission=True)
  cmd('/execute if block '+pos+' '+('air' if broken else block),name+'-post-source-state',1)
  cmd(f'/execute if block {x} 199 {z} end_stone',name+'-post-support',1)
  cmd('/execute store result score case_now h116 run time query gametime',name+'-clock-after',trace=True)
  cmd('/scoreboard players operation case_now h116 -= case_start h116',name+'-elapsed',int(bool(kind)));cmd('/scoreboard players get case_now h116',name+'-clock',int(bool(kind)))
  read('execute if entity '+local,name+'-unfiltered-local-count',callback=[bool(count),count])
  cmd('/scoreboard players set after h116 0',name+'-after-zero',0)
  cmd('/execute store result score after h116 run execute if entity @e[type=item]',name+'-unfiltered-global-after',trace=True)
  cmd('/scoreboard players operation after h116 -= before h116',name+'-global-conservation',count,admission=False)
  cmd('/scoreboard players get after h116',name+'-global-delta',count,admission=False)
  if count:
   item='dirt' if block=='dirt' else 'chorus_flower'
   read('execute if entity '+local[:-1]+f',nbt={{Item:{{id:"minecraft:{item}",count:1}}}}]',name+'-typed-item',callback=[True,1])
   read('data get entity '+one+' Item.id',name+'-item-id');read('data get entity '+one+' Item.count',name+'-item-count',expected=1)
   read('execute if data entity '+one+' UUID',name+'-item-uuid-presence',callback=[True,1]);read('execute unless data entity '+one+' Item.components',name+'-item-components',callback=[True,1])
   cmd('/data get entity '+one+' Item',name+'-full-stack',trace=True)
  cmd('/execute as '+local+' run data get entity @s',name+'-unfiltered-items-full',trace=True)
 cmd('/execute if entity @e[type=pig,nbt={UUID:[I;116,0,0,99],Health:10f,NoAI:1b,NoGravity:1b,Pos:[24.5d,200d,24.5d]}]','owner-preserved',1)
 cmd('/execute store result score now h116 run time query gametime','end-absolute',trace=True);cmd('/scoreboard players operation now h116 -= start h116','relative-op',7);cmd('/scoreboard players get now h116','relative-clock',7)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h116/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'strict_records':sum(not o.get('diagnostic',False) for o in rows),'diagnostic_labels':[o['label'] for o in rows if o.get('diagnostic',False)],'labels':[o['label'] for o in rows],'cases':CASES,'samples':[0],'scope':'Actual chorus projectile impact/drop: bound arrow/snowball, pearl/direct/rule/owner/stone controls; typed local unfiltered items and in-world global count deltas strict. Raw UUID/motion/full bodies and absolute global counts/clocks are predeclared diagnostics.'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Chorus projectile drops',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if reference:assert fresh==reference,'fresh reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(x) for x in fresh.splitlines()];b=[json.loads(x) for x in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows)
 diffs=[dict(label=o['label'],expected=x,actual=y,diagnostic=o.get('diagnostic',False)) for o,x,y in zip(rows,a,b) if x!=y]
 (build/'full-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 strict=[o for o in rows if not o.get('diagnostic',False)];gaps=[d for d in diffs if not d['diagnostic']]
 (build/'strict-report.json').write_text(json.dumps(dict(records=len(strict),passed=len(strict)-len(gaps),differences=gaps),indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('full',len(rows)-len(diffs),'/',len(rows),'strict',len(strict)-len(gaps),'/',len(strict));sys.exit(bool(gaps))
