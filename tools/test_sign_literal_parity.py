#!/usr/bin/env python3
"""Modern sign literal strings before and after same-level clone."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

LINES = ['plain text', '"quoted text"', '{"text":"literal object","bold":true}', '']
CASES = ['initial source', 'same-level clone']
MESSAGES = json.dumps(LINES)
BACK = '[{text:"Structured",color:"red",bold:true},"","",""]'


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
    cmd('/tp @s 14.5 200 14.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    for condition, label in [('loaded 2 200 2', 'loaded-source'), ('loaded 4 200 2', 'loaded-clone'), ('block 2 199 2 stone', 'source-support'), ('block 4 199 2 stone', 'clone-support'), ('block 2 200 2 air', 'source-air'), ('block 4 200 2 air', 'clone-air'), ('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]', 'owner')]:
        cmd('/execute if ' + condition, label, 1)
    cmd('/scoreboard objectives add h238 dummy', 'objective')
    cmd('/execute store result score start h238 run time query gametime', 'clock-start', diagnostic=True)
    tag = '{front_text:{messages:'+MESSAGES+'},back_text:{messages:'+BACK+'}}'
    cmd('/setblock 2 200 2 minecraft:oak_sign[rotation=0]'+tag, 'place-sign', 1)
    def read_sign(x, label):
        at = f'{x} 200 2'
        cmd('/execute if block '+at+' minecraft:oak_sign[rotation=0,waterlogged=false]', label+'-block', 1)
        cmd('/data get block '+at+' id', label+'-id')
        cmd('/execute if data block '+at+' {id:"minecraft:sign",x:'+str(x)+',y:200,z:2}', label+'-entity', 1)
        cmd('/data get block '+at, label+'-body', diagnostic=True)
        cmd('/data get block '+at+' front_text.messages', label+'-messages', admission=False)
        for index in range(4):
            cmd('/data get block '+at+f' front_text.messages[{index}]', label+f'-line{index}', admission=False)
        cmd('/execute if data block '+at+' {front_text:{messages:'+MESSAGES+'}}', label+'-literal-types', admission=False)
        for key in ['text','color','bold']:
            cmd('/data get block '+at+' back_text.messages[0].'+key, label+'-structured-'+key)
        cmd('/execute if data block '+at+' {back_text:{messages:[{text:"Structured",color:"red",bold:1b},"","",""]}}', label+'-structured-types', 1)
    read_sign(2, 'source-before')
    cmd('/clone 2 200 2 2 200 2 4 200 2 replace', 'clone', 1)
    read_sign(4, 'clone-after')
    cmd('/data get block 2 200 2 front_text.messages', 'source-after-messages', admission=False)
    cmd('/execute if block 2 200 2 minecraft:oak_sign[rotation=0]', 'source-retained', 1)
    cmd('/setblock 8 200 2 minecraft:chest{CustomName:"h238_control",Items:[{Slot:0b,id:"minecraft:stone",count:2}]}', 'place-chest', 1)
    cmd('/execute if block 8 200 2 minecraft:chest[type=single]', 'chest-block', 1)
    cmd('/data get block 8 200 2 id', 'chest-id')
    cmd('/data get block 8 200 2 CustomName', 'chest-name')
    cmd('/data get block 8 200 2 Items[0].count', 'chest-count', 2)
    cmd('/execute if data block 8 200 2 {id:"minecraft:chest",Items:[{Slot:0b,id:"minecraft:stone",count:2}]}', 'chest-entity', 1)
    cmd('/execute store result score end h238 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h238 -= start h238', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h238', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='sign-literal-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Modern sign literals', ops)]
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
