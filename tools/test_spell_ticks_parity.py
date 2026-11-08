#!/usr/bin/env python3
"""Loaded spell counters through actual Game entity ticks.

All scalar/body admission commands are strict. Complete entity compounds and
absolute startup clocks remain executable diagnostics; the 31-tick delta is strict.
"""
import hashlib
import json
from pathlib import Path
import sys
try:
    from . import harness_args, test_command_exec_parity as driver
except ImportError:
    import harness_args
    import test_command_exec_parity as driver


def subjects():
    for i, (kind, no_ai, initial) in enumerate((k, a, v) for k in ['evoker', 'illusioner'] for a in [0, 1] for v in [30, 0, -2]):
        yield i + 1, kind, no_ai, initial, [3.5 + (i % 4) * 3, 200.0, 3.5 + (i // 4) * 4]


def program(full=False):
    ops = []

    def cmd(text, label, expected=None, admission=True, diagnostic=False):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if expected is not None:
            op['expect_result'] = expected
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def tick(n, label):
        cmd(f'/tick step {n}', label + '-request')
        ops.append(dict(op='tick', n=n))

    cmd('/tick freeze', 'freeze')
    for key in ['spawn_mobs', 'advance_time', 'mob_griefing']:
        cmd('/gamerule ' + key + ' false', key)
    cmd('/gamerule random_tick_speed 0', 'random-ticks')
    cmd('/forceload add 0 0 15 15', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/fill 0 200 0 15 205 15 air', 'clear')
    cmd('/tp @s 1.5 200 1.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/execute if entity @s[type=player,name=Player]', 'owner-type', 1)
    for key, value in [('UUID', 4), ('Health', 20), ('playerGameType', 1), ('Pos', 3)]:
        cmd('/data get entity @s ' + key, 'owner-' + key, value)
    cmd('/scoreboard objectives add h180 dummy', 'objective')
    cmd('/execute store result score start h180 run time query gametime', 'clock-start', diagnostic=True)
    for i, kind, no_ai, initial, pos in subjects():
        xyz = ' '.join(map(str, pos))
        body = '{UUID:[I;180,0,0,%d],Tags:["h180_%d"],Health:20f,NoAI:%db,NoGravity:1b,PersistenceRequired:1b,SpellTicks:%d,attributes:[{id:"minecraft:movement_speed",base:0.0d}]}' % (i, i, no_ai, initial)
        cmd(f'/summon {kind} {xyz} {body}', f'{i}-summon')
        cmd(f'/execute if loaded {int(pos[0])} 200 {int(pos[2])}', f'{i}-loaded', 1)
        cmd(f'/execute if block {int(pos[0])} 199 {int(pos[2])} stone', f'{i}-floor', 1)
    previous = 0
    for elapsed in [0, 1, 3, 10, 31]:
        if elapsed:
            tick(elapsed - previous, f'tick{elapsed}')
        previous = elapsed
        for i, kind, no_ai, initial, pos in subjects():
            uid = f'000000b4-0000-0000-0000-{i:012x}'
            label = f'{i}-{elapsed}'
            body = '{UUID:[I;180,0,0,%d],Tags:["h180_%d"],Health:20f,NoGravity:1b,PersistenceRequired:1b}' % (i, i)
            cmd(f'/execute if entity @e[type={kind},nbt={body}]', label + '-identity', 1)
            cmd(f'/execute {"if" if no_ai else "unless"} entity @e[type={kind},nbt={{UUID:[I;180,0,0,{i}],NoAI:1b}}]', label + '-ai', 1)
            cmd(f'/data get entity {uid} Pos', label + '-position')
            for field in ['result', 'success']:
                cmd(f'/scoreboard players set {field} h180 -99', label + '-reset-' + field, -99)
            value = max(0, initial - elapsed) if not no_ai and initial > 0 else initial
            cmd(f'/execute store result score result h180 store success score success h180 run data get entity {uid} SpellTicks', label + '-timer-read', admission=False)
            cmd('/scoreboard players get result h180', label + '-timer-result', admission=False)
            cmd('/scoreboard players get success h180', label + '-timer-success', 1)
            # Exact numeric value plus type is retained in the raw data reply.
            cmd(f'/execute if entity @e[type={kind},nbt={{UUID:[I;180,0,0,{i}],SpellTicks:{value}}}]', label + '-timer-typed', admission=False)
            cmd(f'/data get entity {uid}', label + '-body', diagnostic=True)
    cmd('/execute store result score end h180 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h180 -= start h180', 'clock-delta-operation', 31)
    cmd('/scoreboard players get end h180', 'clock-delta', 31)
    return ops


def main():
    full = '--full-diagnostic' in sys.argv
    if full:
        sys.argv.remove('--full-diagnostic')
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference')
        reference = Path(sys.argv[i + 1]).resolve().read_bytes()
        del sys.argv[i:i + 2]
    build = harness_args.ensure_work_dir(driver.lab.LOCAL / 'spell-ticks-parity').resolve()
    ops = program(full)
    driver.corpus_module.groups = lambda: [('spell ticks', ops)]
    sys.argv.append('--allow-gaps')
    driver.main()
    recorded = [o for o in ops if o['op'] == 'cmd' and o.get('record', True)]
    java_bytes = (build / 'vanilla.jsonl').read_bytes()
    java = [json.loads(s) for s in java_bytes.splitlines()]
    rows = {o['label']: r for o, r in zip(recorded, java)}
    assert len(recorded) == len(java)
    for elapsed in [0, 1, 3, 10, 31]:
        for i, _, no_ai, initial, _ in subjects():
            value = max(0, initial - elapsed) if not no_ai and initial > 0 else initial
            assert rows[f'{i}-{elapsed}-timer-read']['results'] == [[True, value]]
            assert rows[f'{i}-{elapsed}-timer-typed']['results'] == [[True, 1]]
    if '--reference-only' in sys.argv:
        return
    if reference and not full:
        assert reference == java_bytes, 'fresh reference changed'
    comparison = reference or java_bytes
    (build / 'comparison-reference.jsonl').write_bytes(comparison)
    java = [json.loads(s) for s in comparison.splitlines()]
    native = [json.loads(s) for s in (build / 'rust.jsonl').read_bytes().splitlines()]
    assert len(recorded) == len(java) == len(native)
    differences = [dict(label=o['label'], expected=a, actual=b) for o, a, b in zip(recorded, java, native) if a != b]
    report = dict(records=len(native), passed=len(native)-len(differences), differences=differences,
                  reference_sha256=hashlib.sha256(comparison).hexdigest(), comparison='complete records; no masks or tolerances')
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
    for o, a, b in zip(recorded, java, native):
        if o['admission'] and not o['label'].endswith('-body') and o['label'] not in ['clock-start', 'clock-end']:
            assert a == b, ('admission', o, a, b)
    print(f'Strict raw records: {report["passed"]}/{report["records"]}')
    if differences:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
