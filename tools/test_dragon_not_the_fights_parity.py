#!/usr/bin/env python3
"""An ender dragon that is not the End fight's own, dying in the End, against vanilla's dedicated server.

EnderDragonFight.setDragonKilled builds the exit portal, a gateway and the egg only for the fight's own dragon
(dragon.getUUID().equals(dragonUUID)); EnderDragon.aiStep joins a dragon to the fight only when its UUID is the
fight's, and tickDeath gives the first kill's 12000 experience only with a fight. The fight takes a dragon of its
own (findOrCreateDragon) only while players stand in it, and never once it is won. A vanilla dedicated server and
Horizon's each serve a fresh world with nobody in the End; the console summons a dying dragon (Health 0, NoAI:
LivingEntity.baseTick runs tickDeath while isDeadOrDying) in the End and waits past its 200 death ticks. Then the
dragon egg, end portal and bedrock blocks around 0 0, the End gateways at the 20 places a fight puts them, and the
experience its orbs hold must match vanilla's (none of the blocks; 500 experience).
"""
import argparse
import atexit
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

try:
    from . import lab, test_rust_parity as base
    from .test_dedicated_server_parity import Server, free_port, game_time
    from .test_server_world_gen_parity import escape
except ImportError:
    import lab
    import test_rust_parity as base
    from test_dedicated_server_parity import Server, free_port, game_time
    from test_server_world_gen_parity import escape

MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')
END = 'execute in minecraft:the_end run '
# Where setDragonKilled builds the exit portal and its egg: the column at 0 0 of the End.
BOX = '-4 0 -4 4 120 4'
BLOCKS = ['dragon_egg', 'end_portal', 'bedrock']
# spawnNewGateway: gateway i of the 20 is 96 blocks out at y 75.
GATEWAYS = [(math.floor(96 * math.cos(a)), math.floor(96 * math.sin(a)))
            for a in (2 * (-math.pi + math.pi / 20 * i) for i in range(20))]
FILLED = re.compile(r'Successfully filled (\d+) block')
VALUE = re.compile(r'has the following entity data: (-?\d+)')


def run_side(label, command, out):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    props = {'server-ip': '127.0.0.1', 'server-port': str(free_port()), 'online-mode': 'false',
             'generate-structures': 'false', 'level-seed': '42',
             'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    server = Server(command, directory, label)
    atexit.register(lambda: server.process.poll() is None and server.process.kill())
    serial = 0

    def reply(text):
        nonlocal serial
        serial += 1
        marker = f'dragon_fight_barrier_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=180)
        return [m.group(1) for line in server.lines[start:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def passed(text):
        return any('Test passed' in line for line in reply(text))

    try:
        server.wait_for(r'Done \(')
        reply('tick freeze')
        reply('gamerule minecraft:spawn_mobs false')
        # The exit portal's column and every gateway's place.
        reply(END + 'forceload add -100 -100 99 99')
        deadline = time.monotonic() + 300
        while not all(passed(END + f'execute if loaded {x} 64 {z}') for x, z in [(-8, -8), (8, 8), *GATEWAYS]):
            assert time.monotonic() < deadline, f'{label}: scene chunks did not load'
        reply(END + 'kill @e[type=!minecraft:player]')
        reply(END + 'summon minecraft:ender_dragon 8.5 100 3.5 {NoAI:1b,Health:0.0f}')
        start = game_time(server)
        reply('tick step 240')
        deadline = time.monotonic() + 180
        while game_time(server) < start + 240:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'
        blocks = {}
        for block in BLOCKS:
            filled = [m for line in reply(END + f'fill {BOX} minecraft:air replace minecraft:{block}')
                      if (m := FILLED.search(line))]
            blocks[block] = int(filled[0].group(1)) if filled else 0
        blocks['end_gateway'] = sum(passed(END + f'execute if block {x} 75 {z} minecraft:end_gateway')
                                    for x, z in GATEWAYS)
        orb = '@e[type=minecraft:experience_orb,limit=1]'
        experience = 0
        for _ in range(500):
            value = [m for line in reply(END + f'data get entity {orb} Value') if (m := VALUE.search(line))]
            if not value:
                break
            count = [m for line in reply(END + f'data get entity {orb} Count') if (m := VALUE.search(line))]
            experience += int(value[0].group(1)) * (int(count[0].group(1)) if count else 1)
            reply(END + f'kill {orb}')
        else:
            raise AssertionError(f'{label}: more than 500 experience orbs')
    finally:
        server.stop()
    return {'blocks': blocks, 'experience': experience}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon game (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'dragon-not-the-fights-parity')
    args = parser.parse_args()
    lock, jars, libraries = base.verify_artifacts()
    if args.binary:
        binary = args.binary.resolve(strict=True)
    else:
        subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client',
                        '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
        binary = Path(metadata['target_directory']) / 'fast' / (
            'horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    commands = {'vanilla': ['java', '-Xmx2g', '-cp', cp, 'net.minecraft.server.Main', 'nogui'],
                'horizon': [binary, '--resources', lab.RESOURCES.resolve(), '--dedicated', '--nogui']}
    results = {label: run_side(label, command, out) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    for label, result in results.items():
        print(f'{label}: {result}', flush=True)
    # The spec as read from setDragonKilled and tickDeath: not the fight's dragon, so nothing built and 500.
    nothing = {'blocks': dict.fromkeys([*BLOCKS, 'end_gateway'], 0), 'experience': 500}
    assert results['vanilla'] == nothing, results['vanilla']
    assert results['horizon'] == results['vanilla'], f"the death of a dragon not the fight's differs; see {out}"
    print("dragon not the fight's: matches vanilla", flush=True)


if __name__ == '__main__':
    main()
