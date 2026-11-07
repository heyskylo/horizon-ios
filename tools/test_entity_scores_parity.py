#!/usr/bin/env python3
"""Named entity_scores predicates against actual target holders."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile

import test_command_exec_parity as driver

CASES = [0, 1, 2, 3, None]
PREDICATES = {
    'h270:min2': {'type': 'minecraft:entity_scores', 'entity': 'this', 'scores': {'pts': {'min': 2}}},
    'h270:always': {'type': 'minecraft:all_of', 'terms': []},
    'h270:pig': {'type': 'minecraft:entity_properties', 'entity': 'this', 'predicate': {'minecraft:entity_type': 'minecraft:pig'}},
}

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
    cmd('/execute if loaded 4 200 4', 'loaded', 1)
    cmd('/execute if block 4 199 4 stone', 'support', 1)
    cmd('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]', 'player', 1)
    cmd('/scoreboard objectives add h270 dummy', 'objective')
    cmd('/scoreboard objectives add pts dummy', 'points-objective')
    cmd('/execute store result score start h270 run time query gametime', 'clock-start', diagnostic=True)
    cmd('/datapack list enabled', 'pack-list')
    cmd('/execute if predicate h270:always', 'named-pack-always', 1)
    for index,score in enumerate(CASES):
        words=f'270,1,0,{index+1}';uuid='[I;'+words+']';target=uuid_string(words);x=3.5+index
        cmd(f'/summon pig {x} 200 4.5 {{UUID:{uuid},Tags:["h270_{index}"],Health:10f,NoAI:1b,NoGravity:1b}}', f'actor{index}-summon', 1)
        cmd(f'/execute if entity @e[type=pig,nbt={{UUID:{uuid},Health:10f,NoAI:1b,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]', f'actor{index}-identity', 1)
        cmd(f'/data get entity {target} UUID', f'actor{index}-uuid')
        cmd(f'/data get entity {target}', f'actor{index}-body', diagnostic=True)
        cmd(f'/execute as {target} if predicate h270:pig run scoreboard players set pack_probe h270 1', f'actor{index}-named-pig', 1)
        if score is not None:
            cmd(f'/scoreboard players set {target} pts {score}', f'actor{index}-score-set', score)
    for phase in ['initial', 'mutated']:
        if phase=='mutated':
            for index,score in [(0,2),(3,1)]:
                cmd(f'/scoreboard players set {uuid_string(f"270,1,0,{index+1}")} pts {score}', f'mutate{index}', score)
        for index in range(5):
            words=f'270,1,0,{index+1}';target=uuid_string(words)
            prefix=f'{phase}-{index}'
            cmd(f'/scoreboard players get {target} pts', prefix+'-score')
            for sign in ['if','unless']:
                cmd(f'/execute {sign} entity @e[type=pig,nbt={{UUID:[I;{words}]}},scores={{pts=2..}}]', prefix+'-direct-'+sign)
                cmd('/scoreboard players set result h270 -99', prefix+'-'+sign+'-reset-result', -99)
                cmd('/scoreboard players set success h270 -99', prefix+'-'+sign+'-reset-success', -99)
                cmd('/scoreboard players set branch h270 0', prefix+'-'+sign+'-reset-branch', 0)
                cmd(f'/execute store result score result h270 store success score success h270 as {target} {sign} predicate h270:min2 run scoreboard players set branch h270 1', prefix+'-'+sign+'-named', admission=False)
                for key in ['result','success','branch']:
                    cmd(f'/scoreboard players get {key} h270', prefix+'-'+sign+'-'+key, admission=False)
    cmd('/execute store result score end h270 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h270 -= start h270', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h270', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='entity-scores-', dir=driver.lab.LOCAL))
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
    driver.corpus_module.predicate_selector_fixtures = lambda names: PREDICATES
    driver.corpus_module.groups = lambda: [('Named entity scores', ops)]
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
