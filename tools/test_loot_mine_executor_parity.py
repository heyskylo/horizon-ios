#!/usr/bin/env python3
"""Actual player/stand/server sources through loot-mine commands (Java26.3).

The existing command harness's explicit source=console input selects actual
server source factories, preserving its existing feedback capture. Default
commands remain owner/player commands. Use --reference-only, then --reference
FILE --binary FILE to keep a frozen reference and complete raw records.
"""
import hashlib,json,sys,tempfile
from pathlib import Path
import test_command_exec_parity as driver
CASES=[(source,block) for source in ['player','stand','console'] for block in ['snow','dirt']]
STAND='0000003c-0000-0000-0000-000000000001'
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False,source=None):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  if source is not None:op['source']=source
  ops.append(op)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight');cmd('/gamerule random_tick_speed 0','no-random')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','default-owner-is-player',1)
 cmd('/data get entity @s UUID','default-owner-uuid',4)
 cmd('/summon armor_stand 10.5 200 7.5 {UUID:[I;60,0,0,1],Health:20f,NoGravity:1b}','stand-summon',1)
 cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-place',1)
 cmd('/execute if block 6 200 4 air','snow-initial-air',1);cmd('/setblock 6 200 4 snow[layers=1]','snow-place',1)
 cmd('/execute if block 8 200 4 air','dirt-initial-air',1);cmd('/setblock 8 200 4 dirt','dirt-place',1)
 for label,pos in [('chest',4),('snow',6),('dirt',8)]:
  cmd(f'/execute if loaded {pos} 200 4',label+'-loaded',1);cmd(f'/execute if block {pos} 199 4 stone',label+'-floor',1)
 cmd('/execute if block 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-state',1)
 for field,expect in [('id',15),('x',4),('y',200),('z',4),('Items',0)]:cmd('/data get block 4 200 4 '+field,'chest-'+field,expect)
 cmd('/item replace block 4 200 4 container.1 with diamond 3','unrelated-place',1)
 cmd('/item replace block 4 200 4 container.2 with diamond_shovel','tool-place',1)
 cmd('/data get block 4 200 4 Items[{Slot:2b}]','actual-tool-stack')
 cmd('/execute if items block 4 200 4 container.2 #minecraft:shovels','actual-tool-tag',1)
 cmd('/scoreboard objectives add h060 dummy','objective')
 cmd('/execute store result score start h060 run time query gametime','clock-start',diagnostic=True)
 cmd('/execute unless entity @s','console-has-no-entity',1,source='console')
 cmd('/execute if entity Player','console-still-sees-real-player',1,source='console')
 for source,block in CASES:
  label=source+'-'+block;x=6 if block=='snow' else 8;state='snow[layers=1]' if block=='snow' else 'dirt'
  cmd(f'/execute if block {x} 200 4 {state}',label+'-source-block',1,source='console')
  cmd('/execute unless entity @s',label+'-outer-no-entity',1,source='console')
  prefix='execute as Player run ' if source=='player' else f'execute as {STAND} run ' if source=='stand' else ''
  if source=='player':
   cmd('/execute as Player if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]',label+'-executor-body',1,source='console')
   cmd('/execute as Player run data get entity @s UUID',label+'-executor-uuid',4,source='console')
  elif source=='stand':
   cmd(f'/execute as {STAND} if entity @s[type=armor_stand,nbt={{Health:20f,NoGravity:1b,Pos:[10.5d,200d,7.5d]}}]',label+'-executor-body',1,source='console')
   cmd(f'/execute as {STAND} run data get entity @s UUID',label+'-executor-uuid',4,source='console')
  else:
   cmd('/execute unless entity @s',label+'-executor-none',1,source='console')
  cmd('/item replace block 4 200 4 container.0 with emerald 4',label+'-reset-target',1,source='console')
  cmd('/execute if data block 4 200 4 {Items:[{Slot:0b,id:"minecraft:emerald",count:4}]}',label+'-initial-sentinel',1,source='console')
  cmd('/data get block 4 200 4 Items',label+'-initial-inventory',source='console')
  for key in ['result','success']:cmd(f'/scoreboard players set {key} h060 -99',label+'-reset-'+key,-99,source='console')
  cmd('/execute store result score result h060 store success score success h060 run '+prefix+f'loot replace block 4 200 4 container.0 mine {x} 200 4 diamond_shovel',label+'-mine',admission=False,source='console')
  for key in ['result','success']:cmd(f'/scoreboard players get {key} h060',label+'-'+key,admission=False,source='console')
  for field in ['Items','Items[{Slot:0b}]','Items[{Slot:1b}]','Items[{Slot:2b}]']:
   cmd('/data get block 4 200 4 '+field,label+'-saved-'+field,admission=False,source='console')
  cmd(f'/execute if block {x} 200 4 {state}',label+'-block-unchanged',1,source='console')
  cmd(f'/data get entity {STAND}',label+'-stand-full',diagnostic=True)
 cmd('/execute store result score end h060 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h060 -= start h060','clock-delta-operation',0);cmd('/scoreboard players get end h060','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='loot-mine-source-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Loot mine executor presence', ops)]
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
