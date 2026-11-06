#!/usr/bin/env python3
"""Generic no-attacker vehicle damage, exact command records and raw bodies."""
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
    cmd(f'/data get entity @e[type=item,nbt={{Item:{{id:"minecraft:{kind}"}}}},limit=1]',label+'-hull-full',admission=False,diagnostic=True)
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
