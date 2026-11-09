#!/usr/bin/env python3
"""The ender dragon's death roar, and a silent dragon's lack of one, against vanilla's dedicated server.

A vanilla dedicated server and Horizon's each serve a fresh flat world; a vanilla-codec client
(VanillaProtocolClient, level-events mode: LevelEventTranscript) joins. With the ticks frozen the console summons a
dead dragon (Health 0, NoAI, in the overworld: no fight; EnderDragon.kill would remove it with no death, and
/damage on a dragon is a gap of Horizon's) and steps 210 ticks, through its death (LivingEntity.baseTick runs
tickDeath while isDeadOrDying) and removal: at the death's first tick EnderDragon.tickDeath sends the global level event 1028 (the roar heard everywhere) unless the
dragon is silent (`dragonDeathTime == 1 && !isSilent()`). Once with a loud dragon, then with a `Silent:1b` one.
Each side's 1028 events (phase, position, data, global) must match vanilla's: one for the loud dragon, none for
the silent one.
"""
import argparse
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
# The phase and the dragon's Silent flag.
PHASES = [('loud', False), ('silent', True)]
# Past the 200 death ticks, and the removal after them.
STEPS = 210
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')


def run_side(label, command, client_command, out):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    port = free_port()
    props = {'server-ip': '127.0.0.1', 'server-port': str(port), 'online-mode': 'false', 'white-list': 'false',
             'level-type': 'minecraft:flat', 'generate-structures': 'false', 'level-seed': '42',
             'gamemode': 'creative', 'force-gamemode': 'true', 'allow-flight': 'true',
             'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    transcript = out / f'{label}-packets.txt'
    server = Server(command, directory, label)
    client = None
    serial = 0

    def reply(text):
        nonlocal serial
        serial += 1
        marker = f'dragon_roar_barrier_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=180)
        return [m.group(1) for line in server.lines[start:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def advance(ticks):
        start = game_time(server)
        reply(f'tick step {ticks}')
        deadline = time.monotonic() + 300
        while game_time(server) < start + ticks:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'
        assert game_time(server) == start + ticks

    try:
        server.wait_for(r'Done \(')
        with (out / f'{label}-client.log').open('w') as log:
            client = subprocess.Popen([*map(str, client_command), '127.0.0.1', str(port), PLAYER,
                                       str(transcript), 'level-events'], cwd=lab.ROOT,
                                      stdout=log, stderr=subprocess.STDOUT)
            server.wait_for(rf'<{PLAYER}> summon the scene', timeout=180)
            reply('tick freeze')
            reply('gamerule minecraft:spawn_mobs false')
            reply('kill @e[type=!minecraft:player]')
            reply('forceload add 0 0 15 15')
            deadline = time.monotonic() + 180
            while not any('Test passed' in line for line in reply('execute if loaded 0 220 0')):
                assert time.monotonic() < deadline, f'{label}: scene chunk did not load'
            reply(f'tp {PLAYER} 8.5 220 11.5')
            reply('tick rate 200')
            for name, silent in PHASES:
                reply('summon minecraft:ender_dragon 8.5 230 3.5 '
                      f'{{NoAI:1b,Health:0.0f,Silent:{int(silent)}b,Tags:["dragon_roar"]}}')
                # A dead entity is in no selector (EntitySelector: isAlive): the loud phase's roar, matched
                # with vanilla's, is what shows the dragon was summoned dying on both sides.
                server.console(f'say level_events_{name}')
                server.wait_for(rf'<{PLAYER}> level events phase {name}', timeout=120)
                advance(STEPS)
            server.console('say level_events_done')
            client.wait(timeout=180)
    finally:
        if client is not None and client.poll() is None:
            client.kill()
            client.wait()
        server.stop()
    lines = transcript.read_text().splitlines()
    assert not any(line.startswith('error ') for line in lines), lines[-10:]
    events = []
    for line in lines:
        if line.startswith('play-level-event '):
            phase, kind, at, data, global_event = line.split()[1:]
            events.append({'phase': phase, 'type': int(kind), 'at': at, 'data': int(data),
                           'global': global_event == 'true'})
    return {'roars': [e for e in events if e['type'] == 1028]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon game (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'dragon-death-roar-parity')
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
                'horizon': [binary, '--resources', lab.RESOURCES.resolve(),
                            '--dedicated', '--nogui']}
    results = {label: run_side(label, command, client, out) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    vanilla, horizon = results['vanilla']['roars'], results['horizon']['roars']
    print(f'vanilla roars: {vanilla}', flush=True)
    print(f'horizon roars: {horizon}', flush=True)
    # The spec as read from EnderDragon.tickDeath: one global roar for the loud dragon, none for the silent one.
    assert [e['phase'] for e in vanilla] == ['loud'] and vanilla[0]['global'], vanilla
    assert horizon == vanilla, f'death roars differ from vanilla; see {out}'
    print('dragon death roar: matches vanilla', flush=True)


if __name__ == '__main__':
    main()
