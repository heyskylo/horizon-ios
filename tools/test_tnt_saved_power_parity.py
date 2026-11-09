#!/usr/bin/env python3
"""Saved TNT power through real summon/get commands; complete declared records."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [('absent', None), ('default', 4), ('zero', 0), ('two', 2), ('five', 5), ('maximum', 128)]


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
    for condition, label in [('loaded 8 200 8', 'loaded'), ('block 8 199 8 stone', 'floor-state'), ('block 8 200 8 air', 'air'), ('entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1}]', 'owner')]:
        cmd('/execute if ' + condition, label, 1)
    cmd('/scoreboard objectives add h091 dummy', 'objective')
    cmd('/execute store result score start h091 run time query gametime', 'clock-start', diagnostic=True)
    for index, (label, power) in enumerate(CASES, 1):
        uid = f'0000005b-0000-0000-0000-{index:012x}'
        x = 1.5 + 2 * (index - 1)
        extra = '' if power is None else f',explosion_power:{power}f'
        # An optional loaded owner UUID is a preservation payload, not a live actor witness.
        owner = ',owner:[I;91,1,0,90]' if label == 'maximum' else ''
        cmd(f'/summon tnt {x} 200 4.5 {{UUID:[I;91,0,0,{index}],Motion:[0d,0d,0d],NoGravity:1b,Silent:1b,Invulnerable:1b,PortalCooldown:7,Tags:["h091_{index}"],fuse:80s,block_state:"minecraft:stone"{extra}{owner}}}', label+'-summon', 1)
        cmd(f'/execute if entity @e[type=tnt,nbt={{UUID:[I;91,0,0,{index}],Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d],NoGravity:1b,Silent:1b,Invulnerable:1b,PortalCooldown:7,Tags:["h091_{index}"]}}]', label+'-actor', 1)
        cmd(f'/data get entity {uid} UUID', label+'-uuid', 4)
        cmd(f'/data get entity {uid}', label+'-body', diagnostic=True)
        cmd(f'/data get entity {uid} fuse', label+'-fuse', 80)
        cmd(f'/execute if data entity {uid} {{fuse:80s,block_state:"minecraft:stone"}}', label+'-fuse-state-types', 1)
        cmd(f'/data get entity {uid} block_state', label+'-block-name')
        if label == 'maximum':
            cmd(f'/data get entity {uid} owner', label+'-owner', 4)
            cmd(f'/execute if data entity {uid} {{owner:[I;91,1,0,90]}}', label+'-owner-type', 1)
        else:
            cmd(f'/execute unless data entity {uid} owner', label+'-no-owner', 1)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h091 -99', label+'-reset-'+holder, -99)
        cmd(f'/execute store result score result h091 store success score success h091 run data get entity {uid} explosion_power', label+'-power-read', admission=False)
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players get {holder} h091', label+'-power-'+holder, admission=False)
        if power is None or power == 4:
            cmd(f'/execute unless data entity {uid} explosion_power', label+'-power-absent', admission=False)
        else:
            cmd(f'/execute if data entity {uid} explosion_power', label+'-power-present', admission=False)
            cmd(f'/execute if data entity {uid} {{explosion_power:{power}f}}', label+'-power-type', admission=False)
    cmd('/execute store result score end h091 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h091 -= start h091', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h091', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='tnt-saved-power-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('TNT saved power', ops)]
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
