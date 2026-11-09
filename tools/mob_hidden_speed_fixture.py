import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
CASES=[('ai-chain',0,False),('noai-chain',1,False),('loaded-flags',1,True),('strong-only',1,False)]
def program():
 ops=[]
 def c(text,label,result=None,admission=True,trace=False):
  o=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:o['expect_result']=result
  if trace:o.update(record=False,trace=True)
  ops.append(o)
 def read(text,label,admission=False):
  for k in ['result','success']:c(f'/scoreboard players set {k} h202 -99',label+'-reset-'+k,-99)
  c('/execute store result score result h202 store success score success h202 run '+text,label+'-call',admission=admission)
  for k in ['result','success']:c(f'/scoreboard players get {k} h202',label+'-'+k,admission=admission)
 c('/tick freeze','freeze');c('/gamerule spawn_mobs false','no-spawn');c('/gamerule advance_time false','no-daylight');c('/gamerule random_tick_speed 0','random0')
 c('/forceload add 0 0','loaded-chunk',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 c('/fill 0 199 0 15 199 15 stone','floor',256);c('/fill 0 200 0 15 204 15 barrier','prefill',1280);c('/fill 0 200 0 15 204 15 air','air',1280)
 c('/tp @s 14.5 200 14.5','owner-pos',1);c('/gamemode creative @s','owner-mode');c('/execute if entity @s[type=player,nbt={Health:20f,Pos:[14.5d,200d,14.5d],playerGameType:1}]','owner-body',1);c('/data get entity @s UUID','owner-uuid',4)
 c('/scoreboard objectives add h202 dummy','objective');c('/execute store result score start h202 run time query gametime','clock-start',trace=True)
 for n,(name,noai,loaded) in enumerate(CASES,1):
  x=2+3*(n-1);z=4;u=str(uuid.UUID(int=(202<<96)|n))
  c(f'/fill {x-1} 200 {z-1} {x+1} 203 {z+1} stone',name+'-cage-solid',36);c(f'/fill {x} 200 {z} {x} 203 {z} air',name+'-cage-air',4)
  effect=',active_effects:[{id:"minecraft:speed",duration:40,amplifier:1b,show_icon:1b,hidden_effect:{duration:200,ambient:1b,show_particles:0b,show_icon:0b}}]' if loaded else ''
  c(f'/summon pig {x}.5 200 {z}.5 '+'{'+f'UUID:[I;202,0,0,{n}],Health:10f,NoAI:{noai}b,NoGravity:1b,Motion:[0d,0d,0d],Tags:[h202],CustomName:"{name}"'+effect+'}',name+'-summon',1)
  c(f'/execute if entity {u}',name+'-direct-uuid',1)
  c(f'/execute as {u} if entity @s[type=pig,nbt='+'{'+f'UUID:[I;202,0,0,{n}],Health:10f,Pos:[{x}.5d,200d,{z}.5d],NoGravity:1b' + (',NoAI:1b' if noai else '')+'}]',name+'-initial-body',1)
  if not noai:c(f'/execute unless data entity {u} NoAI',name+'-declared-ai',1)
  if not loaded:
   if name!='strong-only':read(f'effect give {u} speed 10 0',name+'-weak',True)
   read(f'effect give {u} speed 2 1',name+'-strong',True)
  if name=='noai-chain':
   read(f'effect give {u} speed 2 1',name+'-identical-strong-reapply')
   read(f'effect give {u} speed 5 0',name+'-shorter-weak-reapply')
 last=0
 for t in [0,39,40,41]:
  if t:c(f'/tick step {t-last}',f'tick-{t}',admission=False);ops.append(dict(op='tick',n=t-last))
  c('/execute store result score now h202 run time query gametime',f't{t}-absolute',trace=True);c('/scoreboard players operation now h202 -= start h202',f't{t}-clock-operation',t);c('/scoreboard players get now h202',f't{t}-clock',t)
  for n,(name,noai,loaded) in enumerate(CASES,1):
   if name=='strong-only' and t in [39,40]:continue
   u=str(uuid.UUID(int=(202<<96)|n));p=f'{name}-t{t}';x=2+3*(n-1)
   c(f'/execute as {u} if entity @s[type=pig,nbt='+'{'+f'UUID:[I;202,0,0,{n}],Health:10f,NoGravity:1b' + (',NoAI:1b' if noai else '')+'}]',p+'-identity',1)
   c(f'/execute if loaded {x} 200 4',p+'-loaded',1);c(f'/execute if block {x} 199 4 stone',p+'-floor',1)
   path='active_effects[{id:"minecraft:speed"}]'
   read(f'data get entity {u} '+path+'.duration',p+'-active-duration')
   read(f'data get entity {u} '+path+'.hidden_effect.duration',p+'-hidden-duration')
   read(f'execute if data entity {u} '+path+'.hidden_effect',p+'-hidden-presence')
   if name=='strong-only' and t==41:c(f'/execute unless data entity {u} active_effects',p+'-expired',1,False)
   else:
    d=40-t if t<40 else 200-t
    node='{id:"minecraft:speed",duration:'+str(d)
    if t<40:node+=',amplifier:1b,show_icon:1b'
    elif loaded:node+=',ambient:1b,show_particles:0b,show_icon:0b'
    else:node+=',show_icon:1b'
    node+='}'
    c(f'/execute if data entity {u} '+'{active_effects:['+node+']}',p+'-active-typed',1,False)
    if t>=40:c(f'/execute unless data entity {u} '+path+'.amplifier',p+'-default-amplifier-omitted',1,False)
    elif name!='strong-only':
     h='{duration:'+str(200-t)+( ',ambient:1b,show_particles:0b,show_icon:0b' if loaded else ',show_icon:1b')+'}'
     c(f'/execute if data entity {u} '+'{active_effects:[{id:"minecraft:speed",hidden_effect:'+h+'}]}',p+'-hidden-typed',1,False)
   c(f'/data get entity {u} active_effects',p+'-full-effects',trace=True);c(f'/data get entity {u}',p+'-full-body',trace=True)
  last=t
 u=str(uuid.UUID(int=(202<<96)|2));read(f'effect clear {u} speed','clear-promoted');read(f'execute unless data entity {u} active_effects','clear-no-stale-chain')
 c('/execute store result score end h202 run time query gametime','end-absolute',trace=True);c('/scoreboard players operation end h202 -= start h202','final-clock-op',41);c('/scoreboard players get end h202','final-clock',41)
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not build.exists();build.mkdir(parents=True);ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 (build/'declaration.json').write_text(json.dumps({'sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'scope':'Speed active/hidden state, actual AI+NoAI pigs, load and command paths, clocks0/39/40/41, no movement/attributes/packets claim'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Live dungeon spawner entity',ops)];sys.argv.append('--allow-gaps');driver.main();fresh=(build/'vanilla.jsonl').read_bytes()
 if reference is not None:assert fresh==reference,'reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(s) for s in fresh.splitlines()];b=[json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows);diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps({'records':len(rows),'passed':len(rows)-len(diffs),'differences':diffs},indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print('strict',len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
