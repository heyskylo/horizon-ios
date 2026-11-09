#!/usr/bin/env python3
"""Full Mob command equipment adoption, saved projection and actual slot copying."""
import hashlib,json,sys,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
CASES=[('offhand_custom','weapon.offhand','offhand','stick[custom_data={audit:7}]',3),
 ('head_custom','armor.head','head','iron_helmet[damage=3,custom_data={audit:8}]',1),
 ('offhand_plain','weapon.offhand','offhand','stick',1),
 ('head_plain','armor.head','head','iron_helmet',1),
 ('mainhand_custom','weapon.mainhand','mainhand','diamond[custom_data={audit:9}]',3)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 def action(text,label):
  for key in ['result','success']:cmd(f'/scoreboard players set {key} h023 -99',label+'-reset-'+key,-99)
  cmd('/execute store result score result h023 store success score success h023 run '+text,label,admission=False)
  for key in ['result','success']:cmd(f'/scoreboard players get {key} h023',label+'-'+key,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-place',1)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
 cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1)
 cmd('/data get block 4 200 4 id','chest-id',15)
 cmd('/scoreboard objectives add h023 dummy','objective')
 cmd('/execute store result score start h023 run time query gametime','clock-start',diagnostic=True)
 for i,(label,slot,key,item,count) in enumerate(CASES,1):
  u=str(uuid.UUID(int=(2300<<96)|i));x=2+i
  cmd(f'/summon husk {x}.5 200 7.5 {{UUID:[I;2300,0,0,{i}],Health:20f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b}}',label+'-summon',1)
  guard=f'{{UUID:[I;2300,0,0,{i}],Health:20f,Pos:[{x}.5d,200d,7.5d],NoAI:1b,NoGravity:1b}}'
  cmd(f'/execute as {u} if entity @s[type=husk,nbt='+guard+']',label+'-actor',1)
  cmd(f'/data get entity {u} UUID',label+'-uuid',4)
  cmd(f'/data get entity {u} Pos',label+'-pos',3)
  cmd(f'/data get entity {u} equipment.{key}',label+'-initial-empty')
  cmd('/item replace block 4 200 4 container.0 with emerald 4',label+'-empty-copy-reset',1)
  action(f'item replace block 4 200 4 container.0 from entity {u} {slot}',label+'-empty-copy')
  cmd('/data get block 4 200 4 Items[{Slot:0b}]',label+'-empty-copy-stack')
  other_slot,other_key,other_item=('weapon.offhand','offhand','stick') if key=='mainhand' else ('weapon.mainhand','mainhand','stone')
  cmd(f'/item replace entity {u} {other_slot} with {other_item}',label+'-unrelated-place',1)
  cmd(f'/data get entity {u} equipment.{other_key}',label+'-unrelated-initial')
  cmd(f'/item replace block 4 200 4 container.2 with {item} {count}',label+'-input-admission',1)
  cmd('/data get block 4 200 4 Items[{Slot:2b}]',label+'-valid-input-stack')
  action(f'item replace entity {u} {slot} with {item} {count}',label+'-write')
  cmd(f'/data get entity {u} equipment.{key}',label+'-saved-stack',admission=False)
  cmd(f'/data get entity {u} equipment.{key}.count',label+'-saved-count',admission=False)
  cmd(f'/data get entity {u} equipment.{key}.components',label+'-saved-components',admission=False)
  cmd('/item replace block 4 200 4 container.0 with emerald 4',label+'-copy-reset',1)
  action(f'item replace block 4 200 4 container.0 from entity {u} {slot}',label+'-copy')
  cmd('/data get block 4 200 4 Items[{Slot:0b}]',label+'-copied-stack',admission=False)
  cmd('/data get block 4 200 4 Items[{Slot:0b}].count',label+'-copied-count',admission=False)
  cmd('/data get block 4 200 4 Items[{Slot:0b}].components',label+'-copied-components',admission=False)
  cmd(f'/data get entity {u} equipment.{other_key}',label+'-unrelated-retained')
  cmd(f'/execute as {u} if entity @s[type=husk,nbt='+guard+']',label+'-actor-retained',1)
  cmd(f'/data get entity {u}',label+'-full-body',diagnostic=True)
 cmd('/execute store result score end h023 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h023 -= start h023','clock-delta-operation',0)
 cmd('/scoreboard players get end h023','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='mob-stack-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Mob equipment command stacks', ops)]
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
