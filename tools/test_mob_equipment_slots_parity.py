#!/usr/bin/env python3
"""Actual Mob command body/saddle access with independent saved equipment controls."""
import hashlib,json,sys,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
CASES=[('husk_body','husk',20,'armor.body','diamond_chestplate'),
 ('pig_body','pig',10,'armor.body','diamond_chestplate'),
 ('horse_body','horse',20,'armor.body','iron_horse_armor'),
 ('wolf_body','wolf',8,'armor.body','wolf_armor'),
 ('husk_head','husk',20,'armor.head','iron_helmet'),
 ('husk_saddle','husk',20,'saddle','saddle'),
 ('pig_saddle','pig',10,'saddle','saddle')]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/setblock 4 200 4 chest[facing=north,type=single,waterlogged=false]','chest-place',1)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
 cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1)
 cmd('/data get block 4 200 4 id','chest-id',15)
 cmd('/scoreboard objectives add h025 dummy','objective')
 cmd('/execute store result score start h025 run time query gametime','clock-start',diagnostic=True)
 for i,(label,kind,hp,slot,item) in enumerate(CASES,1):
  u=str(uuid.UUID(int=(2500<<96)|i));x=1+i;y=200;z=5
  nbt=f'{{UUID:[I;2500,0,0,{i}],NoAI:1b,NoGravity:1b,Health:{hp}f,PersistenceRequired:1b}}'
  cmd(f'/summon minecraft:{kind} {x}.5 {y} {z}.5 '+nbt,label+'-summon',1)
  guard=f'{{UUID:[I;2500,0,0,{i}],Health:{hp}f,Pos:[{x}.5d,200d,5.5d],NoAI:1b,NoGravity:1b}}'
  cmd(f'/execute as {u} if entity @s[type=minecraft:{kind},nbt='+guard+']',label+'-actor',1)
  cmd(f'/data get entity {u} UUID',label+'-uuid',4)
  cmd(f'/data get entity {u} Pos',label+'-pos',3)
  for field in ['head','body','saddle']:
   cmd(f'/data get entity {u} equipment.{field}',label+'-initial-'+field)
  control_slot,control_key,control_item=('weapon.offhand','offhand','stick') if slot=='armor.head' else ('armor.head','head','stone')
  cmd(f'/item replace entity {u} {control_slot} with {control_item}',label+'-control-place',1)
  cmd(f'/data get entity {u} equipment.{control_key}',label+'-control-initial')
  for key in ['result','success']:cmd(f'/scoreboard players set {key} h025 -99',label+'-reset-'+key,-99)
  cmd(f'/execute store result score result h025 store success score success h025 run item replace entity {u} {slot} with {item}',label+'-replace',admission=False)
  for key in ['result','success']:cmd(f'/scoreboard players get {key} h025',label+'-'+key,admission=False)
  for field in ['head','body','saddle','offhand']:
   cmd(f'/data get entity {u} equipment.{field}',label+'-saved-'+field,admission=field==control_key)
  cmd('/item replace block 4 200 4 container.0 with emerald 4',label+'-copy-reset',1)
  cmd(f'/item replace block 4 200 4 container.0 from entity {u} {slot}',label+'-slot-read',admission=False)
  cmd('/data get block 4 200 4 Items[{Slot:0b}]',label+'-copied-stack',admission=False)
  cmd(f'/execute as {u} if entity @s[type=minecraft:{kind},nbt='+guard+']',label+'-actor-retained',1)
  cmd(f'/data get entity {u}',label+'-full-body',diagnostic=True)
 cmd('/execute store result score end h025 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h025 -= start h025','clock-delta-operation',0)
 cmd('/scoreboard players get end h025','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='mob-equipment-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Mob command equipment slots', ops)]
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
