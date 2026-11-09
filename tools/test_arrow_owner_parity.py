#!/usr/bin/env python3
"""Loaded arrow Owner through actual command NBT."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

PLAYER = '[I;-1608632253,-450807400,-1786055591,-2146546415]'
CASES = [('absent', None), ('mob', '[I;213,1,2,3]'), ('stand', '[I;213,4,5,6]'), ('player', '[I;-1608632253,-450807400,-1786055591,-2146546415]'), ('unresolved', '[I;213,91,92,93]')]

def uuid_string(words):
    import uuid
    return str(uuid.UUID(hex=''.join(f'{int(n)&0xffffffff:08x}' for n in words.split(','))))


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
    cmd('/execute if loaded 8 200 4', 'loaded', 1)
    cmd('/execute if block 8 199 4 stone', 'support', 1)
    cmd('/execute if entity @s[type=player,name=Player,nbt={UUID:'+PLAYER+',Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]', 'player-identity', 1)
    cmd('/data get entity @s UUID', 'player-uuid')
    cmd('/scoreboard objectives add h213 dummy', 'objective')
    cmd('/execute store result score start h213 run time query gametime', 'clock-start', diagnostic=True)
    for kind, words, x, health in [('pig', '213,1,2,3', 3.5, 10), ('armor_stand', '213,4,5,6', 5.5, 20)]:
        uuid='[I;'+words+']';target=uuid_string(words)
        flags=',NoAI:1b' if kind=='pig' else ''
        cmd(f'/summon {kind} {x} 200 4.5 {{UUID:{uuid},Tags:["h213_owner"],NoGravity:1b,Health:{health}f{flags}}}', kind+'-summon', 1)
        cmd(f'/execute if entity @e[type=minecraft:{kind},nbt={{UUID:{uuid},Health:{health}f,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]', kind+'-identity', 1)
        cmd(f'/data get entity {target} UUID', kind+'-uuid')
        cmd(f'/data get entity {target}', kind+'-body', diagnostic=True)
    for index,(name,owner) in enumerate(CASES):
        words=f'213,10,0,{index+1}';uuid='[I;'+words+']';target=uuid_string(words);z=4.5+index
        owner_tag=',Owner:'+owner if owner else ''
        cmd(f'/summon arrow 8.5 202 {z} {{UUID:{uuid},Tags:["h213_{name}"],CustomName:"Arrow",NoGravity:1b,Motion:[0d,0d,0d]{owner_tag}}}', name+'-summon', 1)
        cmd(f'/execute if entity @e[type=minecraft:arrow,nbt={{UUID:{uuid},NoGravity:1b,Pos:[8.5d,202d,{z}d],Motion:[0d,0d,0d]}}]', name+'-identity', 1)
        cmd(f'/data get entity {target} UUID', name+'-uuid')
        cmd(f'/data get entity {target} Pos', name+'-pos')
        cmd(f'/data get entity {target} Motion', name+'-motion')
        cmd(f'/data get entity {target} CustomName', name+'-name')
        cmd(f'/data get entity {target}', name+'-body', diagnostic=True)
        cmd('/scoreboard players set result h213 -99', name+'-reset-result', -99)
        cmd('/scoreboard players set success h213 -99', name+'-reset-success', -99)
        cmd(f'/execute store result score result h213 store success score success h213 run data get entity {target} Owner', name+'-owner', admission=False)
        cmd('/scoreboard players get result h213', name+'-result', admission=False)
        cmd('/scoreboard players get success h213', name+'-success', admission=False)
        condition=f'if data entity {target} {{Owner:{owner}}}' if owner else f'unless data entity {target} Owner'
        cmd('/execute '+condition, name+'-owner-type-presence', admission=False)
        cmd(f'/data get entity {target} Tags', name+'-tags')
    cmd('/execute store result score end h213 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h213 -= start h213', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h213', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='arrow-owner-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.groups = lambda: [('Loaded arrow owners', ops)]
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
