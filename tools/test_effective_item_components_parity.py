#!/usr/bin/env python3
"""Actual item command effective damageable/stackable validation (Java 26.3).

Default: the predeclared139 complete-record gate. --full-removal-diagnostic
restores the original141 including two separate saved-removal observations.
Use --reference-only to capture fresh Java, then --reference FILE --binary FILE
for frozen old/fixed runs. Scratch output is retained under --work-dir.
"""
import hashlib,json,sys,tempfile
from pathlib import Path
import test_command_exec_parity as driver
FULL_REMOVAL = '--full-removal-diagnostic' in sys.argv
if FULL_REMOVAL: sys.argv.remove('--full-removal-diagnostic')
CASES=[('helmet-two','iron_helmet[max_stack_size=2,custom_data={keep:86}]'),('helmet-max','iron_helmet[max_stack_size=99,custom_data={keep:86}]'),('helmet-plain','iron_helmet'),('stone-two','stone[max_stack_size=2,custom_data={keep:86}]'),('removed-two','iron_helmet[!max_damage,max_stack_size=2,custom_data={keep:86}]'),('added-damage','stone[max_damage=10,max_stack_size=2,custom_data={keep:86}]'),('stone-max','stone[max_stack_size=99,custom_data={keep:86}]')]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-place',1)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1);cmd('/execute if block 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-state',1)
 for key,value in [('id',15),('x',4),('y',200),('z',4),('Items',0)]:cmd('/data get block 4 200 4 '+key,'chest-'+key,value)
 cmd('/item replace block 4 200 4 container.1 with stone[custom_data={keep:1}] 7','unrelated-place',1)
 cmd('/scoreboard objectives add h086 dummy','objective');cmd('/execute store result score start h086 run time query gametime','clock-start',diagnostic=True)
 for label,item in CASES:
  path='Items[{Slot:0b}]'
  cmd('/item replace block 4 200 4 container.0 with emerald[custom_data={sentinel:86}] 4',label+'-reset-target',1)
  cmd('/execute if data block 4 200 4 {Items:[{Slot:0b,id:"minecraft:emerald",count:4,components:{"minecraft:custom_data":{sentinel:86}}}]}',label+'-initial-sentinel',1)
  cmd('/data get block 4 200 4 '+path,label+'-initial-stack')
  for key in ['result','success']:cmd(f'/scoreboard players set {key} h086 -99',label+'-reset-'+key,-99)
  cmd(f'/execute store result score result h086 store success score success h086 run item replace block 4 200 4 container.0 with minecraft:{item} 1',label+'-replace',admission=False)
  for key in ['result','success']:cmd(f'/scoreboard players get {key} h086',label+'-'+key,admission=False)
  for field in ['','id','count','components."minecraft:max_stack_size"','components."minecraft:max_damage"','components."!minecraft:max_damage"','components."minecraft:custom_data"']:
   cmd('/data get block 4 200 4 '+path+('.'+field if field else ''),label+'-saved-'+(field or 'stack'),admission=False)
  cmd('/execute if data block 4 200 4 {Items:[{Slot:0b,id:"minecraft:emerald",count:4,components:{"minecraft:custom_data":{sentinel:86}}}]}',label+'-sentinel-retained',admission=False)
  cmd('/data get block 4 200 4 Items[{Slot:1b}]',label+'-unrelated')
  # Existing predicate route is a retained effective-component diagnostic,
  # not a fabricated fallback if its component/default view differs.
  cmd('/execute if items block 4 200 4 container.0 *[minecraft:max_damage]',label+'-effective-damage-presence',diagnostic=True)
  cmd('/data get block 4 200 4 Items',label+'-full-inventory',diagnostic=True)
 cmd('/execute store result score end h086 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h086 -= start h086','clock-delta-operation',0);cmd('/scoreboard players get end h086','clock-delta',0)
 for op in ops:
  if not FULL_REMOVAL and op.get('label') in {'removed-two-saved-stack','removed-two-saved-components."!minecraft:max_damage"'}:
   op.update(record=False,trace=True)
 assert sum(op['op']=='cmd' and op.get('record',True) for op in ops)==(141 if FULL_REMOVAL else 139)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='effective-item-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Effective item components', ops)]
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
