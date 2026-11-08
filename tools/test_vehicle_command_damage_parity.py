#!/usr/bin/env python3
"""Generic vehicle damage (no attacker, or a creative player), exact command records and raw bodies."""
import hashlib,json,sys,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 def dirt(near,label):
  # Every pile of the chest's 40 dirt, whatever the random split: one is in the vehicle's block,
  # and their counts add up to 40 (each pile's count into its own score, then summed).
  sel=f'@e[type=item,nbt={{Item:{{id:"minecraft:dirt"}}}},{near}'
  cmd(f'/execute if entity {sel},limit=1]',label+'-dirt-present',1,admission=False)
  cmd(f'/execute as {sel}] store result score @s h050 run data get entity @s Item.count',label+'-dirt-counts',diagnostic=True)
  holder=label.replace('-','_')+'_dirt'
  cmd(f'/scoreboard players set {holder} h050 0',label+'-dirt-total-reset',0)
  cmd(f'/scoreboard players operation {holder} h050 += {sel}] h050',label+'-dirt-total-sum',diagnostic=True)
  cmd(f'/scoreboard players get {holder} h050',label+'-dirt-total',40,admission=False)
 def action(text,label):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h050 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h050 store success score success h050 run '+text,label,admission=False)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h050',label+'-'+k,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/gamerule entity_drops true','drops-on')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
 cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/scoreboard objectives add h050 dummy','objective')
 cmd('/execute store result score start h050 run time query gametime','clock-start',diagnostic=True)
 cmd('/execute unless entity @e[type=item]','initial-no-items',1)
 for kind,base,z in [('oak_boat',1,3),('minecart',4,9)]:
  for mode,offset,x in [('sequence',0,3),('threshold',1,7),('invulnerable',2,11)]:
   i=base+offset;u=str(uuid.UUID(int=(500<<96)|i));label=kind+'-'+mode
   inv=1 if mode=='invulnerable' else 0
   cmd(f'/execute if loaded {x} 200 {z}',label+'-load',1)
   cmd(f'/execute if block {x} 199 {z} stone',label+'-support',1)
   cmd(f'/summon {kind} {x}.5 200 {z}.5 '+f'{{UUID:[I;500,0,0,{i}],NoGravity:1b,Invulnerable:{inv}b,Motion:[0d,0d,0d]}}',label+'-summon',1)
   guard=f'{{UUID:[I;500,0,0,{i}],Pos:[{x}.5d,200d,{z}.5d],Motion:[0d,0d,0d],NoGravity:1b,Invulnerable:{inv}b}}'
   cmd(f'/execute as {u} if entity @s[type={kind},nbt='+guard+']',label+'-actor',1)
   for f in ['UUID','Pos','Motion','NoGravity','Invulnerable']:cmd(f'/data get entity {u} {f}',label+'-initial-'+f)
   cmd(f'/data get entity {u}',label+'-initial-full',diagnostic=True)
   action(f'damage {u} '+('1' if mode=='sequence' else '4')+' minecraft:generic',label+'-hit')
   cmd(f'/execute as {u} if entity @s[type={kind},nbt='+guard+']',label+'-survives',1)
   cmd(f'/data get entity {u}',label+'-after-hit-full',diagnostic=True)
   if mode=='sequence':
    action(f'damage {u} 4 minecraft:generic',label+'-break')
    cmd(f'/execute unless entity {u}',label+'-removed',admission=False)
    cmd(f'/execute if entity @e[type=item,nbt={{Item:{{id:"minecraft:{kind}",count:1}}}}]',label+'-hull',admission=False)
    cmd(f'/data get entity @e[type=item,nbt={{Item:{{id:"minecraft:{kind}"}}}},limit=1] Item',label+'-hull-stack',admission=False)
    # VehicleEntity.destroy's spawnAtLocation: the ItemEntity's own motion, 0.2 up (X/Z are random).
    if kind=='oak_boat':cmd(f'/data get entity @e[type=item,nbt={{Item:{{id:"minecraft:{kind}"}}}},limit=1] Motion[1] 1000',label+'-hull-motion-y',admission=False)
    cmd(f'/data get entity @e[type=item,nbt={{Item:{{id:"minecraft:{kind}"}}}},limit=1]',label+'-hull-full',admission=False,diagnostic=True)
 # VehicleEntity.destroy's spawnAtLocation: the cart item has an ItemEntity's own motion, 0.2 up (X/Z random).
 cmd('/data get entity @e[type=item,nbt={Item:{id:"minecraft:minecart"}},limit=1] Motion[1] 1000','minecart-hull-motion-y',200,admission=False)
 cmd('/data get entity @e[type=item,nbt={Item:{id:"minecraft:minecart"}},limit=1] PickupDelay','minecart-hull-delay',10,admission=False)
 # AbstractMinecartContainer.remove: Containers.dropContents scatters the contents from a spot in the
 # cart's block, in pieces of 10 to 30, with no pickup delay. Spots and motions are random; these are not.
 u=str(uuid.UUID(int=(500<<96)|8));items='[{Slot:0b,id:"minecraft:diamond",count:5},{Slot:1b,id:"minecraft:dirt",count:40}]'
 cmd('/execute if block 3 199 13 stone','chest-cart-support',1)
 cmd(f'/summon chest_minecart 3.5 200 13.5 {{UUID:[I;500,0,0,8],NoGravity:1b,Motion:[0d,0d,0d],Items:{items}}}','chest-cart-summon',1)
 cmd(f'/execute as {u} if entity @s[type=chest_minecart,nbt={{Items:{items}}}]','chest-cart-actor',1)
 action(f'damage {u} 5 minecraft:generic','chest-cart-break')
 cmd(f'/execute unless entity {u}','chest-cart-removed',admission=False)
 cmd('/execute if entity @e[type=item,nbt={Item:{id:"minecraft:chest_minecart",count:1}}]','chest-cart-hull',admission=False)
 cmd('/data get entity @e[type=item,nbt={Item:{id:"minecraft:chest_minecart"}},limit=1] Motion[1] 1000','chest-cart-hull-motion-y',admission=False)
 cmd('/data get entity @e[type=item,nbt={Item:{id:"minecraft:chest_minecart"}},limit=1] PickupDelay','chest-cart-hull-delay',admission=False)
 cmd('/execute if entity @e[type=item,nbt={Item:{id:"minecraft:diamond",count:5}},x=3,y=200,z=13,dx=0,dy=0,dz=0]','chest-cart-diamonds',admission=False)
 cmd('/data get entity @e[type=item,nbt={Item:{id:"minecraft:diamond"}},limit=1] PickupDelay','chest-cart-diamonds-delay',admission=False)
 cmd('/execute if entity @e[type=item,nbt={Item:{id:"minecraft:dirt",count:40}}]','chest-cart-dirt-split',admission=False)
 dirt('x=3,y=200,z=13,dx=0,dy=0,dz=0','chest-cart')
 # AbstractChestBoat.remove: the same Containers.dropContents scatter for a destroyed chest boat;
 # its hull item still comes from VehicleEntity.destroy's spawnAtLocation.
 u=str(uuid.UUID(int=(500<<96)|9))
 cmd('/execute if block 7 199 13 stone','chest-boat-support',1)
 cmd(f'/summon oak_chest_boat 7.5 200 13.5 {{UUID:[I;500,0,0,9],NoGravity:1b,Motion:[0d,0d,0d],Items:{items}}}','chest-boat-summon',1)
 cmd(f'/execute as {u} if entity @s[type=oak_chest_boat,nbt={{Items:{items}}}]','chest-boat-actor',1)
 action(f'damage {u} 5 minecraft:generic','chest-boat-break')
 cmd(f'/execute unless entity {u}','chest-boat-removed',admission=False)
 cmd('/execute if entity @e[type=item,nbt={Item:{id:"minecraft:oak_chest_boat",count:1}}]','chest-boat-hull',admission=False)
 cmd('/data get entity @e[type=item,nbt={Item:{id:"minecraft:oak_chest_boat"}},limit=1] PickupDelay','chest-boat-hull-delay',admission=False)
 near='x=7,y=200,z=13,dx=0,dy=0,dz=0'
 cmd(f'/execute if entity @e[type=item,nbt={{Item:{{id:"minecraft:diamond",count:5}}}},{near}]','chest-boat-diamonds',admission=False)
 cmd(f'/data get entity @e[type=item,nbt={{Item:{{id:"minecraft:diamond"}}}},{near},limit=1] PickupDelay','chest-boat-diamonds-delay',admission=False)
 cmd(f'/execute if entity @e[type=item,nbt={{Item:{{id:"minecraft:dirt",count:40}}}},{near}]','chest-boat-dirt-split',admission=False)
 dirt(near,'chest-boat')
 # The contents spill whatever entity_drops says, on the death path and on a creative player's
 # discard alike; only the cart item (and chestVehicleDestroyed's second, empty spill) follow the rule.
 cmd('/gamerule entity_drops false','drops-off')
 for name,i,z,source in [('no-drops-cart',10,13,''),('creative-cart',11,11,' by @s')]:
  u=str(uuid.UUID(int=(500<<96)|i));near=f'x=11,y=200,z={z},dx=0,dy=0,dz=0'
  cmd(f'/execute if block 11 199 {z} stone',name+'-support',1)
  cmd(f'/summon chest_minecart 11.5 200 {z}.5 {{UUID:[I;500,0,0,{i}],NoGravity:1b,Motion:[0d,0d,0d],Items:{items}}}',name+'-summon',1)
  cmd(f'/execute as {u} if entity @s[type=chest_minecart,nbt={{Items:{items}}}]',name+'-actor',1)
  action(f'damage {u} '+('1' if source else '5')+' minecraft:generic'+source,name+'-break')
  cmd(f'/execute unless entity {u}',name+'-removed',admission=False)
  cmd(f'/execute if entity @e[type=item,nbt={{Item:{{id:"minecraft:diamond",count:5}}}},{near}]',name+'-diamonds',admission=False)
  cmd(f'/data get entity @e[type=item,nbt={{Item:{{id:"minecraft:diamond"}}}},{near},limit=1] PickupDelay',name+'-diamonds-delay',admission=False)
  dirt(near,name)
  # Only the first chest minecart's item, from the break above with entity_drops on.
  cmd('/execute if entity @e[type=item,nbt={Item:{id:"minecraft:chest_minecart"}}]',name+'-no-hull',admission=False)
 cmd('/gamerule entity_drops true','drops-restored')
 cmd('/summon pig 13.5 200 6.5 {UUID:[I;500,0,0,7],Health:10f,NoAI:1b,NoGravity:1b}','pig-summon',1)
 pig=str(uuid.UUID(int=(500<<96)|7))
 cmd(f'/execute as {pig} if entity @s[type=pig,nbt={{Health:10f,NoAI:1b,NoGravity:1b}}]','pig-before',1)
 action(f'damage {pig} 1 minecraft:generic','pig-hit')
 cmd(f'/data get entity {pig} Health','pig-health',9)
 cmd('/execute store result score end h050 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h050 -= start h050','clock-delta-operation',0)
 cmd('/scoreboard players get end h050','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='vehicle-damage-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
         labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete records, no masks/tolerances; bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Vehicle generic command damage', ops)]
    sys.argv.append('--allow-gaps'); driver.main()
    java_bytes = (build/'vanilla.jsonl').read_bytes()
    if expected_bytes is not None:
        assert java_bytes==expected_bytes and reference.read_bytes()==expected_bytes, 'fresh Java reference changed'
    java = [json.loads(s) for s in java_bytes.splitlines()]
    if '--reference-only' in sys.argv: return
    native = [json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded)==len(java)==len(native)
    residuals = [dict(label=op['label'], expected=a, actual=b) for op,a,b in zip(recorded,java,native) if a!=b]
    report = dict(records=len(recorded), passed=len(recorded)-len(residuals), residuals=residuals,
                  reference_sha256=hashlib.sha256(java_bytes).hexdigest())
    (build/'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
    for op,a,b in zip(recorded,java,native):
        if op['admission']: assert a==b, ('admission',op,a,b)
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals: raise SystemExit(1)


if __name__ == '__main__':
    main()
