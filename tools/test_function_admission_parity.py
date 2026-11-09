#!/usr/bin/env python3
"""Real valid pack files and atomic ordinary-command function admission."""
import hashlib,json,sys,tempfile
from pathlib import Path
import test_command_exec_parity as driver
CASES=['good','other','bad','forbidden','missing']
FUNCTIONS={
 'h144:good':'scoreboard players set good h144 11\n',
 'h144:other':'scoreboard players add other h144 3\n',
 'h144:bad':'scoreboard players set bad h144 1\nh144_unrecognized_command\nscoreboard players set bad h144 2\n',
 'h144:forbidden':'scoreboard players set forbidden h144 1\ntick freeze\nscoreboard players set forbidden h144 2\n',
}
def program():
 ops=[]
 def cmd(text,label,result=None,admission=True,diagnostic=False):
  op=dict(op='cmd',cmd=text,label=label,admission=admission)
  if result is not None:op['expect_result']=result
  if diagnostic:op.update(record=False,trace=True)
  ops.append(op)
 cmd('/tick freeze','owner-can-freeze');cmd('/gamerule spawn_mobs false','no-spawn');cmd('/gamerule advance_time false','no-daylight')
 cmd('/forceload add 0 0','force',1);ops.append(dict(op='settle',required_chunks=[[0,0]]))
 cmd('/fill 0 199 0 15 199 15 stone','floor',256);cmd('/tp @s 4.5 200 4.5 0 0','owner-position',1);cmd('/gamemode creative @s','owner-mode')
 cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[4.5d,200d,4.5d]}]','owner-body',1);cmd('/data get entity @s UUID','owner-uuid',4)
 cmd('/execute if loaded 4 200 4','loaded',1);cmd('/execute if block 4 199 4 stone','support',1)
 cmd('/scoreboard objectives add h144 dummy','objective');cmd('/execute store result score start h144 run time query gametime','clock-start',diagnostic=True)
 cmd('/datapack list enabled','pack-list-initial',diagnostic=True)
 for phase in ['initial','reloaded']:
  if phase=='reloaded':
   for field in ['bad','forbidden','good','other']:cmd(f'/scoreboard players set {field} h144 -99',phase+'-before-reload-'+field,-99)
   cmd('/reload','reload')
   for field in ['bad','forbidden','good','other']:cmd(f'/scoreboard players get {field} h144',phase+'-after-reload-'+field,-99)
   cmd('/datapack list enabled','pack-list-reloaded',diagnostic=True)
  for case in CASES:
   target=case if case!='missing' else 'bad';sentinel=4 if case=='other' else -99;label=phase+'-'+case
   cmd(f'/scoreboard players set {target} h144 {sentinel}',label+'-reset-target',sentinel)
   cmd(f'/scoreboard players get {target} h144',label+'-before',sentinel)
   for key in ['result','success']:cmd(f'/scoreboard players set {key} h144 -99',label+'-reset-'+key,-99)
   cmd(f'/execute store result score result h144 store success score success h144 run function h144:{case}',label+'-call',admission=False)
   for key in ['result','success']:cmd(f'/scoreboard players get {key} h144',label+'-'+key,admission=False)
   expected=11 if case=='good' else 7 if case=='other' else -99
   cmd(f'/scoreboard players get {target} h144',label+'-after',expected,admission=case in ['good','other'])
  cmd('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:1,Pos:[4.5d,200d,4.5d]}]',phase+'-owner-still-valid',1)
 cmd('/execute store result score end h144 run time query gametime','clock-end',diagnostic=True)
 cmd('/scoreboard players operation end h144 -= start h144','clock-delta-operation',0);cmd('/scoreboard players get end h144','clock-delta',0)
 return ops

def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='function-admission-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.function_return_fixtures = lambda names: FUNCTIONS
    driver.corpus_module.groups = lambda: [('Function ordinary command admission', ops)]
    sys.argv.extend(['--allow-gaps','--function-admission-fixture']); driver.main()
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
