#!/usr/bin/env python3
"""Floating zero factories through actual typed storage commands."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [('f_plain_pos', '0.0f', '0.0f', False), ('f_plain_neg', '-0.0f', '0.0f', True), ('f_exp_pos', '0e0f', '0.0f', False), ('f_exp_neg', '-0e0f', '0.0f', True), ('d_plain_pos', '0.0d', '0.0d', False), ('d_plain_neg', '-0.0d', '0.0d', True), ('d_exp_pos', '0e0d', '0.0d', False), ('d_exp_neg', '-0e0d', '0.0d', True), ('bare_plain_pos', '0.0', '0.0d', False), ('bare_plain_neg', '-0.0', '0.0d', True), ('bare_exp_pos', '0e0', '0.0d', False), ('bare_exp_neg', '-0e0', '0.0d', True), ('f_value_pos', '1.25f', '1.25f', False), ('f_value_neg', '-1.25f', '-1.25f', False), ('d_value_pos', '1.25d', '1.25d', False), ('d_value_neg', '-1.25d', '-1.25d', False), ('f_sub_pos', '1.0e-40f', '1.0e-40f', False), ('f_sub_neg', '-1.0e-40f', '-1.0e-40f', False), ('d_sub_pos', '1.0e-310d', '1.0e-310d', False), ('d_sub_neg', '-1.0e-310d', '-1.0e-310d', False)]

def program():
    ops=[]
    def cmd(text,label,result=None,admission=True,diagnostic=False):
        op=dict(op='cmd',cmd=text,label=label,admission=admission)
        if result is not None:op['expect_result']=result
        if diagnostic:op.update(record=False,trace=True)
        ops.append(op)
    def stored(command,label,admission=False):
        cmd('/scoreboard players set result h190 -99',label+'-reset-result',-99)
        cmd('/scoreboard players set success h190 -99',label+'-reset-success',-99)
        cmd('/execute store result score result h190 store success score success h190 run '+command,label,admission=admission)
        cmd('/scoreboard players get result h190',label+'-result',admission=admission)
        cmd('/scoreboard players get success h190',label+'-success',admission=admission)
    cmd('/tick freeze','freeze')
    cmd('/gamerule spawn_mobs false','no-spawn')
    cmd('/gamerule advance_time false','no-daylight')
    cmd('/forceload add 0 0','force',1)
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone','floor',256)
    cmd('/tp @s 14.5 200 14.5 0 0','player-position',1)
    cmd('/gamemode creative @s','player-mode')
    cmd('/execute if loaded 14 200 14','loaded',1)
    cmd('/execute if block 14 199 14 stone','support',1)
    cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','player',1)
    cmd('/data get entity @s UUID','player-uuid')
    cmd('/scoreboard objectives add h190 dummy','objective')
    cmd('/execute store result score start h190 run time query gametime','clock-start',diagnostic=True)
    for storage in ['source','copy']:
        cmd('/data merge storage h190:'+storage+' {anchor:1b}',storage+'-anchor',1)
        cmd('/execute if data storage h190:'+storage+' {anchor:1b}',storage+'-exists',1)
    original='{'+','.join(key+':'+text for key,text,canonical,target in CASES)+'}'
    canonical='{'+','.join(key+':'+value for key,text,value,target in CASES)+'}'
    stored('data modify storage h190:source samples set value '+original,'initial-write',True)
    cmd('/data get storage h190:source','initial-full',diagnostic=True)
    for key,text,value,target in CASES:
        cmd('/data get storage h190:source samples.'+key,key+'-source',admission=not target)
        cmd('/execute if data storage h190:source {samples:{'+key+':'+value+'}}',key+'-source-type',None if target else 1,admission=not target)
    stored('data modify storage h190:copy samples set from storage h190:source samples','copy-write',True)
    for key,text,value,target in CASES:
        cmd('/data get storage h190:copy samples.'+key,key+'-copy',admission=not target)
        cmd('/execute if data storage h190:copy {samples:{'+key+':'+value+'}}',key+'-copy-type',None if target else 1,admission=not target)
    stored('data modify storage h190:copy samples set from storage h190:source samples','duplicate-copy',True)
    stored('data modify storage h190:source samples set value '+canonical,'canonical-no-write')
    cmd('/data get storage h190:source','source-final-full',diagnostic=True)
    cmd('/data get storage h190:copy','copy-final-full',diagnostic=True)
    for storage in ['source','copy']:
        cmd('/execute if data storage h190:'+storage+' {anchor:1b}',storage+'-retained',1)
    cmd('/execute store result score end h190 run time query gametime','clock-end',diagnostic=True)
    cmd('/scoreboard players operation end h190 -= start h190','clock-delta-operation',0)
    cmd('/scoreboard players get end h190','clock-delta',0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='floating-zero-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Text floating zero', ops)]
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
