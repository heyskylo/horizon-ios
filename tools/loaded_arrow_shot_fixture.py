import hashlib,json,sys,uuid
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'tools'))
import test_command_exec_parity as driver
import harness_args
OWNER='00000072-0000-0000-0000-000000000063'
CASES=[('positive',None),('omitted',None),('false',0),('true',1),('crossbow',None)]
TIMES=[0,1,3,4,6]
def program():
 ops=[];clock=0
 def cmd(text,label,result=None,admission=True,trace=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if trace:op.update(record=False,trace=True)
  ops.append(op)
 def tick(n,label):
  nonlocal clock
  cmd(f'/tick step {n}',label+'-step',admission=False);ops.append(dict(op='tick',n=n));clock+=n
 def read(command,label,admission=False):
  for key in ['result','success']:cmd(f'/scoreboard players set {key} h114 -99',label+'-reset-'+key,-99)
  cmd('/execute store result score result h114 store success score success h114 run '+command,label+'-read',admission=admission)
  for key in ['result','success']:cmd(f'/scoreboard players get {key} h114',label+'-'+key,admission=admission)
 def clocks(label):
  cmd('/execute store result score now h114 run time query gametime',label+'-absolute',trace=True);cmd('/scoreboard players operation now h114 -= start h114',label+'-delta-op',clock);cmd('/scoreboard players get now h114',label+'-delta',clock)
 cmd('/tick freeze','freeze')
 for g in ['spawn_mobs','advance_time','advance_weather','mob_griefing']:cmd('/gamerule '+g+' false',g)
 cmd('/gamerule random_tick_speed 0','no-random');cmd('/gamerule show_advancement_messages false','no-advancement-chat');cmd('/difficulty normal','normal')
 cmd('/forceload add 0 0 31 31','forceload',4);ops.append(dict(op='settle',required_chunks=[[0,0],[0,1],[1,0],[1,1]]))
 cmd('/fill 0 199 0 31 199 31 stone','floor',1024);cmd('/fill 0 200 0 31 205 31 barrier','air-prefill',6144);cmd('/fill 0 200 0 31 205 31 air','air',6144)
 cmd('/tp @s 2.5 200 2.5 0 0','player-position',1);cmd('/gamemode creative @s','player-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[2.5d,200d,2.5d]}]','player-admission',1);cmd('/data get entity @s UUID','player-uuid',4)
 cmd('/summon pig 6.5 200 10.5 {UUID:[I;114,0,0,99],NoAI:1b,NoGravity:1b,Health:10f}','owner-summon',1)
 cmd('/scoreboard objectives add h114 dummy','objective');cmd('/execute store result score start h114 run time query gametime','absolute-start',trace=True)
 tick(1,'owner-first-tick');cmd('/setblock 9 200 10 stone strict','initial-sensor-cell',1)
 for i,(name,flag) in enumerate(CASES):
  cmd('/kill @e[type=arrow]',name+'-clear-arrows',admission=False);cmd('/kill @e[type=item]',name+'-clear-items',admission=False)
  if name=='crossbow':
   cmd('/tp @s 6.5 200 6.5 0 0','crossbow-stance',1)
   cmd('/item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:"minecraft:arrow",count:1}]]','crossbow-loaded',1)
   cmd('/data get entity @s SelectedItemSlot','crossbow-selected-slot',0)
   cmd('/data get entity @s Inventory[{Slot:0b}].id','crossbow-item-id',18)
   cmd('/data get entity @s Inventory[{Slot:0b}].components."minecraft:charged_projectiles"[0].id','crossbow-ammo-id',15)
   cmd('/data get entity @s Inventory[{Slot:0b}]','crossbow-stack-before',trace=True)
  cmd('/setblock 9 200 10 air strict',name+'-reset-air',1);tick(45,name+'-drain');cmd('/setblock 9 200 10 sculk_sensor strict',name+'-sensor',1);tick(2,name+'-ready')
  cmd('/execute if loaded 6 201 10',name+'-source-loaded',1);cmd('/execute if loaded 9 200 10',name+'-sensor-loaded',1)
  cmd('/execute if block 9 199 10 stone',name+'-support',1);cmd('/execute if block 6 201 10 air',name+'-arena-air',1)
  cmd('/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase=inactive,power=0]',name+'-ready-state',1)
  cmd('/data get block 9 200 10 last_vibration_frequency',name+'-ready-frequency',0);cmd('/execute unless data block 9 200 10 listener.event',name+'-no-current',1);cmd('/execute unless data block 9 200 10 listener.selector.event',name+'-no-candidate',1)
  hp=10 if name=='positive' else 9
  cmd(f'/execute if entity @e[type=pig,nbt={{UUID:[I;114,0,0,99],Health:{hp}f,NoAI:1b,NoGravity:1b,Pos:[6.5d,200d,10.5d]}}]',name+'-owner-body',1)
  if name=='positive':read('damage '+OWNER+' 1 minecraft:generic',name+'-damage')
  elif name=='crossbow':
   ops.append(dict(op='use_item',record=False));cmd('/execute if entity @e[type=arrow]',name+'-real-arrow',1);cmd('/execute unless data entity @s Inventory[{Slot:0b}].components."minecraft:charged_projectiles"[0]',name+'-charge-consumed',1);cmd('/data get entity @s Inventory[{Slot:0b}]',name+'-stack-after',trace=True)
  else:
   uid=str(uuid.UUID(int=(114<<96)|i));suffix='' if flag is None else f',HasBeenShot:{flag}b'
   cmd('/summon arrow 6.5 201.5 10.5 {'+f'UUID:[I;114,0,0,{i}],NoGravity:1b,Motion:[0d,0d,0d],Owner:[I;114,0,0,99],item:{{id:"minecraft:arrow",count:1}}'+suffix+'}',name+'-summon',1)
  last=0
  for t in TIMES:
   if t:tick(t-last,name+f'-tick{t}')
   clocks(name+f'-t{t}')
   if name!='positive':
    target='@e[type=arrow,limit=1]' if name=='crossbow' else str(uuid.UUID(int=(114<<96)|i))
    expected=int(name=='true' or t>0)
    cmd('/execute if entity @e[type=arrow]',name+f'-t{t}-count',1)
    if name!='crossbow':cmd('/execute if entity @e[type=arrow,nbt={'+f'UUID:[I;114,0,0,{i}],NoGravity:1b,Motion:[0d,0d,0d],Pos:[6.5d,201.5d,10.5d],HasBeenShot:{expected}b'+'}]',name+f'-t{t}-body',1)
    else:cmd('/execute if entity @e[type=arrow,nbt={'+f'HasBeenShot:{expected}b'+'}]',name+f'-t{t}-shot-flag',1)
    if t in [0,1,6]:read('data get entity '+target+' HasBeenShot',name+f'-t{t}-flag',True)
    if t==0:
     cmd('/data get entity '+target+' item.id',name+'-item-id',15);cmd('/data get entity '+target+' item.count',name+'-item-count',1)
     if name!='crossbow':cmd('/data get entity '+target+' Owner',name+'-actual-owner',4)
    for key in ['UUID','Owner','Pos','Motion']:
     cmd('/data get entity '+target+' '+key,name+f'-t{t}-raw-'+key,trace=True)
    cmd('/data get entity '+target,name+f'-t{t}-full-arrow',trace=True)
   arrival=3 if name=='positive' else 4
   active=name!='true' and t>=arrival
   power=9 if name=='crossbow' else 10
   cmd('/execute if block 9 200 10 sculk_sensor[sculk_sensor_phase='+('active' if active else 'inactive')+',power='+str(power if active else 0)+']',name+f'-t{t}-sensor-state',admission=False)
   if t in [0,6]:read('data get block 9 200 10 last_vibration_frequency',name+f'-t{t}-frequency')
   if t in [0,1,3]:
    for key in ['listener.event.game_event','listener.selector.event.game_event']:
     read('data get block 9 200 10 '+key,name+f'-t{t}-'+key)
   cmd('/data get block 9 200 10',name+f'-t{t}-full-listener',trace=True)
   last=t
 return ops
if __name__=='__main__':
 reference=None
 if '--reference' in sys.argv:
  i=sys.argv.index('--reference');reference=Path(sys.argv[i+1]).read_bytes();del sys.argv[i:i+2]
 build=harness_args.ensure_work_dir(Path('.local/h114/capture')).resolve();assert not (build/'vanilla.jsonl').exists();ops=program();rows=[o for o in ops if o['op']=='cmd' and o.get('record',True)]
 build.mkdir(parents=True,exist_ok=True);(build/'declaration.json').write_text(json.dumps({'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'records':len(rows),'labels':[o['label'] for o in rows],'cases':CASES,'samples':TIMES,'scope':'Sensor arrival/phase/frequency and event ID strings strict; actual arrow flag/type/item/load/owner and loaded stationary Pos/Motion strict. Natural Pos/Motion/random UUID, full listener/source UUID/epochs/body are raw diagnostics. No trajectory/full-listener/source-UUID-codec equality claim.'},indent=2)+'\n')
 driver.corpus_module.groups=lambda:[('Loaded arrow first-shot event',ops)];sys.argv.append('--allow-gaps');driver.main()
 fresh=(build/'vanilla.jsonl').read_bytes()
 if reference:assert fresh==reference,'fresh reference changed'
 if '--reference-only' in sys.argv:sys.exit(0)
 a=[json.loads(x) for x in fresh.splitlines()];b=[json.loads(x) for x in (build/'rust.jsonl').read_bytes().splitlines()];assert len(a)==len(b)==len(rows)
 diffs=[dict(label=o['label'],expected=x,actual=y) for o,x,y in zip(rows,a,b) if x!=y];(build/'strict-report.json').write_text(json.dumps(dict(records=len(rows),passed=len(rows)-len(diffs),differences=diffs),indent=2)+'\n')
 for o,x,y in zip(rows,a,b):
  if o['admission']:assert x==y,('admission',o,x,y)
 print(len(rows)-len(diffs),'/',len(rows));sys.exit(bool(diffs))
