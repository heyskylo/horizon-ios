#!/usr/bin/env python3
"""NumericTag.byteValue boolean load/save through actual Game commands.

Complete declared records are strict; full bodies and absolute clocks remain
predeclared raw traces. NoAI false omission and pickup's always-written byte
are distinct controls, without changing either saved-field policy.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver

CASES = [
    ('absent', None, False),
    ('byte_zero', '0b', False), ('byte_one', '1b', True), ('byte_minus_one', '-1b', True),
    ('short_wrap', '256s', False),
    ('int_wrap', '256', False), ('int_wrap_one', '257', True), ('int_minus_wrap', '-256', False),
    ('long_wrap', '256L', False), ('long_wrap_one', '257L', True),
    ('long_int_wrap', '4294967296L', False), ('long_above_double_precision', '9007199254740993L', True),
    ('float_half', '0.5f', False), ('float_negative_half', '-0.5f', True),
    ('float_wrap', '256.5f', False), ('float_negative_wrap', '-255.5f', False),
    ('double_half', '0.5d', False), ('double_negative_half', '-0.5d', True),
    ('double_wrap', '256.5d', False), ('double_negative_wrap', '-255.5d', False),
    ('float_negative_overflow', '-1.0e20f', False), ('double_negative_overflow', '-1.0e20d', False),
    ('double_positive_overflow', '1.0e20d', True),
    ('string_one', '"1"', False), ('string_true', '"true"', False), ('compound', '{a:1}', False),
]


def program():
    ops = []
    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if result is not None:
            op['expect_result'] = result
        if diagnostic:
            op.update(record=False, trace=True)
        ops.append(op)
    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 14.5 200 14.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    for condition, label in [('loaded 8 200 8', 'loaded'), ('block 8 199 8 stone', 'floor-state'),
                             ('block 8 200 8 air', 'air'), ('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]', 'owner')]:
        cmd('/execute if '+condition, label, 1)
    cmd('/scoreboard objectives add h045 dummy', 'objective')
    cmd('/execute store result score start h045 run time query gametime', 'clock-start', diagnostic=True)
    for index, (label, value, expected) in enumerate(CASES, 1):
        x, z = 1.5+2*((index-1)%6), 1.5+2*((index-1)//6)
        uid = f'0000002d-0000-0000-0000-{index:012x}'
        extra = '' if value is None else f',NoAI:{value},CanPickUpLoot:{value}'
        cmd(f'/summon pig {x} 200 {z} {{UUID:[I;45,0,0,{index}],Tags:["h045_{index}"],Health:10f,NoGravity:1b,PersistenceRequired:1b{extra}}}', label+'-summon', 1)
        cmd(f'/execute if entity @e[type=pig,tag=h045_{index},nbt={{UUID:[I;45,0,0,{index}],Health:10f,NoGravity:1b,PersistenceRequired:1b,Pos:[{x}d,200d,{z}d]}}]', label+'-actor', 1)
        cmd(f'/data get entity {uid}', label+'-body', diagnostic=True)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h045 -99', label+'-reset-'+holder, -99)
        cmd(f'/execute store result score result h045 store success score success h045 run data get entity {uid} NoAI', label+'-noai-read', admission=False)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players get {holder} h045', label+'-'+holder, admission=False)
        if expected:
            cmd(f'/execute if data entity {uid} {{NoAI:1b}}', label+'-noai-byte', admission=False)
        else:
            cmd(f'/execute unless data entity {uid} NoAI', label+'-noai-omitted', admission=False)
        cmd(f'/data get entity {uid} CanPickUpLoot', label+'-pickup-read', int(expected), admission=False)
        cmd(f'/execute if data entity {uid} {{CanPickUpLoot:{int(expected)}b}}', label+'-pickup-byte', admission=False)
    cmd('/execute store result score end h045 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h045 -= start h045', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h045', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        # tools/parity.py runs every harness with no arguments: a fresh folder under .local.
        import shutil
        default = Path(__file__).resolve().parents[1] / '.local' / 'entity-nbt-boolean-parity'
        shutil.rmtree(default, ignore_errors=True)
        sys.argv += ['--work-dir', str(default)]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve attempts; choose a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    (build/'declaration.json').write_text(json.dumps(dict(
        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), records=len(recorded),
        cases=CASES, labels=[op['label'] for op in recorded],
        raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
        comparison='complete declared records, no masks/tolerances; whole bodies and absolute clocks are separate raw traces'), indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('entity NBT boolean bytes', ops)]
    sys.argv.append('--allow-gaps'); driver.main()
    java_bytes = (build/'vanilla.jsonl').read_bytes()
    if expected_bytes is not None:
        assert java_bytes==expected_bytes and reference.read_bytes()==expected_bytes, 'fresh Java reference changed'
    java = [json.loads(s) for s in java_bytes.splitlines()]
    if '--reference-only' in sys.argv:
        return
    native = [json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded)==len(java)==len(native)
    residuals = [dict(label=op['label'],expected=a,actual=b) for op,a,b in zip(recorded,java,native) if a!=b]
    report = dict(records=len(recorded),passed=len(recorded)-len(residuals),residuals=residuals,
                  reference_sha256=hashlib.sha256(java_bytes).hexdigest())
    (build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n')
    for op,a,b in zip(recorded,java,native):
        if op['admission']:
            assert a==b, ('admission',op,a,b)
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals:
        raise SystemExit(1)


if __name__=='__main__':
    main()
