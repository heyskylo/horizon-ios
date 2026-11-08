#!/usr/bin/env python3
"""Real Game/Java mob-rider movement ranges, with explicit flat-world prerequisites.

Random trajectories are retained as diagnostics; only the declared movement
ranges are compared. Stable commands, identity, attachment and floor guards
are strict. --reference reuses the same fresh Java records for an old/fixed pair.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import uuid

import lab
import test_rust_parity as base
from integration_env import run_game


def identity(i):
    return str(uuid.UUID(int=(299 << 96) + i))


def array(i):
    return f'[I;299,0,0,{i}]'


LANES = [
    ('patrol', 'ravager', 'pillager', -40.5, False, False),
    ('pillager', 'ravager', 'pillager', 12.5, False, True),
    ('slime', 'ravager', 'slime', 20.5, False, True),
    ('zombie', 'ravager', 'zombie', 28.5, False, False),
    ('noai_zombie', 'ravager', 'zombie', 36.5, False, True),
    ('spider', 'spider', 'skeleton', 44.5, False, False),
    ('noai_vehicle', 'ravager', 'zombie', 52.5, True, False),
]


def fixture():
    ops = []
    def cmd(s, positive=False, position=None):
        op = {'op': 'cmd', 'cmd': '/' + s}
        if positive:
            op['positive'] = True
        if position:
            op['position'] = position
        ops.append(op)
    for s in ['tick freeze', 'gamerule minecraft:spawn_mobs false',
              'gamerule minecraft:advance_time false', 'gamerule minecraft:mob_griefing false',
              'time set midnight', 'difficulty normal', 'gamemode creative',
              'tp @s -6 -60 32', 'forceload add -32 -64 159 111']:
        cmd(s)
    ops.append({'op': 'cmd', 'cmd': '/kill @e[type=!player]', 'record': False})
    cmd('scoreboard objectives add audit dummy', True)
    ops.append({'op': 'cmd', 'cmd': '/execute store result score #start audit run time query gametime', 'record': False})
    chunks = [[x, z] for x in range(-2, 10) for z in range(-4, 7)]
    ops.append({'op': 'settle', 'required_chunks': chunks})
    for x, z in chunks:
        cmd(f'execute if loaded {16*x} -60 {16*z}', True)
    for x1, x2 in [(-32, 63), (64, 159)]:
        for z1, z2 in [(-64, 23), (24, 111)]:
            cmd(f'fill {x1} -61 {z1} {x2} -61 {z2} stone', True)
    cmd('execute if entity @s[gamemode=creative,nbt={Health:20f}]', True)
    cmd('data get entity @s Pos')
    for n, (name, vehicle, rider, z, noai_v, noai_r) in enumerate(LANES):
        parent, child = n*2 + 1, n*2 + 2
        cmd(f'execute if block 8 -61 {math.floor(z)} stone', True)
        cmd(f'execute if block 8 -60 {math.floor(z)} air', True)
        extra = ',Patrolling:1b,patrol_target:[I;88,-60,-40]' if n == 0 else ''
        extra += ',Size:0' if rider == 'slime' else ''
        child_nbt = (f'id:"minecraft:{rider}",UUID:{array(child)},Tags:["h299_{name}_rider"],'
                     f'PersistenceRequired:1b,Silent:1b,NoAI:{int(noai_r)}b{extra}')
        cmd(f'summon {vehicle} 8.5 -60 {z} {{UUID:{array(parent)},Tags:["h299_{name}"],'
            f'PersistenceRequired:1b,Silent:1b,OnGround:1b,NoAI:{int(noai_v)}b,Passengers:[{{{child_nbt}}}]}}', True)
    for step, ticks in enumerate([2, 400, 600]):
        cmd(f'tick step {ticks}', True)
        ops.append({'op': 'tick', 'n': ticks})
        ops.append({'op': 'cmd', 'cmd': '/execute store result score #clock audit run time query gametime', 'record': False})
        cmd('scoreboard players operation #clock audit -= #start audit')
        cmd('scoreboard players get #clock audit')
        for n, (name, vehicle, rider, _, noai_v, noai_r) in enumerate(LANES):
            parent, child = n*2+1, n*2+2
            for eid, species, noai in [(parent, vehicle, noai_v), (child, rider, noai_r)]:
                cmd(f'execute as {identity(eid)} if entity @s[type=minecraft:{species},nbt={{UUID:{array(eid)}}}]', True)
                cmd(f'execute as {identity(eid)} {"if" if noai else "unless"} entity @s[nbt={{NoAI:1b}}]', True)
                cmd(f'data get entity {identity(eid)} Health', True)
            cmd(f'execute as {identity(child)} on vehicle if entity @s[nbt={{UUID:{array(parent)}}}]', True)
            cmd(f'data get entity {identity(parent)} Pos', position=[step, name])
    return ops


def text(record):
    return ''.join(span[0] for message in record['messages'] for span in message)


def evaluate(ops, records):
    positions = {}
    clocks = iter([2, 402, 1002])
    for op, record in zip(ops, records, strict=True):
        if record['cmd'] != op['cmd']:
            raise ValueError('record order differs')
        command = op['cmd']
        expected = None
        if command == '/tick freeze' or command == '/gamemode creative' or command.startswith('/tp @s '):
            expected = [[True, 1]]
        elif command.startswith('/gamerule '):
            expected = [[True, 0]]
        elif command == '/time set midnight':
            expected = [[True, 18000]]
        elif command.startswith('/forceload add '):
            expected = [[True, 132]]
        elif command.startswith('/fill '):
            expected = [[True, 8448]]
        elif command == '/scoreboard players get #clock audit':
            expected = [[True, next(clocks)]]
        if expected is not None and record['results'] != expected:
            raise ValueError(f'setup/clock control failed: {record}')
        if command == '/difficulty normal' and 'Normal' not in text(record):
            raise ValueError(f'normal difficulty control failed: {record}')
        if op.get('positive') and not any(ok and value > 0 for ok, value in record['results']):
            raise ValueError(f'prerequisite failed: {op["cmd"]}: {record}')
        if 'position' in op:
            match = re.search(r'\[(-?[\d.Ee+]+)d, (-?[\d.Ee+]+)d, (-?[\d.Ee+]+)d\]', text(record))
            if not match:
                raise ValueError(f'missing position: {record}')
            xyz = list(map(float, match.groups()))
            if xyz[1] < -60:
                raise ValueError(f'actor below floor: {record}')
            positions[tuple(op['position'])] = xyz
    gates = {}
    for step, distance in [(1, 10), (2, 50)]:
        a, b = positions[0, 'patrol'], positions[step, 'patrol']
        gates[f'patrol_forward_{step}'] = b[0]-a[0] > distance
        gates[f'patrol_lateral_{step}'] = abs(b[2]-a[2]) < 20
    for name, *_ in LANES[1:]:
        a, b = positions[0, name], positions[2, name]
        moved = (b[0]-a[0])**2 + (b[2]-a[2])**2 > .25
        gates[f'stroll_{name}'] = moved == (name in ('slime', 'zombie', 'spider'))
    return gates, {f'{s}:{name}': xyz for (s, name), xyz in positions.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--reference', type=Path, help='previous output containing fresh Java records/config')
    args = parser.parse_args()
    if args.prepare:
        lab.prepare()
    out = (args.out or lab.LOCAL/'mob-rider-navigation-parity').resolve()
    if args.out is None and out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True, exist_ok=False)
    ops = fixture()
    recorded = [op for op in ops if op['op'] == 'cmd' and op.get('record', True)]
    script = out / 'script.jsonl'
    script.write_text(''.join(json.dumps({k: v for k, v in op.items() if k not in ('positive', 'position')})+'\n' for op in ops))
    lock, jars, libraries = base.verify_artifacts()
    def run(command, name, game=False, **kw):
        with (out / (name+'.log')).open('w') as log:
            result = (run_game if game else subprocess.run)(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT, **kw)
        if result.returncode:
            raise RuntimeError(f'{name}: {result.returncode}; see {out / (name+".log")}')
    if args.binary is None:
        run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'], 'build', cwd=lab.ROOT)
        binary = lab.cargo_target()/'fast'/('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    else:
        binary = args.binary.resolve(strict=True)
    if args.reference:
        ref = args.reference.resolve()
        if script.read_bytes() != (ref/'script.jsonl').read_bytes():
            raise ValueError('reference script differs')
        shutil.copyfile(ref/'java.jsonl', out/'java.jsonl')
        shutil.copyfile(ref/'world_gen_settings.dat', out/'world_gen_settings.dat')
    else:
        classes = out/'classes'
        classes.mkdir()
        cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
        oracle = lab.ROOT/'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCommandExecOracle.java'
        run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', cp, '-d', classes, oracle], 'javac')
        vanilla = out/'vanilla'
        vanilla.mkdir()
        run(['java', '-XX:ActiveProcessorCount=2', '-Xmx3G', '-Dhorizon.parity.flat=true', '-Duser.timezone=UTC', '-cp', str(classes)+os.pathsep+cp,
             'dev.horizonlab.parity.VanillaCommandExecOracle', vanilla/'universe', script, out/'java.jsonl', 1], 'java', cwd=vanilla)
        shutil.copyfile(vanilla/'universe/harness/data/minecraft/world_gen_settings.dat', out/'world_gen_settings.dat')
    java = [json.loads(line) for line in (out/'java.jsonl').read_text().splitlines()]
    java_gates, java_positions = evaluate(recorded, java)
    if not all(java_gates.values()):
        raise ValueError(f'Java range control failed: {java_gates}')
    rust = out/'rust'
    config = rust/'saves/harness/data/minecraft'
    config.mkdir(parents=True)
    # Transfer only generator configuration, never chunks, actors, RNG or AI state.
    shutil.copyfile(out/'world_gen_settings.dat', config/'world_gen_settings.dat')
    env = lab.game_env(HORIZON_PLAYER_NAME='Player', HORIZON_COMMAND_SCRIPT=script,
                       HORIZON_COMMAND_OUTPUT=out/'rust.jsonl', HORIZON_HARNESS_TERRAIN_ONLY='1', HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
    run([binary, '--resources', lab.RESOURCES, '--world', 'harness', '--seed', 1, '--radius', 12],
        'rust', game=True, cwd=rust, env=env)
    native = [json.loads(line) for line in (out/'rust.jsonl').read_text().splitlines()]
    gates, positions = evaluate(recorded, native)
    # Positions use only the explicit range contract; raw trajectories remain below.
    strict = [i for i, (op, v, r) in enumerate(zip(recorded, java, native, strict=True))
              if 'position' not in op and v != r]
    report = {'script_sha256': hashlib.sha256(script.read_bytes()).hexdigest(),
              'java_sha256': hashlib.sha256((out/'java.jsonl').read_bytes()).hexdigest(),
              'stable_record_count': len(recorded)-len(positions),
              'strict_mismatch_indices': strict, 'range_gates': gates,
              'java_range_gates': java_gates, 'java_position_diagnostics': java_positions,
              'native_position_diagnostics': positions,
              'limits': 'Random positions are diagnostics, not exact parity; only declared ranges are gates.'}
    (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({'strict_mismatches': strict, 'ranges': gates}, indent=2))
    raise SystemExit(bool(strict) or not all(gates.values()))


if __name__ == '__main__':
    main()
