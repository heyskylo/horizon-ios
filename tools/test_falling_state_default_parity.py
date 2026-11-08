#!/usr/bin/env python3
"""Falling entity load defaults; rejected admissions never execute dependent data reads."""
import hashlib,json,sys,tempfile,uuid
from pathlib import Path
import test_command_exec_parity as driver
CASES=[('absent',None,True),('sand','"minecraft:sand"',False),('gravel','{id:"minecraft:gravel"}',False),('legacy','{Name:"minecraft:gravel"}',False),('invalid','{}',True)]
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False,**extra):
  op=dict(op='cmd',cmd=text,label=label,admission=admission,**extra)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 def action(text,label):
  for k in ['result','success']:cmd(f'/scoreboard players set {k} h096 -99',label+'-reset-'+k,-99)
  cmd('/execute store result score result h096 store success score success h096 run '+text,label,admission=False)
  for k in ['result','success']:cmd(f'/scoreboard players get {k} h096',label+'-'+k,admission=False)
 cmd('/tick freeze','freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256)
 cmd('/tp @s 14.5 200 14.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','owner',1)
 cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/scoreboard objectives add h096 dummy','objective')
 cmd('/execute store result score start h096 run time query gametime','clock-start',diagnostic=True)
 for n,(label,state,gated) in enumerate(CASES,1):
  u=str(uuid.UUID(int=(960<<96)|n));x=2*n
  cmd(f'/execute if loaded {x} 200 4',label+'-load',1)
  cmd(f'/execute if block {x} 199 4 stone',label+'-floor',1)
  fields=f'UUID:[I;960,0,0,{n}],NoGravity:1b,Motion:[0d,0d,0d],Time:10,DropItem:0b,Silent:1b,Tags:[keep]'+(',BlockState:'+state if state else '')
  action(f'summon falling_block {x}.5 200 4.5 '+'{'+fields+'}',label+'-summon')
  cmd(f'/execute if entity {u}',label+'-present',1,admission=not gated)
  guard=f'{{UUID:[I;960,0,0,{n}],Pos:[{x}.5d,200d,4.5d],Motion:[0d,0d,0d],NoGravity:1b,Time:10,DropItem:0b,Silent:1b,Tags:[keep]}}'
  cmd(f'/execute if entity {u} run execute as {u} if entity @s[type=falling_block,nbt='+guard+']',label+'-actor',1 if not gated else None,diagnostic=gated,**(dict(gated_state=True,actor_label=label+'-present') if gated else {}))
  for f in ['UUID','Pos','Motion','NoGravity','Time','DropItem','HurtEntities','BlockState','Tags','Silent']:
   if gated:
    for k in ['result','success']:cmd(f'/scoreboard players set {k} h096 -99',label+'-'+f+'-reset-'+k,-99)
    cmd(f'/execute if entity {u} run execute store result score result h096 store success score success h096 run data get entity {u} {f}',label+'-saved-'+f,diagnostic=True,gated_state=True,actor_label=label+'-present')
    for k in ['result','success']:
     cmd(f'/execute if entity {u} run scoreboard players get {k} h096',label+'-saved-'+f+'-'+k,diagnostic=True,gated_state=True,actor_label=label+'-present')
   else:action(f'data get entity {u} {f}',label+'-saved-'+f)
  cmd(f'/execute if entity {u} run data get entity {u}',label+'-full',diagnostic=True)
 cmd('/execute store result score end h096 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h096 -= start h096','clock-delta-operation',0)
 cmd('/scoreboard players get end h096','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='falling-state-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Falling entity modern state default', ops)]
    sys.argv.append('--allow-gaps'); driver.main()
    java_bytes = (build/'vanilla.jsonl').read_bytes()
    if expected_bytes is not None:
        assert java_bytes==expected_bytes and reference.read_bytes()==expected_bytes, 'fresh Java reference changed'
    java = [json.loads(s) for s in java_bytes.splitlines()]
    trace_ops=[op for op in ops if op.get('trace')]
    def trace_records(name):
        rows=[json.loads(line.split('command trace: ',1)[1]) for line in (build/name).read_text().splitlines() if 'command trace: ' in line]
        assert len(rows)==len(trace_ops)
        assert all(op['cmd']==row['cmd'] for op,row in zip(trace_ops,rows))
        return {op['label']:row for op,row in zip(trace_ops,rows)}
    java_traces=trace_records('java.log')
    for op in ops:
        if op.get('label','').endswith('-actor') and op.get('trace'):
            assert java_traces[op['label']]['results']==[[True,1]], ('Java actor admission',op)
        if op.get('gated_state') and op['label'].endswith('-BlockState'):
            r=java_traces[op['label']]
            assert r['results']==[[True,14]] and ''.join(s[0] for m in r['messages'] for s in m)=='Falling Sand has the following entity data: "minecraft:sand"', r
    if '--reference-only' in sys.argv: return
    native = [json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded)==len(java)==len(native)
    residuals = [dict(label=op['label'], expected=a, actual=b) for op,a,b in zip(recorded,java,native) if a!=b]
    report = dict(records=len(recorded), passed=len(recorded)-len(residuals), residuals=residuals,
                  reference_sha256=hashlib.sha256(java_bytes).hexdigest())
    (build/'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
    for op,a,b in zip(recorded,java,native):
        if op['admission']: assert a==b, ('admission',op,a,b)
    traces=[java_traces,trace_records('rust.log')]
    state=[]
    for op in ops:
        if not op.get('gated_state'): continue
        actor=op['actor_label']; index=next(i for i,o in enumerate(recorded) if o['label']==actor)
        admitted=native[index]['results']==[[True,1]]
        a,b=(traces[i][op['label']] for i in [0,1])
        state.append(dict(label=op['label'],admitted=admitted,expected=a,actual=b,match=(a==b) if admitted else None))
    phase=dict(records=len(state),admitted=sum(x['admitted'] for x in state),passed=sum(x['match'] is True for x in state),records_detail=state)
    (build/'gated-state-report.json').write_text(json.dumps(phase,indent=2)+'\n')
    assert all(x['match'] for x in state if x['admitted']), 'admitted conditional state mismatch'
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals: raise SystemExit(1)


if __name__ == '__main__':
    main()
