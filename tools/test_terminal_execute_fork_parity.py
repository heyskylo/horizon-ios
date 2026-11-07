#!/usr/bin/env python3
"""Terminal execute fork errors through actual commands, with complete records."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

A = '00000093-0000-0000-0000-000000000001'
B = '00000093-0000-0000-0000-000000000002'
BOTH = '@e[type=pig,tag=h147,sort=nearest]'
CASES = [
    ('nofork-false', 'if score zero h147 matches 1'),
    ('nofork-missing', 'if score zero h147_missing matches 1'),
    ('one-as-false', f'as {A} if score zero h147 matches 1'),
    ('one-as-unless', f'as {A} unless entity @s'),
    ('mixed-as', f'as {BOTH} if entity @s[tag=h147_yes]'),
    ('one-at-false', f'at {A} if block ~ ~-1 ~ air'),
    ('fork-missing', f'as {A} if score zero h147_missing matches 1'),
    ('empty-as', 'as @e[type=pig,tag=h147_none] if score zero h147 matches 1'),
    ('fork-positive', f'as {B} if entity @s[tag=h147_yes]'),
    ('nofork-positive', 'if score zero h147 matches 0'),
    ('nonterminal-false', f'as {A} if score zero h147 matches 1 run scoreboard players set branch h147 7'),
    ('nested-run-error', f'as {A} run execute if score zero h147 matches 1'),
    ('position-nofork-false', 'positioned 0.5 200 0.5 if score zero h147 matches 1'),
    ('many-as-false', f'as {BOTH} if score zero h147 matches 1'),
    ('at-positive', f'at {A} if block ~ ~-1 ~ stone'),
    ('nonterminal-positive', f'as {B} if entity @s[tag=h147_yes] run scoreboard players set branch h147 7'),
]


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
    cmd('/scoreboard objectives add h147 dummy', 'objective')
    cmd('/scoreboard players set zero h147 0', 'zero-score', 0)
    for index in [1, 2]:
        x = 1.5 if index == 1 else 4.5
        tags = '["h147"]' if index == 1 else '["h147","h147_yes"]'
        uid = A if index == 1 else B
        cmd(f'/summon pig {x} 200 4.5 {{UUID:[I;147,0,0,{index}],Health:10f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],Tags:{tags},CustomName:"h147_{index}"}}', f'actor{index}-summon', 1)
        cmd(f'/execute if entity @e[type=pig,nbt={{UUID:[I;147,0,0,{index}],Health:10f,Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d],NoAI:1b,NoGravity:1b,Tags:{tags}}}]', f'actor{index}-guard', 1)
        cmd(f'/data get entity {uid} UUID', f'actor{index}-uuid', 4)
        cmd(f'/data get entity {uid}', f'actor{index}-body', diagnostic=True)
    cmd('/execute if entity '+BOTH, 'two-sources', 2)
    cmd('/execute unless entity @e[type=pig,tag=h147_none]', 'empty-selection', 1)
    cmd('/execute store result score start h147 run time query gametime', 'clock-start', diagnostic=True)
    for label, tail in CASES:
        for holder in ['result', 'success']:
            cmd(f'/scoreboard players set {holder} h147 -99', label+'-reset-'+holder, -99)
        cmd('/scoreboard players set branch h147 -77', label+'-reset-branch', -77)
        cmd('/execute store result score result h147 store success score success h147 '+tail, label+'-command', admission=False)
        for holder in ['result', 'success', 'branch']:
            cmd(f'/scoreboard players get {holder} h147', label+'-'+holder, admission=False)
    cmd('/execute store result score end h147 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h147 -= start h147', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h147', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='terminal-execute-fork-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Terminal execute forks', ops)]
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
