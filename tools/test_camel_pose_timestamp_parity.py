#!/usr/bin/env python3
"""Camel pose timestamp load/save at an admitted shared clock, then one tick.

Existing command harness: --reference-only captures fresh Java; --reference FILE
and --binary FILE compare a frozen Game build. Every run keeps raw clock
alignment, full bodies/motion, exact command feedback and comparison residuals.
"""
import hashlib,json,sys,tempfile
from pathlib import Path
import test_command_exec_parity as driver
CASES=[('absent',None,0),('zero',0,0),('past',1000,1000),('sitting',-1000,-1000),('future',1044,1044)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight');cmd('/gamerule random_tick_speed 0','no-random')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/fill 0 200 0 15 205 15 barrier','clear-prefill',1536);cmd('/fill 0 200 0 15 205 15 air','clear',1536)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1)
 cmd('/scoreboard objectives add h094 dummy','objective')
 cmd('/execute store result score before h094 run time query gametime','clock-before-align',diagnostic=True)
 cmd('/execute if score before h094 matches 0..1023','clock-align-admission',1)
 for n in range(1024):
  cmd('/execute store result score align h094 run time query gametime',f'align-{n}-read',diagnostic=True)
  cmd('/execute if score align h094 matches ..1023 run tick step 1',f'align-{n}-step',diagnostic=True)
  ops.append(dict(op='tick',n=1))
 cmd('/time query gametime','clock-common',1024)
 cmd('/execute store result score start h094 run time query gametime','clock-start',1024)
 for n,(label,tick,expected) in enumerate(CASES,1):
  x=2.5+(n-1)*2.5;uid=f'0000005e-0000-0000-0000-{n:012x}'
  cmd(f'/execute if block {int(x)} 199 4 stone',label+'-floor',1)
  cmd(f'/execute if block {int(x)} 200 4 air',label+'-air',1)
  tag=f'{{UUID:[I;94,0,0,{n}],NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Health:32f,Tags:["h094"]'+(f',LastPoseTick:{tick}L' if tick is not None else '')+'}'
  cmd(f'/summon camel {x} 200 4.5 {tag}',label+'-summon',1)
 def observe(sample):
  cmd('/time query gametime',f'{sample}-clock-absolute',1024+sample)
  cmd('/execute store result score elapsed h094 run time query gametime',f'{sample}-clock-read',1024+sample)
  cmd('/scoreboard players operation elapsed h094 -= start h094',f'{sample}-clock-delta',sample)
  for n,(label,tick,expected) in enumerate(CASES,1):
   x=2.5+(n-1)*2.5;uid=f'0000005e-0000-0000-0000-{n:012x}';stem=f'{label}-{sample}'
   cmd(f'/execute if entity @e[type=camel,nbt={{UUID:[I;94,0,0,{n}],Health:32f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Pos:[{x}d,200d,4.5d],Tags:["h094"]}}]',stem+'-body-admission',1)
   cmd(f'/data get entity {uid} UUID',stem+'-uuid',4)
   cmd(f'/execute if data entity {uid} LastPoseTick',stem+'-presence',1)
   for key in ['result','success']:cmd(f'/scoreboard players set {key} h094 -99',stem+'-reset-'+key,-99)
   cmd(f'/execute store result score result h094 store success score success h094 run data get entity {uid} LastPoseTick',stem+'-tick',admission=False)
   for key in ['result','success']:cmd(f'/scoreboard players get {key} h094',stem+'-'+key,admission=False)
   cmd(f'/execute if entity @e[type=camel,nbt={{UUID:[I;94,0,0,{n}],LastPoseTick:{expected}L}}]',stem+'-typed-value',admission=False)
   for field in ['NoAI','NoGravity','PersistenceRequired','Health']:
    cmd(f'/data get entity {uid} {field}',stem+'-'+field)
   cmd(f'/data get entity {uid}',stem+'-full-body',diagnostic=True)
   cmd(f'/data get entity {uid} Motion',stem+'-motion',diagnostic=True)
 observe(0)
 cmd('/tick step 1','actor-step',1);ops.append(dict(op='tick',n=1));observe(1)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='camel-pose-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=CASES, labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete records, no masks/tolerances; common1024/1025 clocks strict; pre-alignment clocks, bodies and motion are separate traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Camel loaded pose timestamp', ops)]
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
