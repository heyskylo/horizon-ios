#!/usr/bin/env python3
"""Live TNT data adoption with independent stationary inputs and unchanged controls."""
import hashlib,json,sys,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
CASES=[('fuse',1,'{fuse:40s}','{fuse:60s}'),('motion',2,'{Motion:[0d,0d,0d]}','{Motion:[0.125d,0d,-0.25d]}')]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 def action(text,label,admission=False):
  for key in ['result','success']:cmd(f'/scoreboard players set {key} h089 -99',label+'-reset-'+key,-99)
  cmd('/execute store result score result h089 store success score success h089 run '+text,label,admission=admission)
  for key in ['result','success']:cmd(f'/scoreboard players get {key} h089',label+'-'+key,admission=admission)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
 cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1)
 cmd('/scoreboard objectives add h089 dummy','objective')
 cmd('/execute store result score start h089 run time query gametime','clock-start',diagnostic=True)
 pig=str(uuid.UUID(int=(890<<96)|3))
 cmd('/summon pig 11.5 200 5.5 {UUID:[I;890,0,0,3],Health:10f,NoAI:1b,NoGravity:1b}','pig-summon',1)
 pig_guard='{UUID:[I;890,0,0,3],Health:10f,Pos:[11.5d,200d,5.5d],NoAI:1b,NoGravity:1b}'
 cmd(f'/execute as {pig} if entity @s[type=pig,nbt='+pig_guard+']','pig-actor',1)
 cmd(f'/data get entity {pig} UUID','pig-uuid',4)
 cmd(f'/data get entity {pig}','pig-before-full',diagnostic=True)
 for label,i,unchanged,update in CASES:
  u=str(uuid.UUID(int=(890<<96)|i));x=3+2*i
  nbt=f'{{UUID:[I;890,0,0,{i}],fuse:40s,NoGravity:1b,Motion:[0d,0d,0d],explosion_power:2f,owner:[I;890,0,0,3],Tags:[keep],Silent:1b}}'
  cmd(f'/summon tnt {x}.5 200 5.5 '+nbt,label+'-summon',1)
  guard=f'{{UUID:[I;890,0,0,{i}],Pos:[{x}.5d,200d,5.5d],NoGravity:1b}}'
  cmd(f'/execute as {u} if entity @s[type=tnt,nbt='+guard+']',label+'-actor',1)
  cmd(f'/execute if block {x} 199 5 stone',label+'-floor',1)
  for field in ['UUID','Pos','Motion','fuse','NoGravity','Rotation','owner','block_state','explosion_power','Tags','Silent']:
   cmd(f'/data get entity {u} {field}',label+'-initial-'+field)
  cmd(f'/data get entity {u}',label+'-initial-full',diagnostic=True)
  action(f'data merge entity {u} '+unchanged,label+'-unchanged',admission=True)
  cmd(f'/data get entity {u} fuse',label+'-unchanged-fuse',40)
  cmd(f'/data get entity {u} Motion',label+'-unchanged-motion',3)
  action(f'data merge entity {u} '+update,label+'-update')
  for field in ['UUID','Pos','Motion','fuse','NoGravity','Rotation','owner','block_state','explosion_power','Tags','Silent']:
   cmd(f'/data get entity {u} {field}',label+'-saved-'+field,admission=field not in ['fuse','Motion'])
  cmd(f'/execute as {u} if entity @s[type=tnt,nbt='+guard+']',label+'-actor-retained',1)
  cmd(f'/data get entity {u}',label+'-saved-full',diagnostic=True)
 action(f'data merge entity {pig} {{Silent:1b}}','pig-update',admission=True)
 cmd(f'/execute as {pig} if entity @s[type=pig,nbt='+pig_guard+']','pig-actor-retained',1)
 cmd(f'/data get entity {pig} Silent','pig-silent',1)
 cmd(f'/data get entity {pig}','pig-after-full',diagnostic=True)
 cmd('/execute store result score end h089 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h089 -= start h089','clock-delta-operation',0)
 cmd('/scoreboard players get end h089','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='tnt-adoption-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=CASES, labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete records, no masks/tolerances; bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Live TNT data adoption', ops)]
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
