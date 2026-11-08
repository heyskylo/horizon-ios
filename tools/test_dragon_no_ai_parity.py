#!/usr/bin/env python3
"""An ender dragon with NoAI, against vanilla's dedicated server.

EnderDragon.aiStep returns right after the flap and checkCrystals when isNoAi() (flapTime = 0.5): no flight
history, phase, steering or parts, so the dragon stays where it is. A vanilla dedicated server and Horizon's each
serve a fresh flat world; the console summons a NoAI dragon in a force-loaded chunk of the overworld, steps 100
ticks, and reads its position and its NoAI back; then saves, restarts and reads them again; then a vanilla-codec
client (VanillaProtocolClient, entities mode) joins and records the entity data the dragon is paired with. All
must match vanilla's: the dragon where it was summoned, NoAI kept through the save, and NoAI's bit of
Mob.DATA_MOB_FLAGS_ID (15) sent to the client (EnderDragon.aiStep on the client stops there too).
"""
import argparse
import atexit
import json
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
DATA = re.compile(r'has the following entity data: (.*)$')
STEPS = 100


def run_side(label, command, client_command, out):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    port = free_port()
    props = {'server-ip': '127.0.0.1', 'server-port': str(port), 'online-mode': 'false', 'white-list': 'false',
             'level-type': 'minecraft:flat', 'generate-structures': 'false', 'level-seed': '42',
             'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    servers = []
    atexit.register(lambda: [s.process.kill() for s in servers if s.process.poll() is None])
    serial = 0

    def reply(server, text):
        nonlocal serial
        serial += 1
        marker = f'dragon_no_ai_barrier_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=180)
        return [m.group(1) for line in server.lines[start:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def read(server):
        found = {}
        for key in ('Pos', 'NoAI'):
            data = [m.group(1) for line in reply(server, f'data get entity @e[tag=no_ai,limit=1] {key}')
                    if (m := DATA.search(line))]
            found[key] = data[0] if data else None
        return found

    reads = {}
    server = Server(command, directory, label)
    servers.append(server)
    try:
        server.wait_for(r'Done \(')
        reply(server, 'tick freeze')
        reply(server, 'gamerule minecraft:spawn_mobs false')
        reply(server, 'forceload add 0 0 15 15')
        deadline = time.monotonic() + 180
        while not any('Test passed' in line for line in reply(server, 'execute if loaded 8 100 8')):
            assert time.monotonic() < deadline, f'{label}: scene chunk did not load'
        reply(server, 'kill @e[type=!minecraft:player]')
        reply(server, 'summon minecraft:ender_dragon 8.5 100 8.5 {NoAI:1b,Tags:["no_ai"]}')
        start = game_time(server)
        reply(server, f'tick step {STEPS}')
        deadline = time.monotonic() + 180
        while game_time(server) < start + STEPS:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'
        reads['stepped'] = read(server)
        reply(server, 'save-all flush')
        server.stop()
        server = Server(command, directory, f'{label}-restart')
        servers.append(server)
        server.wait_for(r'Done \(')
        # A chunk's entities load a few ticks after it (PersistentEntitySectionManager).
        deadline = time.monotonic() + 60
        while not any('Test passed' in line for line in reply(server, 'execute if entity @e[tag=no_ai]')):
            if time.monotonic() > deadline:
                break
        reads['restarted'] = read(server)
        # What a client is told: the dragon's pairing data carries Mob.DATA_MOB_FLAGS_ID (15), NoAI its first bit.
        # The client records the entities added once it asks for them: a new dragon then.
        reply(server, 'kill @e[tag=no_ai]')
        transcript = out / f'{label}-packets.txt'
        with (out / f'{label}-client.log').open('w') as log:
            client = subprocess.Popen([*map(str, client_command), '127.0.0.1', str(port), PLAYER, str(transcript),
                                       'entities'], cwd=lab.ROOT, stdout=log, stderr=subprocess.STDOUT)
            servers.append(type('C', (), {'process': client})())
            server.wait_for(rf'<{PLAYER}> summon the entities', timeout=180)
            reply(server, 'summon minecraft:ender_dragon 8.5 100 8.5 {NoAI:1b,Tags:["no_ai"]}')
            client.wait(timeout=180)
        lines = [line for line in transcript.read_text().splitlines()
                 if line.startswith('play-entities entity minecraft:ender_dragon ')]
        assert len(lines) == 1, (label, lines)
        pairing = re.search(r'[|] pairing ([^|]*)[|]', lines[0])
        flags = [kv.split('=', 1)[1] for kv in pairing.group(1).split() if kv.startswith('15=')] if pairing else []
        reads['client_flags'] = flags[0] if flags else None
    finally:
        server.stop()
    return reads


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon game (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'dragon-no-ai-parity')
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
    classes = out / 'classes'
    classes.mkdir(exist_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java'
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', source, '-cp', cp, '-d', classes,
                    source / 'dev/horizonlab/parity/VanillaProtocolClient.java'], check=True)
    client = ['java', '-Xmx1g', '-cp', str(classes) + os.pathsep + cp, 'dev.horizonlab.parity.VanillaProtocolClient']
    results = {label: run_side(label, command, client, out) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    for label, result in results.items():
        print(f'{label}: {result}', flush=True)
    # The spec as read from EnderDragon.aiStep: a NoAI dragon stays where it was summoned, and keeps NoAI.
    still = {'Pos': '[8.5d, 100.0d, 8.5d]', 'NoAI': '1b'}
    assert results['vanilla'] == {'stepped': still, 'restarted': still, 'client_flags': '1'}, results['vanilla']
    assert results['horizon'] == results['vanilla'], f'a NoAI dragon differs from vanilla; see {out}'
    print('dragon with NoAI: matches vanilla', flush=True)


if __name__ == '__main__':
    main()
