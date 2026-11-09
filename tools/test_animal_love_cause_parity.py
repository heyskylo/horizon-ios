#!/usr/bin/env python3
"""Animal LoveCause through real summon/get commands; complete declared records."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [('absent', None), ('valid', '[I;57,1,2,3]'),
         ('short', '[I;57,1,2]'), ('long', '[I;57,1,2,3,4]'),
         ('empty', '[I;]'), ('scalar', '7'), ('compound', '{value:7}')]


def program(include_overlong=False):
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
    for condition, label in [('loaded 8 200 8', 'loaded'), ('block 8 199 8 stone', 'floor-state'), ('block 8 200 8 air', 'air'), ('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]', 'owner')]:
        cmd('/execute if ' + condition, label, 1)
    cmd('/scoreboard objectives add h057 dummy', 'objective')
    cmd('/execute store result score start h057 run time query gametime', 'clock-start', diagnostic=True)
    for index, (label, cause) in enumerate(CASES if include_overlong else [c for c in CASES if c[0] != 'long'], 1):
        uid = f'00000039-0000-0000-0000-{index:012x}'
        x = 1.5 + 1.5 * (index - 1)
        extra = '' if cause is None else ',LoveCause:' + cause
        cmd(f'/summon pig {x} 200 4.5 {{UUID:[I;57,0,0,{index}],Health:10f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],Age:0,InLove:123,Tags:["h057_{index}"],CustomName:"h057_{index}"{extra}}}', label+'-summon', 1)
        cmd(f'/execute if entity @e[type=pig,nbt={{UUID:[I;57,0,0,{index}],Health:10f,Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d],NoAI:1b,NoGravity:1b,Tags:["h057_{index}"],CustomName:"h057_{index}"}}]', label+'-actor', 1)
        cmd(f'/data get entity {uid} UUID', label+'-uuid', 4)
        cmd(f'/data get entity {uid}', label+'-body', diagnostic=True)
        cmd(f'/data get entity {uid} InLove', label+'-in-love', 123)
        cmd(f'/execute if data entity {uid} {{InLove:123}}', label+'-in-love-type', 1)
        cmd(f'/data get entity {uid} CustomName', label+'-name')
        cmd(f'/data get entity {uid} Tags', label+'-tags', 1)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h057 -99', label+'-reset-'+holder, -99)
        cmd(f'/execute store result score result h057 store success score success h057 run data get entity {uid} LoveCause', label+'-cause-read', admission=False)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players get {holder} h057', label+'-cause-'+holder, admission=False)
        if label == 'valid':
            cmd(f'/execute if data entity {uid} LoveCause', label+'-cause-present', admission=False)
            cmd(f'/execute if data entity {uid} {{LoveCause:[I;57,1,2,3]}}', label+'-cause-type', admission=False)
        else:
            cmd(f'/execute unless data entity {uid} LoveCause', label+'-cause-absent', admission=False)
    cmd('/execute store result score end h057 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h057 -= start h057', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h057', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='animal-love-cause-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    include_overlong = '--uuid-length-diagnostic' in sys.argv
    if include_overlong: sys.argv.remove('--uuid-length-diagnostic')
    ops = program(include_overlong); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=CASES if include_overlong else [c for c in CASES if c[0] != 'long'], labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete records, no masks/tolerances; bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('Animal LoveCause', ops)]
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
