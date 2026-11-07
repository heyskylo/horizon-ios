#!/usr/bin/env python3
"""Inline slot-range name admission through actual item fill."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [('first-populated','container.0',False),('first-empty','container.0',True),('invalid99','container.99',False),('invalid54','container.54',False),('wildcard','container.*',False),('global-last','container.53',False)]
POPULATED = '[{Slot:0b,id:"minecraft:stone",count:3},{Slot:5b,id:"minecraft:apple",count:2}]'
EMPTY_ZERO = '[{Slot:5b,id:"minecraft:apple",count:2}]'
CONTROL = '[{Slot:0b,id:"minecraft:stick",count:3}]'

def program():
    ops = []
    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if result is not None: op['expect_result'] = result
        if diagnostic: op.update(record=False, trace=True)
        ops.append(op)
    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 14.5 200 14.5 0 0', 'player-position', 1)
    cmd('/gamemode creative @s', 'player-mode')
    for condition,label in [('loaded 4 200 4','loaded-target'),('loaded 6 200 4','loaded-control'),('block 4 199 4 stone','target-support'),('block 6 199 4 stone','control-support'),('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','player')]:
        cmd('/execute if '+condition,label,1)
    cmd('/setblock 4 200 4 chest{Items:'+POPULATED+'}', 'place-target', 1)
    cmd('/setblock 6 200 4 chest{Items:'+CONTROL+'}', 'place-control', 1)
    for x,label in [(4,'target'),(6,'control')]:
        cmd(f'/execute if block {x} 200 4 chest[type=single]', label+'-block',1)
        cmd(f'/execute if data block {x} 200 4 {{id:"minecraft:chest",x:{x},y:200,z:4}}', label+'-entity',1)
    cmd('/scoreboard objectives add h279 dummy', 'objective')
    cmd('/execute store result score start h279 run time query gametime', 'clock-start', diagnostic=True)
    cmd('/data get block 4 200 4 Items', 'diagnostic-initial')
    cmd('/item fill block 4 200 4 {type:"minecraft:slot_range",slots:"container.99"} with diamond', 'original-inline99', admission=False)
    cmd('/data get block 4 200 4 Items', 'original-inline99-state')
    cmd('/execute if data block 4 200 4 {Items:'+POPULATED+'}', 'original-inline99-unchanged',1)
    for label,name,empty in CASES:
        initial=EMPTY_ZERO if empty else POPULATED
        cmd('/data modify block 4 200 4 Items set value '+initial, label+'-reset')
        cmd('/data get block 4 200 4 Items', label+'-initial')
        cmd('/execute if data block 4 200 4 {Items:'+initial+'}', label+'-initial-types',1)
        if empty:cmd('/execute unless data block 4 200 4 Items[{Slot:0b}]', label+'-empty-zero',1)
        cmd('/data get block 6 200 4 Items', label+'-control-before')
        cmd('/scoreboard players set result h279 -99', label+'-reset-result',-99)
        cmd('/scoreboard players set success h279 -99', label+'-reset-success',-99)
        source='{type:"minecraft:slot_range",slots:"'+name+'"}'
        cmd('/execute store result score result h279 store success score success h279 run item fill block 4 200 4 '+source+' with diamond', label+'-fill',admission=False)
        for key in ['result','success']:
            cmd('/scoreboard players get '+key+' h279',label+'-'+key,admission=False)
        cmd('/data get block 4 200 4 Items', label+'-after')
        cmd('/data get block 6 200 4 Items', label+'-control-after')
        if name in ['container.99','container.54','container.53']:
            cmd('/execute if data block 4 200 4 {Items:'+initial+'}', label+'-unchanged',1)
        else:
            cmd('/execute if data block 4 200 4 {Items:[{Slot:0b,id:"minecraft:diamond",count:1}]}', label+'-diamond',1)
        cmd('/data get block 4 200 4', label+'-body',diagnostic=True)
    cmd('/execute store result score end h279 run time query gametime','clock-end',diagnostic=True)
    cmd('/scoreboard players operation end h279 -= start h279','clock-delta-operation',0)
    cmd('/scoreboard players get end h279','clock-delta',0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='slot-range-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Inline slot ranges', ops)]
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
