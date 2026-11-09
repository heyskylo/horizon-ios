#!/usr/bin/env python3
"""ItemEntity saved max-stack default patches; zero observation ticks."""
import hashlib,json,sys,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
CASES=[('helmet-plain','iron_helmet',None),('helmet-default','iron_helmet','{"minecraft:max_stack_size":1}'),('helmet-keep','iron_helmet','{"minecraft:max_stack_size":1,"minecraft:custom_data":{keep:88}}'),('stone-plain','stone',None),('stone-default','stone','{"minecraft:max_stack_size":64}'),('stone-nondefault','stone','{"minecraft:max_stack_size":16}'),('stone-removed','stone','{"!minecraft:max_stack_size":{}}'),('pearl-plain','ender_pearl',None),('pearl-default','ender_pearl','{"minecraft:max_stack_size":16}'),('pearl-nondefault','ender_pearl','{"minecraft:max_stack_size":8}')]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 def read(text,label,admission=False):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h088 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h088 store success score success h088 run '+text,label,admission=admission)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h088',label+'-'+k,admission=admission)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
 cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/scoreboard objectives add h088 dummy','objective')
 cmd('/execute store result score start h088 run time query gametime','clock-start',diagnostic=True)
 for n,(label,item,components) in enumerate(CASES,1):
  u=str(uuid.UUID(int=(880<<96)|n));x=2+n
  cmd(f'/execute if loaded {x} 200 4',label+'-load',1)
  cmd(f'/execute if block {x} 199 4 stone',label+'-floor',1)
  stack='{id:"minecraft:'+item+'",count:1'+(',components:'+components if components else '')+'}'
  cmd(f'/summon item {x}.5 200 4.5 '+f'{{UUID:[I;880,0,0,{n}],NoGravity:1b,Motion:[0d,0d,0d],Health:5s,Item:'+stack+'}',label+'-summon',1)
  guard=f'{{UUID:[I;880,0,0,{n}],Pos:[{x}.5d,200d,4.5d],Motion:[0d,0d,0d],NoGravity:1b,Health:5s,Item:{{id:"minecraft:{item}",count:1}}}}'
  cmd(f'/execute as {u} if entity @s[type=item,nbt='+guard+']',label+'-actor',1)
  for f in ['UUID','Pos','Motion','NoGravity','Health']:cmd(f'/data get entity {u} {f}',label+'-'+f)
  read(f'data get entity {u} Item',label+'-saved-stack')
  read(f'data get entity {u} Item.components."minecraft:max_stack_size"',label+'-saved-size')
  read(f'execute if data entity {u} Item.components."minecraft:max_stack_size"',label+'-size-presence')
  read(f'data get entity {u} Item.components."!minecraft:max_stack_size"',label+'-removal')
  read(f'data get entity {u} Item.components."minecraft:custom_data"',label+'-custom')
  cmd(f'/data get entity {u}',label+'-full',diagnostic=True)
 cmd('/execute store result score end h088 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h088 -= start h088','clock-delta-operation',0)
 cmd('/scoreboard players get end h088','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='item-max-stack-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('ItemEntity saved max-stack prototype', ops)]
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
