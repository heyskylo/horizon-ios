#!/usr/bin/env python3
"""Ender dragons that are not the End fight's own, dying in the End, against vanilla's dedicated server.

EnderDragonFight.setDragonKilled builds the exit portal, a gateway and the egg only for the fight's own dragon
(dragon.getUUID().equals(dragonUUID)); EnderDragon.aiStep joins a dragon to the fight only when its UUID is the
fight's, and tickDeath gives the first kill's 12000 experience only with a fight. The fight takes a dragon
(findOrCreateDragon) only while a player stands in it, only a living one (ServerLevel.getDragons), and never
once it is won. A vanilla dedicated server and Horizon's each serve a fresh world; a vanilla-codec client
(VanillaProtocolClient, level-events mode, only to be a player) joins in the overworld. With the ticks frozen the
console summons a dying dragon (Health 0, NoAI: LivingEntity.baseTick runs tickDeath while isDeadOrDying) in the
End and steps past its 200 death ticks, three times:
- alone: nobody is in the End, so there is no fight to join;
- arriving: the player is sent to the End with it, so the fight begins, passes over the dying dragon and makes its
  own;
- won: the fight's dragon is killed (/kill: setDragonKilled, a gateway and the egg), then the dying one is summoned.
After each, the dragon egg and end portal blocks around 0 0, the End gateways at the 20 places a fight puts them,
the experience its orbs hold and the living dragons must match vanilla's.
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

PLAYER = 'Tester'
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')
END = 'execute in minecraft:the_end run '
# Where setDragonKilled builds the exit portal and its egg: the column at 0 0 of the End.
BOX = '-4 0 -4 4 120 4'
BLOCKS = ['dragon_egg', 'end_portal']
# spawnNewGateway: gateway i of the 20 is 96 blocks out at y 75.
GATEWAYS = [(math.floor(96 * math.cos(a)), math.floor(96 * math.sin(a)))
            for a in (2 * (-math.pi + math.pi / 20 * i) for i in range(20))]
DYING = 'summon minecraft:ender_dragon 8.5 100 3.5 {NoAI:1b,Health:0.0f}'
STEPS = 240
FILLED = re.compile(r'Successfully filled (\d+) block')
VALUE = re.compile(r'has the following entity data: (-?\d+)')
COUNT = re.compile(r'Test passed, count: (\d+)|Test passed. Count: (\d+)', re.IGNORECASE)


def run_side(label, command, client_command, out):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    port = free_port()
    props = {'server-ip': '127.0.0.1', 'server-port': str(port), 'online-mode': 'false', 'white-list': 'false',
             'generate-structures': 'false', 'level-seed': '42', 'gamemode': 'creative', 'force-gamemode': 'true',
             'allow-flight': 'true', 'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    transcript = out / f'{label}-packets.txt'
    server = Server(command, directory, label)
    atexit.register(lambda: server.process.poll() is None and server.process.kill())
    client = None
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

    def advance(ticks):
        start = game_time(server)
        reply(f'tick step {ticks}')
        deadline = time.monotonic() + 300
        while game_time(server) < start + ticks:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'

    def measure():
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
        dragons = [m for line in reply(END + 'execute if entity @e[type=minecraft:ender_dragon]')
                   if (m := COUNT.search(line))]
        return {'blocks': blocks, 'experience': experience,
                'living dragons': int(next(g for g in dragons[0].groups() if g)) if dragons else 0}

    results = {}
    try:
        server.wait_for(r'Done \(')
        with (out / f'{label}-client.log').open('w') as log:
            client = subprocess.Popen([*map(str, client_command), '127.0.0.1', str(port), PLAYER,
                                       str(transcript), 'level-events'], cwd=lab.ROOT,
                                      stdout=log, stderr=subprocess.STDOUT)
            server.wait_for(rf'<{PLAYER}> summon the scene', timeout=180)
            reply('tick freeze')
            reply('gamerule minecraft:spawn_mobs false')
            # The exit portal's column, every gateway's place and where the player stands.
            reply(END + 'forceload add -100 -100 99 99')
            deadline = time.monotonic() + 300
            while not all(passed(END + f'execute if loaded {x} 64 {z}')
                          for x, z in [(-8, -8), (8, 8), (0, 90), *GATEWAYS]):
                assert time.monotonic() < deadline, f'{label}: scene chunks did not load'
            reply(END + 'kill @e[type=!minecraft:player]')
            reply(END + DYING)
            advance(STEPS)
            results['alone'] = measure()
            # Far from the orbs (a player picks them up), within 192 blocks of 0 128 0 (the fight's players).
            reply(END + DYING)
            reply(f'execute in minecraft:the_end run tp {PLAYER} 0.5 80 90.5')
            advance(STEPS)
            results['arriving'] = measure()
            reply(END + 'kill @e[type=minecraft:ender_dragon]')
            advance(1)
            reply(END + DYING)
            advance(STEPS)
            results['won'] = measure()
            server.console('say level_events_done')
            client.wait(timeout=180)
    finally:
        if client is not None and client.poll() is None:
            client.kill()
            client.wait()
        server.stop()
    lines = transcript.read_text().splitlines()
    assert not any(line.startswith('error ') for line in lines), lines[-10:]
    return results


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
    classes = out / 'classes'
    classes.mkdir(exist_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java'
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', source, '-cp', cp, '-d', classes,
                    source / 'dev/horizonlab/parity/VanillaProtocolClient.java'], check=True)
    client = ['java', '-Xmx1g', '-cp', str(classes) + os.pathsep + cp,
              'dev.horizonlab.parity.VanillaProtocolClient']
    commands = {'vanilla': ['java', '-Xmx2g', '-cp', cp, 'net.minecraft.server.Main', 'nogui'],
                'horizon': [binary, '--resources', lab.RESOURCES.resolve(), '--dedicated', '--nogui']}
    results = {label: run_side(label, command, client, out) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    for label, result in results.items():
        for case, value in result.items():
            print(f'{label} {case}: {value}', flush=True)
    # The spec as read from setDragonKilled, findOrCreateDragon and tickDeath: no dying dragon is the fight's,
    # so each is worth 500 and builds nothing; the fight that began makes a dragon of its own, and its /kill
    # builds the exit portal, the egg and one gateway.
    nothing = dict.fromkeys([*BLOCKS, 'end_gateway'], 0)
    vanilla = results['vanilla']
    assert vanilla['alone'] == {'blocks': nothing, 'experience': 500, 'living dragons': 0}, vanilla['alone']
    assert vanilla['arriving'] == {'blocks': nothing, 'experience': 500, 'living dragons': 1}, vanilla['arriving']
    assert vanilla['won']['blocks']['end_gateway'] == 1 and vanilla['won']['blocks']['dragon_egg'] == 1, vanilla
    assert vanilla['won']['experience'] == 500 and vanilla['won']['living dragons'] == 0, vanilla['won']
    assert results['horizon'] == vanilla, f"the deaths of dragons not the fight's differ; see {out}"
    print("dragons not the fight's: matches vanilla", flush=True)


if __name__ == '__main__':
    main()
